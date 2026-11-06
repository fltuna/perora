//! The second bar and the panels: the settings of what is in hand or selected (its colour, its size,
//! a shape's inside), where the Tauri version had them (`.bar.second`, `Panel`, `Colours`, `Size`).
//!
//! The second bar comes down over the top of the picture when a tool that has one is taken by its
//! button, or when the select button is pressed again with something selected; it goes when the
//! picture is pressed. Nothing comes down that was not asked for by a press (owner: 「ユーザーが
//! 明示的にクリックしない限りは閉じたら開かないようにしたいどのツール類も。」). A panel opens under
//! the button that asks for it, and goes at a press anywhere else, at Esc, when the tool or what is
//! selected changes, and when one of the colours offered is chosen (owner: 「クリックして色選択したら
//! 閉じるといいかも」).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnchoredPositionMode, AnyElement, Bounds, BoxShadow, Context, Div, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseDownEvent, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement, Styled, anchored, canvas, deferred, div, point, px, rgba,
};
use perora_core::colours::{NONE_TAKES, PALETTE};
use perora_core::edit::{Colour, Edit};
use perora_ui::swatch::swatch;
use perora_ui::{Slider, Theme};

use super::{Editor, tip};
use crate::icons;
use crate::paint;
use crate::tools::Kind;

/// The panels that open under a button: of the pen (under its button on the first bar), of the
/// stroke of the pen selected, of a shape's inside, and of the outline of shapes and lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Panel {
    Pen,
    Stroke,
    Fill,
    Outline,
}

/// Which second bar there is to have: of the shapes and lines in hand, or of what is selected (a
/// shape, a line or an arrow, a stroke of the pen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Offered {
    Figures,
    Shape,
    Line,
    Stroke,
}

/// A setting, as a panel changes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Set {
    Colour(Colour),
    /// A width, in steps.
    Step(u32),
    /// The colour of a shape's inside; none: no inside.
    Inside(Option<Colour>),
    /// How solid a shape's inside is (more than 0).
    Solid(f32),
}

impl Set {
    /// The name changes of it are gathered under into one step to undo, while it is dragged.
    pub(super) fn name(self) -> &'static str {
        match self {
            Set::Colour(_) => "color",
            Set::Step(_) => "width",
            Set::Inside(_) => "fillColor",
            Set::Solid(_) => "fill",
        }
    }
}

/// Where the buttons that open panels were last laid out (in the window): the panel is put under
/// its button, and a press on the button is told from a press away from the panel.
pub(super) type Openers = Rc<RefCell<HashMap<Panel, Bounds<Pixels>>>>;

/// What a colour chosen in a panel does (none: no colour).
type Pick = Rc<dyn Fn(&mut Editor, Option<Colour>, &mut Context<Editor>)>;
/// What a size set in a panel does.
type SetStep = Rc<dyn Fn(&mut Editor, u32, &mut Context<Editor>)>;

/// The steps of a width.
pub(super) const STEPS: (u32, u32) = (1, 20);

/// How wide a panel is.
const PANEL: f32 = 322.0;

/// What a size is shown by: a stroke of the pen, or a line (`Size` in editor.tsx, in a box 240 by
/// 64).
const PEN_SAMPLE: &str = "M24 40C64 4 100 12 124 34S184 60 216 26";
const LINE_SAMPLE: &str = "M24 32H216";

/// A button of the bars, 36 by 34: framed in the accent while it is the tool in hand.
pub(super) fn button(
    id: impl Into<SharedString>,
    current: bool,
    enabled: bool,
    theme: &Theme,
) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .relative()
        .flex_none()
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
}

/// What sets one group of a bar apart from the next.
pub(super) fn rule(theme: &Theme) -> Div {
    div()
        .flex_none()
        .w(px(1.0))
        .h(px(20.0))
        .mx(px(8.0))
        .bg(theme.border)
}

