//! The tour of the editor: the tools one by one (selecting last: it is about what the others drew),
//! what the mouse does on the picture beyond drawing, looking closer, what is done with the capture
//! and how it leaves (P6 of docs/gpui-rewrite.md; the Tauri version's `tours.tsx`). It comes once on
//! its own, a moment after the first capture is shown here, and again from its "i" at the top left.

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, CursorStyle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use perora_ui::Theme;
use perora_ui::tour::{Asked, At, Button, Keyed, Look, Step, Tour, Words};

use super::{Editor, Event, glyph, tip};

/// A moment after the window came, so that it is seen to come first.
const COMES_AFTER: Duration = Duration::from_millis(400);
/// The "i" in a ring (Segoe Fluent Icons: Info).
const GUIDE: &str = "\u{E946}";

/// What a step of the tour has the editor show meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Showing {
    Nothing,
    /// A tool used on the picture, as it would be.
    Tool(Played),
    /// The other button held and the wheel turned: the tools one after another.
    Turning,
    /// The other button pressed twice: the settings there.
    Asking,
    /// The wheel turned: the picture looked at closer, and back.
    Looking,
    /// The choice of what the capture leaves as, open.
    Choosing,
}

/// The tools the tour uses on the picture, one step each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Played {
    Pen,
    Marker,
    Shapes,
    Text,
    Stamp,
    Hide,
    Crop,
    Select,
}

impl Played {
    const ALL: [Played; 8] = [
        Played::Pen,
        Played::Marker,
        Played::Shapes,
        Played::Text,
        Played::Stamp,
        Played::Hide,
        Played::Crop,
        Played::Select,
    ];

    /// The mark of its button, and what is said of it.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Played::Pen => ("tool-pen", "tour.tool.pen"),
            Played::Marker => ("tool-marker", "tour.tool.marker"),
            Played::Shapes => ("tool-shapes", "tour.tool.shapes"),
            Played::Text => ("tool-text", "tour.tool.text"),
            Played::Stamp => ("tool-stamp", "tour.tool.stamp"),
            Played::Hide => ("tool-hide", "tour.tool.hide"),
            Played::Crop => ("tool-crop", "tour.tool.crop"),
            Played::Select => ("tool-select", "tour.editor.select"),
        }
    }
}

pub(super) struct Touring {
    pub tour: Tour,
    /// What each step shows meanwhile (by the step).
    pub showing: Vec<Showing>,
}

/// Whether the tour is to come on its own, and when.
#[derive(Default)]
pub(super) struct Offered {
    /// It was seen (to the end, or put away): it does not come on its own.
    pub seen: bool,
    /// It comes at this moment, unless something else happened meanwhile.
    pub at: Option<Instant>,
}

impl Editor {
    /// Whether the tour of the editor was seen (the settings say): it does not come on its own then.
    pub fn set_tour_seen(&mut self, seen: bool) {
        self.offered.seen = seen;
    }

    /// Whether uploading is on offer: what the tour says of the things to do with the capture.
    pub fn set_offers_upload(&mut self, offers: bool) {
        self.offers_upload = offers;
    }

