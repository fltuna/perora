//! The editor: the capture, what is drawn on it, and the tools to draw with. It shows the picture as
//! closely as the user looks at it, takes presses and keys for the tool in hand, and keeps the
//! drawing (`perora_core::drawing`). What leaves (saving, copying) is the window's: the editor tells
//! it what was asked (`Event`), and the window makes the image with the same drawing (`paint`).
//!
//! P4 of docs/gpui-rewrite.md, first part: the picture, looking closer and moving it, the pen, undo
//! and redo. The other tools, the selection, the second bar and its panels come next.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyView, App, Bounds, Context, CursorStyle, DispatchPhase, EventEmitter, FocusHandle,
    Focusable, Hsla, InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Render, ScrollDelta, ScrollEvent, SharedString, StatefulInteractiveElement, Styled, Window,
    canvas, div, point, px,
};
use perora_core::drawing::{Drawing, unit};
use perora_core::edit::{Edit, Place};
use perora_core::language::Language;
use perora_ui::{Theme, Tip};

use crate::icons;
use crate::marks;
use crate::paint::{self, Painter};
use crate::picture::Picture;
use crate::tools::{self, Kind, Press, Tool};
use crate::view::{self, Area, Looking};

/// A capture to draw on.
pub struct Capture {
    pub id: u64,
    pub picture: Rc<Picture>,
}

/// What a capture leaves as, when a key asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    Save,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The drawing changed.
    Changed,
    /// A key asks for the capture to leave.
    Leave(Leave),
    /// Esc, with nothing to let go of and nothing drawn: the capture is thrown away.
    Discard,
}

/// What the pointer is doing with a button held.
enum Gesture {
    /// The tool in hand is drawing (with the left button).
    Draw,
    /// The picture is held and moved, with that button: the place that was in the middle when it
    /// was taken hold of, and where the pointer was (logical pixels in the place).
    Pan {
        from: (f32, f32),
        was: Place,
        button: MouseButton,
    },
}

impl Gesture {
    fn button(&self) -> MouseButton {
        match self {
            Gesture::Draw => MouseButton::Left,
            Gesture::Pan { button, .. } => *button,
        }
    }
}

pub struct Editor {
    capture: Option<Capture>,
    drawing: Drawing,
    /// The drawing as the window draws it: made anew only when it changes.
    shown: Rc<Vec<Edit>>,
    tool: Kind,
    pen: tools::pen::Pen,
    looking: Looking,
    /// Where the picture is shown, as last laid out (logical pixels of the window; its corner on a
    /// whole pixel of the screen).
    stage: Rc<Cell<Option<Bounds<Pixels>>>>,
    gesture: Option<Gesture>,
    /// Space is held: the pointer takes hold of the picture, not of a tool.
    grabbing: bool,
    /// The last change was refused: the drawing holds no more (said until something is drawn).
    full: bool,
    /// While the capture leaves: nothing is drawn, no key does anything.
    frozen: bool,
    focus: FocusHandle,
    language: Language,
    /// What the window puts at the top right (what the capture leaves as) and under the picture at
    /// the left (what is said of it).
    pub top_right: Option<AnyView>,
    pub below: Option<AnyView>,
    /// Pictures no longer shown: the GPU lets go of them at the next drawing.
    retired: Vec<Rc<Picture>>,
}

