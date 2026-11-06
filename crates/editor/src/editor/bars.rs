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
    Bounds, BoxShadow, Context, Div, InteractiveElement, IntoElement, Pixels, SharedString, Styled,
    canvas, div, point, px, rgba,
};
use perora_core::edit::{Colour, Edit};
use perora_ui::Theme;

use super::Editor;
use crate::tools::Kind;

/// The panels that open under a button: of the pen (under its button on the first bar), of the
/// stroke of the pen selected, of a shape's inside, and of the outline of shapes and lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Panel {
    Pen,
    Marker,
    Stamps,
    Stroke,
    Fill,
    Outline,
    /// What opens from the box by what is written: its inside, the line around it, its font.
    LetterFill,
    LetterOutline,
    Fonts,
}

impl Panel {
    /// Whether it opens from the box by what is written (not under a button of the bars).
    pub(super) fn of_letters(self) -> bool {
        matches!(
            self,
            Panel::LetterFill | Panel::LetterOutline | Panel::Fonts
        )
    }
}

/// Which second bar there is to have: of the shapes and lines in hand, or of what is selected (a
/// shape, a line or an arrow, a stroke of the pen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Offered {
    Figures,
    Shape,
    Line,
    Stroke,
    /// The numbers: of the tool in hand, or of the one selected.
    Stamps,
    /// The two ways to make a part unreadable and how strongly (of the tool in hand); how strongly
    /// alone (of the one selected).
    Hiding,
    Hidden,
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

