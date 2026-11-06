//! What goes with a setting, under it, for as long as it is to be seen: it opens out as it comes
//! (taller and taller, and seen more and more, in 0.2 seconds) and closes as it goes, so that what
//! is under it moves out of the way and back rather than being suddenly elsewhere (`Reveal` and
//! `.reveal` in the Tauri version; owner: 「サブ設定が出てくるときはアニメーションしてほしい」).
//!
//! As a page is first drawn it is just there or not, and for a user who has the system move nothing
//! it never moves. Closing, it cannot be pressed; closed, it takes no room at all, not even the gap
//! between two settings.

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    Styled, Window, div, px,
};

use crate::motion;

/// How long it takes to open or close.
const OPENS_IN: Duration = Duration::from_millis(200);
/// The room between the settings of a page, which a closed one gives back.
const GAP: f32 = 4.0;

/// Whether the user has the system animate what is in its windows (the "animation effects" of the
/// accessibility settings): without that, nothing of Perora's moves.
pub fn animates() -> bool {
    #[cfg(windows)]
    {
        perora_core::platform::animates()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[derive(IntoElement)]
pub struct Reveal {
    id: ElementId,
    shown: bool,
    children: Vec<AnyElement>,
}

impl Reveal {
    pub fn new(id: impl Into<ElementId>, shown: bool) -> Self {
        Self {
            id: id.into(),
            shown,
            children: Vec::new(),
        }
    }
}

impl ParentElement for Reveal {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// What was last drawn: whether it was to be seen, since when it is opening or closing (`None` once
/// it is done), and how tall it is when open (as it was last laid out).
struct Drawn {
    shown: bool,
    since: Option<Instant>,
    height: Pixels,
}

impl RenderOnce for Reveal {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let shown = self.shown;
        let drawn = window.use_keyed_state(self.id.clone(), cx, |_, _| Drawn {
            shown,
            since: None,
            height: px(0.0),
        });
        let animates = animates();
        let (since, height) = drawn.update(cx, |drawn, _| {
            if drawn.shown != shown {
                // (Changed again before it was done: it goes back from about where it is.)
                let done = drawn
                    .since
                    .map_or(OPENS_IN, |since| since.elapsed().min(OPENS_IN));
                drawn.since = animates
                    .then(|| Instant::now().checked_sub(OPENS_IN - done))
                    .flatten();
                drawn.shown = shown;
            }
            if drawn.since.is_some_and(|since| since.elapsed() >= OPENS_IN) {
                drawn.since = None;
            }
            (drawn.since, drawn.height)
        });
        let share = match since {
            Some(since) => {
                let eased = motion::EASE.at(motion::along(since, Duration::ZERO, OPENS_IN, window));
                if shown { eased } else { 1.0 - eased }
            }
            None => 1.0,
        };

        // Closed, and done closing: not there at all.
        if !shown && since.is_none() {
            return div().hidden();
        }
        let measured = drawn.clone();
        let within = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .pt(px(GAP))
            .children(self.children);
        div()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .mt(px(-GAP))
            .overflow_hidden()
            .when(since.is_some(), |reveal| {
                reveal.h(height * share).opacity(share)
            })
            .on_children_prepainted(move |bounds, _, cx| {
                if let Some(within) = bounds.first() {
                    let height = within.size.height;
                    measured.update(cx, |drawn, _| drawn.height = height);
                }
            })
            .child(within)
            // (Closing, nothing in it is to be pressed: as the Tauri version's `inert`.)
            .when(!shown, |reveal| {
                reveal.child(div().absolute().inset_0().occlude())
            })
    }
}