impl EventEmitter<Event> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Editor {
    pub fn new(language: Language, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The window left with Space held: the pointer is the tool's again (its letting go is
        // another window's to hear).
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.grabbing {
                this.grabbing = false;
                cx.notify();
            }
        })
        .detach();
        Self {
            capture: None,
            drawing: Drawing::default(),
            shown: Rc::new(Vec::new()),
            tool: Kind::Select,
            pen: tools::pen::Pen::default(),
            looking: Looking::default(),
            stage: Rc::new(Cell::new(None)),
            gesture: None,
            grabbing: false,
            full: false,
            frozen: false,
            focus: cx.focus_handle(),
            language,
            top_right: None,
            below: None,
            retired: Vec::new(),
        }
    }

    /// Another capture, drawn on anew: nothing of the one before is left (what is drawn, what is
    /// undone, the tool, the look).
    pub fn show(&mut self, capture: Capture, cx: &mut Context<Self>) {
        if let Some(old) = self.capture.take() {
            self.retired.push(old.picture);
        }
        self.capture = Some(capture);
        self.drawing = Drawing::default();
        self.shown = Rc::new(Vec::new());
        self.tool = Kind::Select;
        self.pen = tools::pen::Pen::default();
        self.looking = Looking::default();
        self.gesture = None;
        self.grabbing = false;
        self.full = false;
        self.frozen = false;
        cx.notify();
    }

    pub fn capture_id(&self) -> Option<u64> {
        self.capture.as_ref().map(|capture| capture.id)
    }

    pub fn edits(&self) -> &[Edit] {
        self.drawing.edits()
    }

    /// Whether the last change was refused for a drawing that holds no more.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// The capture leaves (or stops leaving): meanwhile nothing is drawn, no key does anything.
    pub fn set_frozen(&mut self, frozen: bool, cx: &mut Context<Self>) {
        if frozen {
            self.give_up_gesture();
        }
        self.frozen = frozen;
        cx.notify();
    }

    /// Before the capture leaves, or the window is put out of the way: what is under way is done or
    /// given up, and a Space held is let go. (P4 later: the text being written is written, the
    /// frame being cut to is cut to.)
    pub fn settle(&mut self, cx: &mut Context<Self>) {
        self.give_up_gesture();
        self.grabbing = false;
        cx.notify();
    }

    fn give_up_gesture(&mut self) {
        if let Some(Gesture::Draw) = self.gesture.take() {
            self.pen.give_up();
        }
    }

    /// The picture: all of the image (P4 later: what it is cut down to).
    fn view(&self) -> Option<Area> {
        self.capture.as_ref().map(|capture| capture.picture.whole())
    }

    fn unit(&self) -> f32 {
        self.capture.as_ref().map_or(1.0, |capture| {
            unit(capture.picture.width, capture.picture.height)
        })
    }

    /// The look in the place the picture was last shown in.
    fn look(&self, window: &Window) -> Option<(Bounds<Pixels>, view::Look)> {
        let stage = self.stage.get()?;
        let view = self.view()?;
        let size = (stage.size.width.as_f32(), stage.size.height.as_f32());
        Some((
            stage,
            view::look(size, view, self.looking, window.scale_factor()),
        ))
    }

    fn local(stage: Bounds<Pixels>, position: Point<Pixels>) -> (f32, f32) {
        (
            (position.x - stage.origin.x).as_f32(),
            (position.y - stage.origin.y).as_f32(),
        )
    }

    fn press(
        &self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        window: &Window,
    ) -> Option<Press> {
        let (stage, look) = self.look(window)?;
        let (x, y) = Self::local(stage, position);
        Some(Press {
            at: look.in_image(x, y),
            scale: look.scale,
            unit: self.unit(),
            shift: modifiers.shift,
        })
    }

    fn tool_mut(&mut self) -> Option<&mut dyn Tool> {
        match self.tool {
            Kind::Select => None,
            Kind::Pen => Some(&mut self.pen),
        }
    }

    fn tool_ref(&self) -> Option<&dyn Tool> {
        match self.tool {
            Kind::Select => None,
            Kind::Pen => Some(&self.pen),
        }
    }

    fn down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.frozen || self.capture.is_none() || self.gesture.is_some() {
            return;
        }
        window.focus(&self.focus, cx);
        let Some((stage, look)) = self.look(window) else {
            return;
        };
        let held = event.button == MouseButton::Middle
            || (event.button == MouseButton::Left && self.grabbing);
        if held {
            self.gesture = Some(Gesture::Pan {
                from: Self::local(stage, event.position),
                was: look.middle,
                button: event.button,
            });
            cx.notify();
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some(press) = self.press(event.position, event.modifiers, window) else {
            return;
        };
        let begins_beside = self.tool.begins_beside();
        let on_picture = self.view().is_some_and(|view| view.contains(press.at));
        if !on_picture && !begins_beside {
            return;
        }
        if let Some(tool) = self.tool_mut()
            && tool.begin(&press)
        {
            self.gesture = Some(Gesture::Draw);
            cx.notify();
        }
    }

    fn moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The button was let go where the window did not hear it (another window took the mouse):
        // what it was doing ends there.
        if self.gesture.is_some() && event.pressed_button.is_none() {
            self.finish(event.position, event.modifiers, window, cx);
            return;
        }
        match &self.gesture {
            None => {}
            Some(Gesture::Pan { from, was, button }) => {
                let (from, was, button) = (*from, *was, *button);
                if let (Some((stage, look)), Some(view)) = (self.look(window), self.view()) {
                    let to = Self::local(stage, event.position);
                    let moved = view::moved(self.looking, was, from, to, look.scale);
                    // As far as its middle can go, and held there: back from past that, it comes
                    // along at once, not once the pointer is back where it stopped.
                    let asked = moved.middle.unwrap_or(look.middle);
                    let middle = [
                        asked[0].clamp(view.x, view.right()),
                        asked[1].clamp(view.y, view.bottom()),
                    ];
                    if middle != asked {
                        self.gesture = Some(Gesture::Pan {
                            from: to,
                            was: middle,
                            button,
                        });
                    }
                    self.looking = Looking {
                        times: look.times,
                        middle: Some(middle),
                    };
                    cx.notify();
                }
            }
            Some(Gesture::Draw) => {
                if let Some(press) = self.press(event.position, event.modifiers, window)
                    && let Some(tool) = self.tool_mut()
                {
                    tool.go_on(&press);
                    cx.notify();
                }
            }
        }
    }

    fn up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        // (Only the button it began with ends it: another one let go is nothing to it.)
        if self
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.button() == event.button)
        {
            self.finish(event.position, event.modifiers, window, cx);
        }
    }

    /// The button the gesture began with is let go there: what was drawn is kept.
    fn finish(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.gesture.take() {
            None => {}
            Some(Gesture::Pan { .. }) => cx.notify(),
            Some(Gesture::Draw) => {
                let press = self.press(position, modifiers, window);
                let drawn = match (press, self.tool_mut()) {
                    (Some(press), Some(tool)) => tool.end(&press),
                    (None, Some(tool)) => {
                        tool.give_up();
                        None
                    }
                    _ => None,
                };
                // What was drawn wholly beside the picture is not kept: it is not on it.
                if let (Some(edit), Some(view)) = (drawn, self.view())
                    && marks::mark_of(&edit).is_some_and(|mark| mark.area(&edit).meets(&view))
                {
                    let mut edits = self.drawing.edits().to_vec();
                    edits.push(edit);
                    self.change(edits, cx);
                }
                cx.notify();
            }
        }
    }

    fn wheel(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.frozen || self.gesture.is_some() {
            return;
        }
        let Some(stage) = self.stage.get() else {
            return;
        };
        let Some(view) = self.view() else {
            return;
        };
        let lines = match event.delta {
            ScrollDelta::Lines(lines) => lines.y,
            // (A browser's hundred pixels a notch, three lines.)
            ScrollDelta::Pixels(pixels) => pixels.y.as_f32() * 3.0 / 100.0,
        };
        let size = (stage.size.width.as_f32(), stage.size.height.as_f32());
        if let Some(looking) = view::wheeled(
            size,
            view,
            self.looking,
            window.scale_factor(),
            Self::local(stage, event.position),
            lines,
        ) {
            self.looking = looking;
            cx.notify();
        }
    }

    /// The drawing, changed: one step to undo. Not when it would hold more than a drawing does.
    fn change(&mut self, edits: Vec<Edit>, cx: &mut Context<Self>) {
        match self.drawing.change(edits) {
            Ok(changed) => {
                self.full = false;
                if changed {
                    self.drawn(cx);
                }
            }
            Err(_) => self.full = true,
        }
        cx.notify();
    }

    fn drawn(&mut self, cx: &mut Context<Self>) {
        self.shown = Rc::new(self.drawing.edits().to_vec());
        cx.emit(Event::Changed);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_none() && self.drawing.undo() {
            self.full = false;
            self.drawn(cx);
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_none() && self.drawing.redo() {
            self.drawn(cx);
        }
    }

    /// Takes up a tool (by its button or its key).
    fn take(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if self.gesture.is_some() || kind == self.tool {
            return;
        }
        self.tool = kind;
        cx.notify();
    }

    /// Esc lets go of one thing at a time: what is being drawn, then the tool. With nothing drawn,
    /// it throws the capture away; with something drawn, it does not (that is the bin's).
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_some() {
            self.give_up_gesture();
            cx.notify();
            return;
        }
        if self.tool != Kind::Select {
            self.tool = Kind::Select;
            cx.notify();
            return;
        }
        if self.drawing.is_empty() {
            cx.emit(Event::Discard);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if self.frozen {
            cx.stop_propagation();
            return;
        }
        if key == "escape" {
            if !event.is_held {
                self.escape(cx);
            }
            return;
        }
        // While a button is held, no key does anything but Esc: nothing leaves half drawn.
        if self.gesture.is_some() {
            return;
        }
        if modifiers.control && !modifiers.alt && !modifiers.platform {
            match key {
                "z" if !modifiers.shift => self.undo(cx),
                "z" | "y" => self.redo(cx),
                "s" if !event.is_held => cx.emit(Event::Leave(Leave::Save)),
                // (P5: uploading, when it is on offer.)
                "enter" if !event.is_held && !modifiers.shift => cx.emit(Event::Leave(Leave::Save)),
                _ => {}
            }
            return;
        }
        if modifiers.alt || modifiers.platform || modifiers.control {
            return;
        }
        match key {
            "space" => {
                if !self.grabbing {
                    self.grabbing = true;
                    cx.notify();
                }
            }
            "0" => {
                self.looking = Looking::default();
                cx.notify();
            }
            _ if !event.is_held => {
                if let Some(kind) = Kind::ALL.into_iter().find(|kind| kind.key() == key) {
                    self.take(kind, cx);
                }
            }
            _ => {}
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" && self.grabbing {
            self.grabbing = false;
            cx.notify();
        }
    }
}