    /// The window is in sight with a capture: the tour comes on its own a moment later, if it was
    /// not seen.
    pub fn offer_tour(&mut self, cx: &mut Context<Self>) {
        if self.offered.seen || self.touring.is_some() || self.capture.is_none() {
            return;
        }
        let at = Instant::now() + COMES_AFTER;
        self.offered.at = Some(at);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COMES_AFTER).await;
            let _ = this.update(cx, |editor, cx| {
                if editor.offered.at == Some(at) && !editor.offered.seen {
                    editor.offered.at = None;
                    editor.start_tour(cx);
                }
            });
        })
        .detach();
    }

    /// Whether the tour is on: nothing else hears the keys or the pointer meanwhile.
    pub fn touring(&self) -> bool {
        self.touring.is_some()
    }

    /// Whether the step of the tour on now has the choice of what the capture leaves as open.
    pub fn tour_opens_choice(&self) -> bool {
        self.touring.as_ref().is_some_and(|touring| {
            touring.showing.get(touring.tour.at()) == Some(&Showing::Choosing)
        })
    }

    /// The tour, from its first step.
    pub fn start_tour(&mut self, cx: &mut Context<Self>) {
        if self.capture.is_none() || self.frozen {
            return;
        }
        self.offered.at = None;
        let (steps, showing) = self.tour_steps();
        let motion = perora_core::platform::animations_enabled();
        self.touring = Some(Touring {
            tour: Tour::new(steps, motion, false),
            showing,
        });
        self.step_shown(cx);
        cx.notify();
    }

    /// The tour is over: `seen` (to the end, or put away) it does not come on its own again; put
    /// away by the window going out of sight, it is not seen.
    pub fn end_tour(&mut self, seen: bool, cx: &mut Context<Self>) {
        self.offered.at = None;
        if self.touring.is_none() {
            return;
        }
        self.stop_playing(cx);
        self.touring = None;
        if seen && !self.offered.seen {
            self.offered.seen = true;
            cx.emit(Event::Toured);
        }
        cx.notify();
    }

    /// The steps, in the user's language: and what each has the editor show.
    fn tour_steps(&self) -> (Vec<Step>, Vec<Showing>) {
        let t = |key: &str| self.language.text(key, &[]);
        let mut steps = vec![(
            Step::new(At::Mark("tools"), t("tour.editor.tools")),
            Showing::Nothing,
        )];
        for played in Played::ALL {
            let (mark, said) = played.names();
            steps.push((
                Step::new(At::Mark(mark), t(said)).look(Look::Ring),
                Showing::Tool(played),
            ));
        }
        steps.extend([
            (
                Step::new(At::Mark("stage"), t("tour.editor.wheel")),
                Showing::Turning,
            ),
            (
                Step::new(At::Mark("stage"), t("tour.editor.right")),
                Showing::Asking,
            ),
            (
                Step::new(At::Mark("stage"), t("tour.editor.zoom")),
                Showing::Looking,
            ),
            (
                Step::new(
                    At::Mark("actions"),
                    t(if self.offers_upload {
                        "tour.editor.actions"
                    } else {
                        "tour.editor.actionsWithoutUpload"
                    }),
                ),
                Showing::Nothing,
            ),
            (
                Step::new(At::Mark("as"), t("tour.preview.as")),
                Showing::Nothing,
            ),
            (
                Step::new(At::Mark("menu"), t("tour.choice")),
                Showing::Choosing,
            ),
            (
                Step::new(At::Mark("status"), t("tour.editor.status")),
                Showing::Nothing,
            ),
            (
                Step::new(At::Mark("guide"), t("tour.again")),
                Showing::Nothing,
            ),
        ]);
        steps.into_iter().unzip()
    }

    /// The words of the tour's own buttons.
    pub(super) fn tour_words(&self) -> Words {
        let language = self.language.clone();
        let t = |key: &str| self.language.text(key, &[]).into();
        Words {
            count: Box::new(move |at, of| {
                language.text(
                    "tour.count",
                    &[("at", &at.to_string()), ("of", &of.to_string())],
                )
            }),
            skip: t("tour.skip"),
            back: t("tour.back"),
            next: t("tour.next"),
            done: t("tour.done"),
        }
    }

    /// A key while the tour is on: the tour's (whether it does something or not), unless it is the
    /// system's. Whether it was the tour's.
    pub(super) fn tour_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(touring) = self.touring.as_mut() else {
            return false;
        };
        match touring.tour.key(event) {
            Keyed::NotMine => false,
            Keyed::Taken(asked) => {
                if let Some(asked) = asked {
                    self.tour_asked(asked, cx);
                }
                cx.stop_propagation();
                true
            }
        }
    }

    /// One of the tour's buttons.
    pub(super) fn tour_button(&mut self, button: Button, cx: &mut Context<Self>) {
        let Some(touring) = self.touring.as_mut() else {
            return;
        };
        let asked = match button {
            Button::Skip => Asked::End,
            Button::Back => touring.tour.back(),
            Button::Next => touring.tour.next(),
        };
        self.tour_asked(asked, cx);
    }

    fn tour_asked(&mut self, asked: Asked, cx: &mut Context<Self>) {
        match asked {
            Asked::End => self.end_tour(true, cx),
            Asked::Moved => self.step_shown(cx),
        }
        cx.notify();
    }

    /// The step on now is shown: what the step before set going is stopped (and what it did undone),
    /// and what this one shows is set going.
    pub(super) fn step_shown(&mut self, cx: &mut Context<Self>) {
        self.stop_playing(cx);
        let Some(touring) = self.touring.as_ref() else {
            return;
        };
        let showing = touring
            .showing
            .get(touring.tour.at())
            .copied()
            .unwrap_or(Showing::Nothing);
        self.start_playing(showing, cx);
    }

    /// The button the tour is shown again by: an "i" in a ring, at the top left.
    pub(super) fn guide_button(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let text = theme.text;
        let hover = theme.surface_hover;
        div()
            .id("guide")
            .relative()
            .size(px(34.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .text_color(theme.muted)
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.text_color(text).bg(hover))
            .tooltip(tip(self.language.text("tour.open", &[])))
            .on_click(cx.listener(|this, _, _, cx| this.start_tour(cx)))
            .child(glyph(GUIDE, 16.0))
            .child(self.marks.mark("guide"))
    }

    /// The tour over the window, while it is on.
    pub(super) fn tour_view(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let words = self.tour_words();
        let marks = self.marks.clone();
        let this = cx.entity().downgrade();
        let hand = self.hand_view();
        let touring = self.touring.as_mut()?;
        let view = touring
            .tour
            .view(&marks, &words, theme, window, move |button, _, cx| {
                let _ = this.update(cx, |editor, cx| editor.tour_button(button, cx));
            });
        Some(
            div()
                .absolute()
                .inset_0()
                .child(view)
                .when_some(hand, |layer, hand| layer.child(hand)),
        )
    }
}