impl Panel {
    /// The panel of the pen or the marker, under its button.
    pub(super) fn of_stroke(kind: Kind) -> Self {
        if kind == Kind::Marker {
            Panel::Marker
        } else {
            Panel::Pen
        }
    }
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
pub(super) type Pick = Rc<dyn Fn(&mut Editor, Option<Colour>, &mut Context<Editor>)>;
/// What a size set in a panel does.
pub(super) type SetStep = Rc<dyn Fn(&mut Editor, u32, &mut Context<Editor>)>;

/// The steps of a width.
pub(super) const STEPS: (u32, u32) = (1, 20);

/// How wide a panel is.
pub(super) const PANEL: f32 = 322.0;

/// What a size is shown by: a stroke of the pen, or a line (`Size` in editor.tsx, in a box 240 by
/// 64).
pub(super) const PEN_SAMPLE: &str = "M24 40C64 4 100 12 124 34S184 60 216 26";
pub(super) const LINE_SAMPLE: &str = "M24 32H216";

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
pub(super) fn recorded(openers: &Openers, panel: Panel) -> impl IntoElement + use<> {
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

pub(super) fn shadow(down: f32, blur: f32, dark: u32) -> Vec<BoxShadow> {
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

    pub(super) fn chosen_edit(&self) -> Option<&Edit> {
        self.chosen()
            .and_then(|index| self.drawing.edits().get(index))
    }

    /// The second bar there is to have, open or not.
    pub(super) fn offered(&self) -> Option<Offered> {
        match self.chosen_edit() {
            Some(Edit::Pen { .. } | Edit::Marker { .. }) => Some(Offered::Stroke),
            Some(Edit::Rectangle { .. } | Edit::Ellipse { .. }) => Some(Offered::Shape),
            Some(Edit::Line { .. } | Edit::Arrow { .. }) => Some(Offered::Line),
            Some(Edit::Stamp { .. }) => Some(Offered::Stamps),
            Some(Edit::Blur { .. } | Edit::Mosaic { .. }) => Some(Offered::Hidden),
            Some(_) => None,
            None if self.tool.is_figure() => Some(Offered::Figures),
            None if self.tool == Kind::Stamp => Some(Offered::Stamps),
            None if self.tool.is_hider() => Some(Offered::Hiding),
            None => None,
        }
    }

    /// The second bar that is there.
    pub(super) fn second(&self) -> Option<Offered> {
        if self.shut { None } else { self.offered() }
    }

    /// Whether a shape's inside can be set: of a shape (not of a line).
    pub(super) fn fills(&self) -> bool {
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
            Panel::Marker => self.tool == Kind::Marker && self.chosen().is_none(),
            Panel::Stroke => second == Some(Offered::Stroke),
            Panel::Fill => self.fills(),
            Panel::Outline => matches!(
                second,
                Some(Offered::Figures | Offered::Shape | Offered::Line)
            ),
            Panel::Stamps => second == Some(Offered::Stamps),
            Panel::LetterFill | Panel::LetterOutline | Panel::Fonts => self.lettering_shown,
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

    /// The colour and size of the strokes of the pen (or the `marker`): what it draws next, or the
    /// one selected.
    pub(super) fn stroke_now(&self, marker: bool) -> (Colour, u32) {
        match (self.chosen_edit(), marker) {
            (Some(Edit::Pen { color, width, .. }), false)
            | (Some(Edit::Marker { color, width, .. }), true) => (*color, self.step_of(*width)),
            (_, false) => (self.pen.colour, self.pen.step),
            (_, true) => (self.marker.colour, self.marker.step),
        }
    }

    /// The colour and size (in steps) of the numbers: what is put down next, or the one selected.
    pub(super) fn stamps_now(&self) -> (Colour, u32) {
        match self.chosen_edit() {
            Some(edit @ Edit::Stamp { color, .. }) => {
                let across = crate::marks::box_of(edit).width;
                let step = (across / (crate::marks::stamp::STEP * self.unit())).round();
                (*color, step.max(1.0) as u32)
            }
            _ => (self.stamps.colour, self.stamps.step),
        }
    }

    /// A setting of the numbers: for the next, and for the one the settings are about (put down
    /// anew around its middle).
    pub(super) fn set_stamping(&mut self, set: Set, dragged: bool, cx: &mut Context<Self>) {
        if self.frozen {
            return;
        }
        let (colour, step) = self.stamps_now();
        let (colour, step) = match set {
            Set::Colour(to) => (to, step),
            Set::Step(to) => (colour, to),
            _ => return,
        };
        self.stamps.colour = colour;
        self.stamps.step = step;
        let unit = self.unit();
        let Some(index) = self.about() else {
            cx.notify();
            return;
        };
        let Some(edit @ Edit::Stamp { number, .. }) = self.drawing.edits().get(index).cloned()
        else {
            return;
        };
        let area = crate::marks::box_of(&edit);
        let middle = [area.x + area.width / 2.0, area.y + area.height / 2.0];
        let across = match set {
            Set::Step(step) => step as f32 * crate::marks::stamp::STEP * unit,
            _ => area.width,
        };
        let next = crate::marks::stamp::stamped(middle, across, number, colour);
        self.alter(index, next, set, dragged, cx);
        cx.notify();
    }

    /// Whether the stroke selected is the marker's.
    pub(super) fn chosen_is_marker(&self) -> bool {
        matches!(self.chosen_edit(), Some(Edit::Marker { .. }))
    }

    /// The outline of shapes and lines: what is drawn next, or the one selected.
    pub(super) fn outline_now(&self) -> (Colour, u32) {
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

    /// A setting of the pen (or the `marker`): for what it draws next, and for what the settings are
    /// about.
    pub(super) fn set_stroke(
        &mut self,
        marker: bool,
        set: Set,
        dragged: bool,
        cx: &mut Context<Self>,
    ) {
        if self.frozen {
            return;
        }
        let tool = if marker {
            &mut self.marker
        } else {
            &mut self.pen
        };
        match set {
            Set::Colour(colour) => tool.colour = colour,
            Set::Step(step) => tool.step = step,
            _ => return,
        }
        let unit = self.unit();
        let Some(index) = self.about() else {
            cx.notify();
            return;
        };
        let mut next = match self.drawing.edits().get(index) {
            Some(edit @ Edit::Pen { .. }) if !marker => edit.clone(),
            Some(edit @ Edit::Marker { .. }) if marker => edit.clone(),
            _ => {
                cx.notify();
                return;
            }
        };
        if let Edit::Pen { color, width, .. } | Edit::Marker { color, width, .. } = &mut next {
            match set {
                Set::Colour(colour) => *color = colour,
                Set::Step(step) => *width = step as f32 * unit,
                _ => {}
            }
        }
        self.alter(index, next, set, dragged, cx);
        cx.notify();
    }

    /// A setting of the outline of shapes and lines.
    pub(super) fn set_outline(&mut self, set: Set, dragged: bool, cx: &mut Context<Self>) {
        if self.frozen {
            return;
        }
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
        if self.frozen {
            return;
        }
        let (colour, fill) = self.inside_now();
        let (colour, fill) = match set {
            Set::Inside(Some(to)) => (to, if fill == 0.0 { self.solid } else { fill }),
            Set::Inside(None) => (colour, 0.0),
            Set::Solid(solid) => (colour, solid),
            _ => return,
        };
        // (How solid it was set to by its slider: what an inside given a colour is again. Not what a
        // shape chosen had.)
        if let Set::Solid(solid) = set {
            self.solid = solid;
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
                // (Its colour only when it is given one: no inside, or another solidness, leaves the
                // colour it says, or does not say, as it is.)
                if let Set::Inside(Some(to)) = set {
                    *fill_color = Some(to);
                }
            }
            _ => return,
        }
        self.alter(index, next, set, dragged, cx);
        cx.notify();
    }

    /// A press on a button that opens a panel: it opens, or goes when it is open.
    pub(super) fn open(&mut self, panel: Panel, cx: &mut Context<Self>) {
        if self.frozen {
            return;
        }
        self.asked = if self.panel() == Some(panel) {
            None
        } else {
            Some(panel)
        };
        cx.notify();
    }

    /// The pen's (or the marker's) tool button records where it is: its panel opens under it.
    pub(super) fn stroke_opener(&self, kind: Kind) -> impl IntoElement + use<> {
        recorded(&self.openers, Panel::of_stroke(kind))
    }
}
