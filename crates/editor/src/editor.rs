//! The editor: the capture, what is drawn on it, and the tools to draw with. It shows the picture as
//! closely as the user looks at it, takes presses and keys for the tool in hand, and keeps the
//! drawing (`perora_core::drawing`). What leaves (saving, copying) is the window's: the editor tells
//! it what was asked (`Event`), and the window makes the image with the same drawing (`paint`).
//!
//! P4 of docs/gpui-rewrite.md: the picture, looking closer and moving it, undo and redo, the pen,
//! the shapes and lines, selecting what is drawn and moving it (`selecting`), and the settings of
//! what is in hand or selected (`bars`: the second bar and the panels). The other tools come next.

mod bars;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

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
use crate::marks::{self, Hold, mark_of};
use crate::paint::{self, Painter};
use crate::picture::Picture;
use crate::selecting::{self, Pressed};
use crate::tools::{self, Kind, Press, Tool};
use crate::view::{self, Area, Looking};
use bars::{Openers, Panel, Set};

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
    /// These are moved, by as far as the pointer went from where it took hold (pixels of the image).
    Move {
        indices: Vec<usize>,
        from: Place,
        by: Place,
    },
    /// The one selected is pulled by a hold: as it was taken hold of, and as it is now.
    Resize {
        index: usize,
        hold: Hold,
        from: Place,
        was: Edit,
        now: Edit,
    },
    /// A part is marked out: what is wholly in it is selected, along with `with`.
    Marking {
        from: Place,
        to: Place,
        with: Vec<usize>,
    },
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
            Gesture::Pan { button, .. } => *button,
            _ => MouseButton::Left,
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
    figures: tools::figure::Figures,
    /// The last thing in the list was just drawn, with the tool still in hand: a shape is taken
    /// hold of there to be put right (owner: 「図形は描画直後は直前のやつを描画モードでも掴めるように」).
    just_drawn: bool,
    looking: Looking,
    /// Where the picture is shown, as last laid out (logical pixels of the window; its corner on a
    /// whole pixel of the screen).
    stage: Rc<Cell<Option<Bounds<Pixels>>>>,
    gesture: Option<Gesture>,
    /// Space is held: the pointer takes hold of the picture, not of a tool.
    grabbing: bool,
    /// What is selected (by its place in the list): one, several together, or nothing.
    selected: Vec<usize>,
    /// The pointer over a hold of the one selected: the way it is pulled there.
    pointing: Option<CursorStyle>,
    /// The last change was refused: the drawing holds no more (said until something is drawn).
    full: bool,
    /// While the capture leaves: nothing is drawn, no key does anything.
    frozen: bool,
    /// The second bar is put away: at first, and whenever a tool is taken otherwise than by its
    /// button, or something comes to be selected or no longer is (`follow`).
    shut: bool,
    /// The tool whose button was pressed, until it is in hand.
    by_button: Option<Kind>,
    /// The panel asked for (open while it still belongs to what is in hand: `panel`).
    asked: Option<Panel>,
    /// The tool and the one selected as the bars last followed them.
    was: (Kind, Option<usize>),
    /// How solid a shape's inside was the last time it was any: what it is again when it is given
    /// a colour.
    solid: f32,
    openers: Openers,
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
            figures: tools::figure::Figures::default(),
            just_drawn: false,
            looking: Looking::default(),
            stage: Rc::new(Cell::new(None)),
            gesture: None,
            grabbing: false,
            selected: Vec::new(),
            pointing: None,
            full: false,
            frozen: false,
            shut: true,
            by_button: None,
            asked: None,
            was: (Kind::Select, None),
            solid: 1.0,
            openers: Rc::new(RefCell::new(Default::default())),
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
        self.figures = tools::figure::Figures::default();
        self.just_drawn = false;
        self.looking = Looking::default();
        self.gesture = None;
        self.grabbing = false;
        self.selected.clear();
        self.pointing = None;
        self.full = false;
        self.frozen = false;
        self.shut = true;
        self.by_button = None;
        self.asked = None;
        self.was = (Kind::Select, None);
        self.solid = 1.0;
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
        if let Some(Gesture::Draw) = self.gesture.take()
            && let Some(tool) = self.tool_mut()
        {
            tool.give_up();
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
        // (To a tenth of a pixel, as the Tauri version had it: what is drawn is kept at that, and a
        // click that moves nothing changes nothing.)
        Some(Press {
            at: look.in_image(x, y).map(marks::tenth),
            scale: look.scale,
            unit: self.unit(),
            shift: modifiers.shift,
        })
    }

    fn tool_mut(&mut self) -> Option<&mut dyn Tool> {
        match self.tool {
            Kind::Select => None,
            Kind::Pen => Some(&mut self.pen),
            Kind::Rectangle | Kind::Ellipse | Kind::Line | Kind::Arrow => Some(&mut self.figures),
        }
    }

    fn tool_ref(&self) -> Option<&dyn Tool> {
        match self.tool {
            Kind::Select => None,
            Kind::Pen => Some(&self.pen),
            Kind::Rectangle | Kind::Ellipse | Kind::Line | Kind::Arrow => Some(&self.figures),
        }
    }

    /// Whether a press at `at` takes hold of the shape just drawn, its tool still in hand: on it as a
    /// press means it (inside a shape with no inside, the next one is drawn), and not at an end of a
    /// line (the next line is begun there, as often as not). Not a stroke of the pen: the next one
    /// is begun on the one before all the time.
    fn takes_last(&self, at: Place, scale: f32) -> bool {
        let (Some(last), Some(view)) = (self.drawing.edits().last(), self.view()) else {
            return false;
        };
        let near = selecting::NEAR / scale;
        let ends = match last {
            Edit::Line { from, to, .. } | Edit::Arrow { from, to, .. } => vec![*from, *to],
            _ => Vec::new(),
        };
        self.selected.is_empty()
            && self.just_drawn
            && kind_of(last) == Some(self.tool)
            && self.tool.is_figure()
            && selecting::seen_at(last, at, near, &view)
            && !ends
                .iter()
                .any(|end| (at[0] - end[0]).hypot(at[1] - end[1]) <= near * 2.0)
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
        // (The picture is pressed on: the bar of the tool, there since its button was pressed, goes.)
        self.shut = true;
        let Some(press) = self.press(event.position, event.modifiers, window) else {
            return;
        };
        if self.tool == Kind::Select {
            self.select(&press, event.modifiers.control, cx);
            return;
        }
        let begins_beside = self.tool.begins_beside();
        let on_picture = self.view().is_some_and(|view| view.contains(press.at));
        if !on_picture && !begins_beside {
            return;
        }
        if self.takes_last(press.at, press.scale) {
            let last = self.drawing.edits().len() - 1;
            self.gesture = Some(Gesture::Move {
                indices: vec![last],
                from: press.at,
                by: [0.0, 0.0],
            });
            cx.notify();
            return;
        }
        self.selected.clear();
        if let Some(tool) = self.tool_mut()
            && tool.begin(&press)
        {
            self.gesture = Some(Gesture::Draw);
            cx.notify();
        }
    }

    /// A press with the select tool: a click is a thing, a drag from nothing a part; on what is
    /// selected, or a hold of it, it is taken hold of.
    fn select(&mut self, press: &Press, ctrl: bool, cx: &mut Context<Self>) {
        let Some(view) = self.view() else {
            return;
        };
        let edits = self.drawing.edits();
        let at = press.at;
        match selecting::press(edits, &self.selected, at, press.scale, &view, ctrl) {
            Pressed::Selection(those) => self.selected = those,
            Pressed::Marking { with } => {
                if !ctrl {
                    self.selected.clear();
                }
                self.gesture = Some(Gesture::Marking {
                    from: at,
                    to: at,
                    with,
                });
            }
            Pressed::Moving(indices) => {
                self.selected = indices.clone();
                self.gesture = Some(Gesture::Move {
                    indices,
                    from: at,
                    by: [0.0, 0.0],
                });
            }
            Pressed::Resizing(index, hold) => {
                let was = edits[index].clone();
                self.gesture = Some(Gesture::Resize {
                    index,
                    hold,
                    from: at,
                    now: was.clone(),
                    was,
                });
            }
        }
        cx.notify();
    }

    /// Over a hold of the one selected, the pointer says which way it is pulled there; over the
    /// shape just drawn, that it is taken hold of there (the arrow: GPUI has no pointer of moving on
    /// Windows, owner: 「カーソルの移動はまあそのままでもいいんじゃね？」).
    fn hover(&mut self, at: Place, scale: f32, cx: &mut Context<Self>) {
        let pointing = match (self.tool, self.selected.as_slice()) {
            (Kind::Select, [index]) => self
                .drawing
                .edits()
                .get(*index)
                .and_then(|edit| selecting::hold_at(edit, at, scale))
                .map(|(hold, _)| pointer_of(hold)),
            _ if self.takes_last(at, scale) => Some(CursorStyle::Arrow),
            _ => None,
        };
        if pointing != self.pointing {
            self.pointing = pointing;
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
        if let Some(Gesture::Pan { from, was, button }) = &self.gesture {
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
            return;
        }
        let Some(press) = self.press(event.position, event.modifiers, window) else {
            return;
        };
        let unit = self.unit();
        match &mut self.gesture {
            None => self.hover(press.at, press.scale, cx),
            Some(Gesture::Pan { .. }) => {}
            Some(Gesture::Draw) => {
                if let Some(tool) = self.tool_mut() {
                    tool.go_on(&press);
                    cx.notify();
                }
            }
            Some(Gesture::Move { from, by, .. }) => {
                *by = [press.at[0] - from[0], press.at[1] - from[1]];
                cx.notify();
            }
            Some(Gesture::Resize {
                hold,
                from,
                was,
                now,
                ..
            }) => {
                if let Some(mark) = mark_of(was) {
                    let by = [press.at[0] - from[0], press.at[1] - from[1]];
                    *now = mark.resized(was, *hold, by, press.shift, unit);
                }
                cx.notify();
            }
            Some(Gesture::Marking { to, .. }) => {
                *to = press.at;
                cx.notify();
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
            Some(Gesture::Move { indices, by, .. }) => {
                // (Not a move that leaves them where they were.)
                let edits = moved_in(self.drawing.edits(), &indices, by);
                if edits != self.drawing.edits() {
                    self.change(edits, cx);
                }
                cx.notify();
            }
            Some(Gesture::Resize {
                index, was, now, ..
            }) => {
                // (Only what it was when it was taken hold of is made larger.)
                if self.drawing.edits().get(index) == Some(&was) && now != was {
                    let mut edits = self.drawing.edits().to_vec();
                    edits[index] = now;
                    self.change(edits, cx);
                }
                cx.notify();
            }
            Some(Gesture::Marking { from, to, with }) => {
                // What is wholly in the part marked out, and in sight, is selected (along with what
                // was, with Ctrl). A part too small to have been meant was a press.
                if let (Some((_, look)), Some(view)) = (self.look(window), self.view())
                    && (to[0] - from[0]).hypot(to[1] - from[1]) * look.scale
                        >= selecting::LEAST_MARKED
                {
                    let part = look.part;
                    let in_sight = |area: &Area| {
                        area.x < part.right().min(view.right())
                            && area.right() > part.x.max(view.x)
                            && area.y < part.bottom().min(view.bottom())
                            && area.bottom() > part.y.max(view.y)
                    };
                    let those = selecting::marked(self.drawing.edits(), from, to, &with, in_sight);
                    if !those.is_empty() {
                        self.selected = those;
                    }
                }
                cx.notify();
            }
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
                    && marks::area_of(&edit).meets(&view)
                {
                    let mut edits = self.drawing.edits().to_vec();
                    edits.push(edit);
                    self.change(edits, cx);
                    // (Not when the drawing took no more: the one before is still the last.)
                    if !self.full {
                        self.just_drawn = true;
                    }
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

    /// The drawing, changed by a setting being dragged (a slider): one step with the changes of the
    /// same setting of the same thing just before it.
    fn change_dragged(&mut self, edits: Vec<Edit>, what: &str, cx: &mut Context<Self>) {
        match self.drawing.change_dragged(edits, what, Instant::now()) {
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

    /// What the settings are about: what is selected (one alone); or, with nothing selected, what was
    /// just drawn while the tool that drew it is still in hand (set right at a look, without being
    /// selected first). Otherwise they are only for what is drawn next.
    fn about(&self) -> Option<usize> {
        if !self.selected.is_empty() {
            return self.chosen();
        }
        let last = self.drawing.edits().len().checked_sub(1)?;
        (self.just_drawn && kind_of(&self.drawing.edits()[last]) == Some(self.tool)).then_some(last)
    }

    /// The one at `index` set so: one step to undo (with the steps of the same setting just before
    /// it, when it is dragged). Set to what it is already, nothing to undo.
    fn alter(&mut self, index: usize, next: Edit, set: Set, dragged: bool, cx: &mut Context<Self>) {
        let mut edits = self.drawing.edits().to_vec();
        match edits.get_mut(index) {
            Some(edit) if *edit != next => *edit = next,
            _ => return,
        }
        if dragged {
            self.change_dragged(edits, &format!("{index}:{}", set.name()), cx);
        } else {
            self.change(edits, cx);
        }
    }

    fn drawn(&mut self, cx: &mut Context<Self>) {
        self.shown = Rc::new(self.drawing.edits().to_vec());
        cx.emit(Event::Changed);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_none() && self.drawing.undo() {
            self.full = false;
            self.just_drawn = false;
            self.selected.clear();
            self.pointing = None;
            self.drawn(cx);
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_none() && self.drawing.redo() {
            self.just_drawn = false;
            self.selected.clear();
            self.pointing = None;
            self.drawn(cx);
        }
    }

    /// What is selected is taken away.
    fn remove(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_some() || self.selected.is_empty() {
            return;
        }
        let edits = self
            .drawing
            .edits()
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.selected.contains(index))
            .map(|(_, edit)| edit.clone())
            .collect();
        self.selected.clear();
        self.pointing = None;
        self.change(edits, cx);
    }

    /// Takes up a tool by its key: the tool and nothing else (no bar comes down, no panel opens).
    /// The one in hand again does nothing, unless something is selected.
    fn take(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if self.gesture.is_some() || (kind == self.tool && self.selected.is_empty()) {
            return;
        }
        self.by_button = None;
        self.hold(kind, cx);
    }

    /// A tool's button is pressed: the tool is taken, with its second bar. The one in hand pressed
    /// again: its bar goes, or comes back (and the pen's panel opens or goes); with something
    /// selected, the select button does that for the bar of what is selected.
    fn pick(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if self.gesture.is_some() {
            return;
        }
        if kind == self.tool && kind == Kind::Pen && self.chosen().is_none() {
            self.asked = if self.panel() == Some(Panel::Pen) {
                None
            } else {
                Some(Panel::Pen)
            };
            cx.notify();
            return;
        }
        if kind == self.tool && self.offered().is_some() {
            self.asked = None;
            self.shut = !self.shut;
            cx.notify();
            return;
        }
        if kind != self.tool {
            self.by_button = Some(kind);
        }
        self.hold(kind, cx);
    }

    /// The tool is in hand. Another than the select tool lets go of what is selected.
    fn hold(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.tool = kind;
        self.pointing = None;
        self.just_drawn = false;
        if kind.is_figure() {
            self.figures.figure = kind;
        }
        if kind != Kind::Select {
            self.selected.clear();
        }
        self.asked = None;
        cx.notify();
    }

    /// Esc lets go of one thing at a time: what the pointer is doing, the panel that is open, what
    /// is selected, then the tool. With nothing drawn, it throws the capture away; with something
    /// drawn, it does not (that is the bin's).
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.gesture.is_some() {
            self.give_up_gesture();
            cx.notify();
            return;
        }
        if self.panel().is_some() {
            self.asked = None;
            cx.notify();
            return;
        }
        if !self.selected.is_empty() {
            self.selected.clear();
            self.pointing = None;
            cx.notify();
            return;
        }
        if self.tool != Kind::Select {
            self.by_button = None;
            self.hold(Kind::Select, cx);
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
            "delete" | "backspace" => self.remove(cx),
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

impl Editor {
    /// While a hold is pulled, the pointer stays the one it was taken hold of with.
    fn held_pointer(&self) -> Option<CursorStyle> {
        match &self.gesture {
            Some(Gesture::Resize { hold, .. }) => Some(pointer_of(*hold)),
            _ => None,
        }
    }
}

/// The tool that draws an edit of this kind.
fn kind_of(edit: &Edit) -> Option<Kind> {
    match edit {
        Edit::Pen { .. } => Some(Kind::Pen),
        Edit::Rectangle { .. } => Some(Kind::Rectangle),
        Edit::Ellipse { .. } => Some(Kind::Ellipse),
        Edit::Line { .. } => Some(Kind::Line),
        Edit::Arrow { .. } => Some(Kind::Arrow),
        _ => None,
    }
}

/// The list with those at `indices` moved by `by`.
fn moved_in(edits: &[Edit], indices: &[usize], by: Place) -> Vec<Edit> {
    edits
        .iter()
        .enumerate()
        .map(|(index, edit)| match mark_of(edit) {
            Some(mark) if indices.contains(&index) => mark.moved(edit, by),
            _ => edit.clone(),
        })
        .collect()
}

/// The pointer over a hold: the arrows of the way it is pulled. (An end of a line goes any way: the
/// Tauri version showed the four arrows of moving, which GPUI does not have on Windows.)
fn pointer_of(hold: Hold) -> CursorStyle {
    match hold {
        Hold::Side { x, y } if x != 0 && y != 0 => {
            if x == y {
                CursorStyle::ResizeUpLeftDownRight
            } else {
                CursorStyle::ResizeUpRightDownLeft
            }
        }
        Hold::Side { x, .. } if x != 0 => CursorStyle::ResizeLeftRight,
        Hold::Side { .. } => CursorStyle::ResizeUpDown,
        Hold::From | Hold::To => CursorStyle::Arrow,
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
            bars::button(id, current, enabled, theme)
        };
        let mut bar = div().flex().items_center().gap(px(4.0));
        for kind in [Kind::Select, Kind::Pen] {
            let pen = kind == Kind::Pen;
            let colour = pen.then(|| paint::colour(self.pen.colour));
            let named = format!("{} ({})", t(kind.name()), kind.key().to_uppercase());
            bar = bar.child(
                button(kind.name().into(), self.tool == kind, true)
                    .tooltip(tip(named))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(kind, cx)))
                    .child(icons::icon(
                        icons::of_tool(kind, colour, None),
                        16.0,
                        theme.text,
                    ))
                    // In hand, the pen says it has a panel of its own: pressed again, it opens.
                    .when(pen && self.tool == kind, |button| {
                        button.child(
                            div()
                                .absolute()
                                .bottom(px(2.0))
                                .left_0()
                                .right_0()
                                .flex()
                                .justify_center()
                                .child(icons::caret(theme.text)),
                        )
                    })
                    .when(pen, |button| button.child(self.pen_opener())),
            );
        }
        // The shapes and lines: one button, which takes up the last of them taken; drawn in their
        // outline colour, with their inside.
        let figures = &self.figures;
        let inside =
            (figures.fill > 0.0).then(|| (paint::colour(figures.fill_colour), figures.fill));
        let last = figures.figure;
        bar = bar.child(
            button("figures".into(), self.tool.is_figure(), true)
                .tooltip(tip(t("editor.tool.shapes")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let kind = if this.tool.is_figure() {
                        this.tool
                    } else {
                        last
                    };
                    this.pick(kind, cx)
                }))
                .child(icons::icon(
                    icons::shapes(paint::colour(figures.colour), inside),
                    16.0,
                    theme.text,
                )),
        );
        let typing = false;
        let (can_undo, can_redo) = (
            self.drawing.can_undo() && !typing,
            self.drawing.can_redo() && !typing,
        );
        let can_delete = !self.selected.is_empty() && !typing;
        bar.child(bars::rule(theme))
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
                button("delete".into(), false, can_delete)
                    .tooltip(tip(format!("{} (Delete)", t("editor.delete"))))
                    .when(can_delete, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| this.remove(cx)))
                    })
                    .child(glyph(DELETE, 16.0)),
            )
    }

    fn stage(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        let stage = self.stage.clone();
        let picture = self.capture.as_ref().map(|capture| capture.picture.clone());
        let view = self.view();
        let looking = self.looking;
        let edits = match &self.gesture {
            Some(Gesture::Move { indices, by, .. }) => {
                Rc::new(moved_in(self.drawing.edits(), indices, *by))
            }
            Some(Gesture::Resize { index, now, .. }) => {
                let mut edits = self.drawing.edits().to_vec();
                if let Some(edit) = edits.get_mut(*index) {
                    *edit = now.clone();
                }
                Rc::new(edits)
            }
            _ => self.shown.clone(),
        };
        let selected = self.selected.clone();
        let marking = match &self.gesture {
            Some(Gesture::Marking { from, to, .. }) => Some((*from, *to)),
            _ => None,
        };
        let shaping = self.tool_ref().and_then(|tool| tool.shaping());
        let retired = std::mem::take(&mut self.retired);
        let cursor = if self.grabbing || matches!(self.gesture, Some(Gesture::Pan { .. })) {
            CursorStyle::OpenHand
        } else if let Some(pointing) = self.held_pointer().or(self.pointing) {
            pointing
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
                        // What is selected, framed (not cut to the picture: it may reach beside
                        // it). One alone has its holds; several together only their frames.
                        for index in &selected {
                            let Some(edit) = edits.get(*index) else {
                                continue;
                            };
                            let margin = marks::frame_around(edit, look.scale);
                            paint::frame(window, &painter, marks::box_of(edit), margin);
                            if selected.len() == 1
                                && let Some(mark) = mark_of(edit)
                            {
                                for (hold, at) in mark.holds(edit, margin) {
                                    paint::grip(
                                        window,
                                        &painter,
                                        at,
                                        !matches!(hold, Hold::Side { .. }),
                                    );
                                }
                            }
                        }
                        if let Some((from, to)) = marking {
                            paint::marked(window, &painter, from, to);
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
        self.follow();
        let theme = Theme::get(cx).clone();
        let tools = self.tools_bar(&theme, cx);
        let second = self.second_bar(&theme, cx);
        let panel = self.panel_view(&theme, cx);
        let stage = self.stage(cx);
        let zoom = self.zoom(&theme, window, cx);
        let text: Hsla = theme.text;
        div()
            .key_context("Editor")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .relative()
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
            .children(second)
            .children(panel)
    }
}