/// Records where an element was laid out, as the opener of `panel`.
fn recorded(openers: &Openers, panel: Panel) -> impl IntoElement {
    let openers = openers.clone();
    canvas(
        move |bounds, _, _| {
            openers.borrow_mut().insert(panel, bounds);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

fn shadow(down: f32, blur: f32, dark: u32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: rgba(dark).into(),
        offset: point(px(0.0), px(down)),
        blur_radius: px(blur),
        spread_radius: px(0.0),
        inset: false,
    }]
}

impl Editor {
    /// The one selected, when one alone is.
    pub(super) fn chosen(&self) -> Option<usize> {
        match self.selected.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }

    fn chosen_edit(&self) -> Option<&Edit> {
        self.chosen()
            .and_then(|index| self.drawing.edits().get(index))
    }

    /// The second bar there is to have, open or not.
    pub(super) fn offered(&self) -> Option<Offered> {
        match self.chosen_edit() {
            Some(Edit::Pen { .. }) => Some(Offered::Stroke),
            Some(Edit::Rectangle { .. } | Edit::Ellipse { .. }) => Some(Offered::Shape),
            Some(Edit::Line { .. } | Edit::Arrow { .. }) => Some(Offered::Line),
            Some(_) => None,
            None if self.tool.is_figure() => Some(Offered::Figures),
            None => None,
        }
    }

    /// The second bar that is there.
    pub(super) fn second(&self) -> Option<Offered> {
        if self.shut { None } else { self.offered() }
    }

    /// Whether a shape's inside can be set: of a shape (not of a line).
    fn fills(&self) -> bool {
        match self.second() {
            Some(Offered::Shape) => true,
            Some(Offered::Figures) => matches!(self.tool, Kind::Rectangle | Kind::Ellipse),
            _ => false,
        }
    }

    /// The panel that is open: the one asked for, while it still belongs to what is in hand.
    pub(super) fn panel(&self) -> Option<Panel> {
        let second = self.second();
        let open = match self.asked? {
            Panel::Pen => self.tool == Kind::Pen && self.chosen().is_none(),
            Panel::Stroke => second == Some(Offered::Stroke),
            Panel::Fill => self.fills(),
            Panel::Outline => matches!(
                second,
                Some(Offered::Figures | Offered::Shape | Offered::Line)
            ),
        };
        open.then_some(self.asked?)
    }

    /// As the Tauri version's effects had it: when the tool in hand changes, or something comes to
    /// be selected or no longer is, the second bar is there only for a tool just taken by its
    /// button; and the panel goes when the tool or the one selected changes.
    pub(super) fn follow(&mut self) {
        let chosen = self.chosen();
        let (tool, was) = self.was;
        if self.tool != tool || chosen.is_some() != was.is_some() {
            self.shut = self.by_button != Some(self.tool);
            self.by_button = None;
        }
        if self.tool != tool || chosen != was {
            self.asked = None;
        }
        self.was = (self.tool, chosen);
    }

    /// The colour and size of the pen's strokes: what it draws next, or the one selected.
    fn stroke_now(&self) -> (Colour, u32) {
        match self.chosen_edit() {
            Some(Edit::Pen { color, width, .. }) => (*color, self.step_of(*width)),
            _ => (self.pen.colour, self.pen.step),
        }
    }

    /// The outline of shapes and lines: what is drawn next, or the one selected.
    fn outline_now(&self) -> (Colour, u32) {
        match self.chosen_edit() {
            Some(
                Edit::Rectangle { color, width, .. }
                | Edit::Ellipse { color, width, .. }
                | Edit::Line { color, width, .. }
                | Edit::Arrow { color, width, .. },
            ) => (*color, self.step_of(*width)),
            _ => (self.figures.colour, self.figures.step),
        }
    }

    /// A shape's inside (its colour, and how solid it is: 0, none): of what is drawn next, or of the
    /// one selected.
    pub(super) fn inside_now(&self) -> (Colour, f32) {
        match self.chosen_edit() {
            Some(
                Edit::Rectangle {
                    color,
                    fill,
                    fill_color,
                    ..
                }
                | Edit::Ellipse {
                    color,
                    fill,
                    fill_color,
                    ..
                },
            ) => (fill_color.unwrap_or(*color), *fill),
            _ => (self.figures.fill_colour, self.figures.fill),
        }
    }

    fn step_of(&self, width: f32) -> u32 {
        (width / self.unit()).round().max(1.0) as u32
    }

    /// A setting of the pen: for what it draws next, and for what the settings are about.
    pub(super) fn set_stroke(&mut self, set: Set, dragged: bool, cx: &mut Context<Self>) {
        match set {
            Set::Colour(colour) => self.pen.colour = colour,
            Set::Step(step) => self.pen.step = step,
            _ => return,
        }
        let unit = self.unit();
        let Some(index) = self.about() else {
            cx.notify();
            return;
        };
        if let Some(Edit::Pen {
            points,
            color,
            width,
        }) = self.drawing.edits().get(index).cloned()
        {
            let (color, width) = match set {
                Set::Colour(colour) => (colour, width),
                Set::Step(step) => (color, step as f32 * unit),
                _ => (color, width),
            };
            let next = Edit::Pen {
                points,
                color,
                width,
            };
            self.alter(index, next, set, dragged, cx);
        }
        cx.notify();
    }

    /// A setting of the outline of shapes and lines.
    pub(super) fn set_outline(&mut self, set: Set, dragged: bool, cx: &mut Context<Self>) {
        match set {
            Set::Colour(colour) => self.figures.colour = colour,
            Set::Step(step) => self.figures.step = step,
            _ => return,
        }
        let unit = self.unit();
        let Some(index) = self.about() else {
            cx.notify();
            return;
        };
        let Some(mut next) = self.drawing.edits().get(index).cloned() else {
            return;
        };
        match &mut next {
            Edit::Rectangle { color, width, .. }
            | Edit::Ellipse { color, width, .. }
            | Edit::Line { color, width, .. }
            | Edit::Arrow { color, width, .. } => match set {
                Set::Colour(colour) => *color = colour,
                Set::Step(step) => *width = step as f32 * unit,
                _ => {}
            },
            _ => return,
        }
        self.alter(index, next, set, dragged, cx);
        cx.notify();
    }

    /// A setting of a shape's inside. An inside given a colour where it had none is as solid as an
    /// inside last was.
    pub(super) fn set_inside(&mut self, set: Set, dragged: bool, cx: &mut Context<Self>) {
        let (colour, fill) = self.inside_now();
        let (colour, fill) = match set {
            Set::Inside(Some(to)) => (to, if fill == 0.0 { self.solid } else { fill }),
            Set::Inside(None) => (colour, 0.0),
            Set::Solid(solid) => (colour, solid),
            _ => return,
        };
        if fill > 0.0 {
            self.solid = fill;
        }
        // What is drawn next has the inside this one has now, all of it.
        self.figures.fill_colour = colour;
        self.figures.fill = fill;
        let Some(index) = self.about() else {
            cx.notify();
            return;
        };
        let Some(mut next) = self.drawing.edits().get(index).cloned() else {
            return;
        };
        match &mut next {
            Edit::Rectangle {
                fill: was,
                fill_color,
                ..
            }
            | Edit::Ellipse {
                fill: was,
                fill_color,
                ..
            } => {
                *was = fill;
                *fill_color = Some(colour);
            }
            _ => return,
        }
        self.alter(index, next, set, dragged, cx);
        cx.notify();
    }

    /// A press on a button that opens a panel: it opens, or goes when it is open.
    fn open(&mut self, panel: Panel, cx: &mut Context<Self>) {
        self.asked = if self.panel() == Some(panel) {
            None
        } else {
            Some(panel)
        };
        cx.notify();
    }

    /// The second bar, when there is one.
    pub(super) fn second_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let second = self.second()?;
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let mut bar = div()
            .id("second")
            .occlude()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .py(px(5.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow(shadow(4.0, 14.0, 0x0000_004d));
        if second == Offered::Figures {
            let (outline, _) = self.outline_now();
            let (fill_colour, fill) = self.inside_now();
            let inside = (fill > 0.0).then(|| (paint::colour(fill_colour), fill));
            for kind in [Kind::Rectangle, Kind::Ellipse, Kind::Line, Kind::Arrow] {
                let named = format!("{} ({})", t(kind.name()), kind.key().to_uppercase());
                bar = bar.child(
                    button(
                        format!("second-{}", kind.name()),
                        self.tool == kind,
                        true,
                        theme,
                    )
                    .tooltip(tip(named))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(kind, cx)))
                    .child(icons::icon(
                        icons::of_tool(kind, Some(paint::colour(outline)), inside),
                        16.0,
                        theme.text,
                    )),
                );
            }
            bar = bar.child(rule(theme));
        }
        match second {
            Offered::Stroke => {
                let (colour, _) = self.stroke_now();
                bar = bar.child(self.opener(
                    Panel::Stroke,
                    t("editor.stroke"),
                    Some(colour),
                    true,
                    theme,
                    cx,
                ));
            }
            Offered::Figures | Offered::Shape | Offered::Line => {
                let fills = self.fills();
                let (fill_colour, fill) = self.inside_now();
                let shown = (fills && fill > 0.0).then_some(fill_colour);
                let (outline, _) = self.outline_now();
                bar = bar
                    .child(self.opener(Panel::Fill, t("editor.fill"), shown, fills, theme, cx))
                    .child(self.opener(
                        Panel::Outline,
                        t("editor.outline"),
                        Some(outline),
                        true,
                        theme,
                        cx,
                    ));
            }
        }
        // Over the top of the picture, under the first bar; no room is made for it (owner: 「押した
        // ときに下にpadding作られるの消してほしいー」).
        Some(
            deferred(
                div()
                    .absolute()
                    .top(px(16.0 + 40.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(bar),
            )
            .into_any_element(),
        )
    }

    /// A button of the second bar that opens a panel: the colour that is set (none: no colour),
    /// what it is the colour of, and the mark that there is more.
    fn opener(
        &self,
        panel: Panel,
        label: SharedString,
        colour: Option<Colour>,
        enabled: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let open = self.panel() == Some(panel);
        div()
            .id(SharedString::from(format!("opener-{panel:?}")))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(px(34.0))
            .pl(px(8.0))
            .pr(px(10.0))
            .rounded(theme.radius_small)
            .border_1()
            .border_color(gpui::transparent_black())
            .when(open, |opener| opener.bg(theme.surface_hover))
            .when(enabled && !open, |opener| {
                opener.hover(|style| style.bg(theme.surface_hover))
            })
            .when(!enabled, |opener| opener.opacity(0.4))
            .when(enabled, |opener| {
                opener.on_click(cx.listener(move |this, _, _, cx| this.open(panel, cx)))
            })
            .child(swatch(colour.map(paint::colour), 20.0, false, theme))
            .child(label)
            .child(icons::caret(theme.text))
            .child(recorded(&self.openers, panel))
    }

    /// The pen's tool button records where it is: its panel opens under it.
    pub(super) fn pen_opener(&self) -> impl IntoElement {
        recorded(&self.openers, Panel::Pen)
    }

    /// The panel that is open, under its button.
    pub(super) fn panel_view(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.panel()?;
        let under = *self.openers.borrow().get(&panel)?;
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let heading = |text: SharedString| div().font_weight(FontWeight::SEMIBOLD).child(text);
        let mut settings: Vec<AnyElement> = Vec::new();
        match panel {
            Panel::Pen | Panel::Stroke => {
                let (colour, step) = self.stroke_now();
                settings.push(
                    self.colours(
                        PALETTE.iter().copied().map(Some).collect(),
                        Some(colour),
                        Rc::new(|this: &mut Editor, colour, cx| {
                            if let Some(colour) = colour {
                                this.set_stroke(Set::Colour(colour), false, cx);
                            }
                        }),
                        theme,
                        cx,
                    )
                    .into_any_element(),
                );
                settings.push(
                    self.size(
                        PEN_SAMPLE,
                        colour,
                        step,
                        theme,
                        cx,
                        Rc::new(|this, step, cx| this.set_stroke(Set::Step(step), true, cx)),
                    )
                    .into_any_element(),
                );
            }
            Panel::Fill => {
                let (colour, fill) = self.inside_now();
                // "None" takes the place of the darkest grey: the rows stay six wide.
                let offered = std::iter::once(None)
                    .chain(
                        PALETTE
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| *index != NONE_TAKES)
                            .map(|(_, colour)| Some(*colour)),
                    )
                    .collect();
                settings.push(
                    self.colours(
                        offered,
                        (fill > 0.0).then_some(colour),
                        Rc::new(|this: &mut Editor, colour, cx| {
                            this.set_inside(Set::Inside(colour), false, cx)
                        }),
                        theme,
                        cx,
                    )
                    .into_any_element(),
                );
                let some = fill > 0.0;
                let shown = ((if some { fill } else { self.solid }) * 100.0).round();
                let this = cx.entity().downgrade();
                settings.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .when(!some, |setting| setting.opacity(0.4))
                        .child(heading(t("editor.opacity")))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Slider::new("opacity", shown, 5.0, 100.0)
                                            .step(5.0)
                                            .enabled(some)
                                            .on_change(move |value, _, cx| {
                                                let _ = this.update(cx, |this, cx| {
                                                    this.set_inside(
                                                        Set::Solid(value / 100.0),
                                                        true,
                                                        cx,
                                                    )
                                                });
                                            }),
                                    ),
                                )
                                .child(
                                    div()
                                        .min_w(px(34.0))
                                        .flex()
                                        .justify_end()
                                        .child(format!("{}%", (fill * 100.0).round())),
                                ),
                        )
                        .into_any_element(),
                );
            }
            Panel::Outline => {
                let (colour, step) = self.outline_now();
                settings.push(
                    self.colours(
                        PALETTE.iter().copied().map(Some).collect(),
                        Some(colour),
                        Rc::new(|this: &mut Editor, colour, cx| {
                            if let Some(colour) = colour {
                                this.set_outline(Set::Colour(colour), false, cx);
                            }
                        }),
                        theme,
                        cx,
                    )
                    .into_any_element(),
                );
                settings.push(
                    self.size(
                        LINE_SAMPLE,
                        colour,
                        step,
                        theme,
                        cx,
                        Rc::new(|this, step, cx| this.set_outline(Set::Step(step), true, cx)),
                    )
                    .into_any_element(),
                );
            }
        }
        let openers = self.openers.clone();
        let content = div()
            .id("panel")
            .occlude()
            .w(px(PANEL))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .pt(px(14.0))
            .px(px(16.0))
            .pb(px(16.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow(shadow(8.0, 24.0, 0x0000_0059))
            .cursor_default()
            // A press anywhere but in it and on what opened it puts it away.
            .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let on_opener = openers
                    .borrow()
                    .get(&panel)
                    .is_some_and(|bounds| bounds.contains(&event.position));
                if !on_opener && this.asked == Some(panel) {
                    this.asked = None;
                    cx.notify();
                }
            }))
            .children(settings);
        let at = point(under.center().x - px(PANEL / 2.0), under.bottom() + px(8.0));
        Some(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .position(at)
                    .snap_to_window_with_margin(px(8.0))
                    .child(content),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The colours to choose from, six to a row (none: no colour). Choosing one is the choice the
    /// panel was opened for: it goes.
    fn colours(
        &self,
        offered: Vec<Option<Colour>>,
        current: Option<Colour>,
        pick: Pick,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let none = t("editor.none");
        let rows = offered.chunks(6).enumerate().map(|(row, colours)| {
            div()
                .flex()
                .justify_between()
                .children(colours.iter().enumerate().map(|(at, colour)| {
                    let colour = *colour;
                    let pick = pick.clone();
                    let named: SharedString = match colour {
                        Some(colour) => colour.written().into(),
                        None => none.clone(),
                    };
                    div()
                        .id(("colour", row * 6 + at))
                        .cursor_pointer()
                        .rounded_full()
                        .tooltip(tip(named))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            pick(this, colour, cx);
                            this.asked = None;
                            cx.notify();
                        }))
                        .child(swatch(
                            colour.map(paint::colour),
                            36.0,
                            colour == current,
                            theme,
                        ))
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("editor.colours")),
            )
            .child(
                // (The ring of the colour in use reaches a little past it.)
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .p(px(3.0))
                    .children(rows),
            )
    }

    /// How wide the pen or a line is: a stroke as wide as it would be drawn, and a slider.
    fn size(
        &self,
        sample: &'static str,
        colour: Colour,
        step: u32,
        theme: &Theme,
        cx: &mut Context<Self>,
        set: SetStep,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        // On the screen, a step is about a pixel.
        let wide = (step as f32 * 1.2 + 1.0).min(40.0);
        let this = cx.entity().downgrade();
        let shown = step.clamp(STEPS.0, STEPS.1);
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("editor.size")),
            )
            .child(
                icons::drawing(
                    vec![icons::sample(
                        sample,
                        wide,
                        paint::colour(colour),
                        1.0,
                        false,
                    )],
                    (240.0, 64.0),
                    theme.text,
                )
                .w_full()
                .h(px(64.0)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div().flex_1().min_w_0().child(
                            Slider::new("size", shown as f32, STEPS.0 as f32, STEPS.1 as f32)
                                .on_change(move |value, _, cx| {
                                    let _ = this
                                        .update(cx, |this, cx| set(this, value.round() as u32, cx));
                                }),
                        ),
                    )
                    .child(
                        div()
                            .min_w(px(34.0))
                            .flex()
                            .justify_end()
                            .child(step.to_string()),
                    ),
            )
    }
}

/// The text colour, a little faded: what is said of a setting beside it.
#[allow(dead_code)]
fn muted(theme: &Theme) -> Hsla {
    theme.muted
}
