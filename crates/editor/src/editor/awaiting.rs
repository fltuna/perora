//! The editor with no image in it: asked for from the main window. An image is dropped on it,
//! chosen, or taken from the clipboard (its button, Ctrl+V); Esc, or the button, closes it (as
//! closing the window does). And an image dragged over the editor with one in it: what dropping it
//! does, over all of it.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, ExternalPaths, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use perora_ui::Theme;

use super::{Editor, Event, Open, glyph, tip};

/// The picture the well shows (Segoe Fluent Icons: a photo).
const PHOTO: &str = "\u{E91B}";

/// The first file of those dropped: the one opened (one at a time).
pub(super) fn first_of(paths: &ExternalPaths) -> Option<PathBuf> {
    paths.paths().first().cloned()
}

impl Editor {
    /// The keys of the editor waiting for an image: Esc closes it, Ctrl+V opens what is on the
    /// clipboard. (Ctrl+V on the editor with an image puts back what was copied of the drawing.)
    pub(super) fn awaiting_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if event.is_held {
            return;
        }
        if key == "escape" && !modifiers.modified() {
            cx.stop_propagation();
            cx.emit(Event::Discard);
        } else if key == "v"
            && modifiers.control
            && !modifiers.alt
            && !modifiers.shift
            && !modifiers.platform
        {
            cx.stop_propagation();
            cx.emit(Event::Open(Open::Clipboard));
        }
    }

    /// What the window shows while it waits: the well to drop an image in, and the buttons.
    pub(super) fn awaiting_view(
        &self,
        theme: &Theme,
        dropping: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| self.language.text(key, &[]);
        let button = |id: &'static str, label: String, primary: bool| {
            div()
                .id(id)
                .h(px(32.0))
                .px(px(16.0))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .border_1()
                .text_size(theme.text_size)
                .when(primary, |button| {
                    button
                        .bg(theme.accent)
                        .border_color(theme.accent)
                        .text_color(theme.accent_text)
                })
                .when(!primary, |button| {
                    let hover = theme.accent;
                    button
                        .bg(theme.surface)
                        .border_color(theme.border)
                        .text_color(theme.text)
                        .hover(move |style| style.border_color(hover))
                })
                .child(label)
        };
        let clipboard = t("preview.awaiting.clipboard");
        div().size_full().p(px(16.0)).flex().child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .rounded(px(8.0))
                .border_2()
                .border_dashed()
                .map(|well| {
                    if dropping {
                        well.text_color(theme.text)
                            .border_color(theme.accent)
                            .bg(theme.accent.opacity(0.08))
                    } else {
                        well.text_color(theme.muted).border_color(theme.border)
                    }
                })
                .child(glyph(PHOTO, 48.0))
                .child(div().text_size(px(16.0)).child(if dropping {
                    t("preview.drop")
                } else {
                    t("preview.awaiting")
                }))
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            button("choose", t("preview.awaiting.choose"), true).on_click(
                                cx.listener(|_, _, _, cx| cx.emit(Event::Open(Open::Choose))),
                            ),
                        )
                        .child(
                            button("clipboard", clipboard.clone(), false)
                                .tooltip(tip(format!("{clipboard} (Ctrl+V)")))
                                .on_click(
                                    cx.listener(|_, _, _, cx| {
                                        cx.emit(Event::Open(Open::Clipboard))
                                    }),
                                ),
                        )
                        .child(
                            button("close", t("preview.awaiting.close"), false)
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Discard))),
                        ),
                ),
        )
    }

    /// Over the editor with an image, while one is dragged over it: dropping it opens it here (asked
    /// first, when this one is drawn on). It takes no pointer: the drop is the window's to hear.
    pub(super) fn dropping_view(&self, theme: &Theme) -> impl IntoElement + use<> {
        div()
            .absolute()
            .inset(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(18.0))
            .text_color(theme.text)
            .bg(theme.background.opacity(0.82))
            .border_2()
            .border_dashed()
            .border_color(theme.accent)
            .rounded(px(8.0))
            .child(
                div()
                    .px(px(20.0))
                    .py(px(12.0))
                    .bg(theme.surface)
                    .rounded(px(8.0))
                    .child(self.language.text("preview.drop", &[])),
            )
    }
}