/// A tooltip that says this.
fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| Tip::view(text.clone(), cx)
}

/// Segoe Fluent Icons, as the Tauri version has them.
const UNDO: &str = "\u{E7A7}";
const REDO: &str = "\u{E7A6}";
const DELETE: &str = "\u{E74D}";
/// The zoom button's (the Tauri version draws its own: a frame with corners).
const WHOLE: &str = "\u{E740}";

fn glyph(glyph: &'static str, size: f32) -> gpui::Div {
    div()
        .font_family("Segoe Fluent Icons")
        .text_size(px(size))
        .line_height(px(size))
        .child(glyph)
}

impl Editor {
    fn tools_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = |key: &str| self.language.text(key, &[]);
        let button = |id: SharedString, current: bool, enabled: bool| {
            div()
                .id(id)
                .w(px(36.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius_small)
                .border_1()
                .border_color(if current {
                    theme.accent
                } else {
                    gpui::transparent_black()
                })
                .when(current, |button| button.bg(theme.surface))
                .when(!enabled, |button| button.opacity(0.4))
                .when(enabled && !current, |button| {
                    button.hover(|style| style.bg(theme.surface_hover))
                })
        };
        let mut bar = div().flex().items_center().gap(px(2.0));
        for kind in Kind::ALL {
            let colour = match kind {
                Kind::Pen => Some(paint::colour(self.pen.colour)),
                Kind::Select => None,
            };
            let named = format!("{} ({})", t(kind.name()), kind.key().to_uppercase());
            bar = bar.child(
                button(kind.name().into(), self.tool == kind, true)
                    .tooltip(tip(named))
                    .on_click(cx.listener(move |this, _, _, cx| this.take(kind, cx)))
                    .child(icons::icon(icons::of_tool(kind, colour), 16.0, theme.text)),
            );
        }
        let typing = false;
        let (can_undo, can_redo) = (
            self.drawing.can_undo() && !typing,
            self.drawing.can_redo() && !typing,
        );
        bar.child(div().w(px(1.0)).h(px(20.0)).mx(px(6.0)).bg(theme.border))
            .child(
                button("undo".into(), false, can_undo)
                    .tooltip(tip(format!("{} (Ctrl+Z)", t("editor.undo"))))
                    .when(can_undo, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| this.undo(cx)))
                    })
                    .child(glyph(UNDO, 16.0)),
            )
            .child(
                button("redo".into(), false, can_redo)
                    .tooltip(tip(format!("{} (Ctrl+Y)", t("editor.redo"))))
                    .when(can_redo, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| this.redo(cx)))
                    })
                    .child(glyph(REDO, 16.0)),
            )
            .child(
                // (P4 later: what is selected.)
                button("delete".into(), false, false)
                    .tooltip(tip(format!("{} (Delete)", t("editor.delete"))))
                    .child(glyph(DELETE, 16.0)),
            )
    }

    fn stage(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        let stage = self.stage.clone();
        let picture = self.capture.as_ref().map(|capture| capture.picture.clone());
        let view = self.view();
        let looking = self.looking;
        let edits = self.shown.clone();
        let shaping = self.tool_ref().and_then(|tool| tool.shaping());
        let retired = std::mem::take(&mut self.retired);
        let cursor = if self.grabbing || matches!(self.gesture, Some(Gesture::Pan { .. })) {
            CursorStyle::OpenHand
        } else if self.tool.begins_beside() {
            CursorStyle::Crosshair
        } else {
            CursorStyle::Arrow
        };
        let frozen = self.frozen;
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                for picture in retired {
                    picture.let_go(window);
                }
                let dense = window.scale_factor();
                // (On a whole pixel of the screen: what is shown at a pixel to a pixel is the image.)
                let origin = point(
                    px((bounds.origin.x.as_f32() * dense).round() / dense),
                    px((bounds.origin.y.as_f32() * dense).round() / dense),
                );
                let placed = Bounds {
                    origin,
                    size: bounds.size,
                };
                stage.set(Some(placed));
                if let (Some(picture), Some(view)) = (picture, view) {
                    let size = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
                    let look = view::look(size, view, looking, dense);
                    let painter = Painter {
                        origin,
                        offset: [look.part.x, look.part.y],
                        scale: look.scale,
                    };
                    let bases = picture.bases(&look, size, dense, window);
                    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                        paint::draw_all(window, &painter, view, &bases, &edits, None);
                        if let Some(shaping) = &shaping {
                            paint::draw_all(
                                window,
                                &painter,
                                view,
                                &[],
                                std::slice::from_ref(shaping),
                                None,
                            );
                        }
                    });
                }
                if !frozen {
                    window.set_cursor_style(cursor, &hitbox);
                }

                // Every mouse event of the window: while a button is held, the window goes on
                // getting them, also beside the place and outside the window.
                let down = this.clone();
                let over = hitbox.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && over.is_hovered(window) {
                        let _ = down.update(cx, |this, cx| this.down(event, window, cx));
                    }
                });
                let moved = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = moved.update(cx, |this, cx| this.moved(event, window, cx));
                    }
                });
                let up = this.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        let _ = up.update(cx, |this, cx| this.up(event, window, cx));
                    }
                });
                let wheel = this.clone();
                let over = hitbox;
                window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && over.is_hovered(window) {
                        let _ = wheel.update(cx, |this, cx| this.wheel(event, window, cx));
                    }
                });
            },
        )
        .size_full()
    }

    /// How closely it is looked at, and the button that shows all of it.
    fn zoom(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| self.language.text(key, &[]);
        let percent = self
            .look(window)
            .map(|(_, look)| look.percent(window.scale_factor()))
            .unwrap_or(100);
        let whole = self.looking.is_whole();
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_color(theme.muted)
            .child(
                div()
                    .id("zoom")
                    .tooltip(tip(t("editor.zoom")))
                    .child(format!("{percent}%")),
            )
            .child(
                div()
                    .id("whole")
                    .w(px(28.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius_small)
                    .when(whole, |button| button.opacity(0.4))
                    .when(!whole, |button| {
                        button
                            .hover(|style| style.bg(theme.surface_hover))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.looking = Looking::default();
                                cx.notify();
                            }))
                    })
                    .tooltip(tip(format!("{} (0)", t("editor.zoom.whole"))))
                    .child(glyph(WHOLE, 14.0)),
            )
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let tools = self.tools_bar(&theme, cx);
        let stage = self.stage(cx);
        let zoom = self.zoom(&theme, window, cx);
        let text: Hsla = theme.text;
        div()
            .key_context("Editor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(16.0))
            .bg(theme.background)
            .text_color(text)
            .font_family(theme.font.clone())
            .text_size(theme.text_size_small)
            .child(
                div()
                    .flex()
                    .items_start()
                    // (P6: the tour's i, at the left.)
                    .child(div().flex_1().min_w_0())
                    .child(tools)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(156.0))
                            .flex()
                            .justify_end()
                            .children(self.top_right.clone()),
                    ),
            )
            .child(div().flex_1().min_h_0().relative().child(stage))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(12.0))
                    .child(div().flex_1().min_w_0().children(self.below.clone()))
                    .child(zoom),
            )
    }
}
