//! The account page, as the Tauri version's (`main_window.tsx`, `Account` and `Recent`): signed
//! out, a card to sign in with; while the browser is awaited, a card to send it there again or to
//! give up; signed in, who it is and signing out, a warning when the account may not upload, and
//! the user's recent uploads (a click opens the page to share, the button copies its link).

use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, CursorStyle, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, div, img, px,
};
use perora_core::language::Language;
use perora_ui::{Button, Ring, Theme, Tip, glyph};

use super::MainWindow;
use super::rows::{about, card, hint_text, notice};
use crate::account::{self, Recent, Upload};

/// Segoe Fluent Icons, as the Tauri version has them.
const COPY: &str = "\u{E8C8}";
const DONE: &str = "\u{E73E}";
/// How long the button says the link was copied.
const COPIED_FOR: Duration = Duration::from_millis(1600);
/// How many uploads go across, and how high their pictures are.
const COLUMNS: u16 = 5;
const PICTURE_HEIGHT: f32 = 96.0;

pub fn page(
    this: &MainWindow,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> Vec<AnyElement> {
    let t = |key: &str| -> SharedString { language.text(key, &[]).into() };
    let Some(accounts) = account::get(cx) else {
        return Vec::new();
    };

    if accounts.waiting.is_some() {
        return vec![
            card(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(Ring::new(16.0))
                    .child(about(t("main.account.waiting"), None, theme)),
                Some(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("reopen", t("main.account.reopen"))
                                .on_click(|_, cx| account::sign_in(cx)),
                        )
                        .child(
                            Button::new("cancelSignIn", t("main.account.cancel"))
                                .on_click(|_, cx| account::cancel_sign_in(cx)),
                        )
                        .into_any_element(),
                ),
                false,
                theme,
            )
            .into_any_element(),
        ];
    }

    let Some(signed_in) = accounts.signed_in.clone() else {
        return vec![
            card(
                about(
                    t("main.account.signedOut"),
                    Some(t("main.account.signedOutHint")),
                    theme,
                ),
                Some(
                    Button::new("signIn", t("main.account.signIn"))
                        .accent()
                        .on_click(|_, cx| account::sign_in(cx))
                        .into_any_element(),
                ),
                false,
                theme,
            )
            .into_any_element(),
        ];
    };

    let who: SharedString = match &signed_in.name {
        Some(name) => language
            .text("main.account.signedInAs", &[("name", name)])
            .into(),
        None => t("main.account.signedIn"),
    };
    let mut page = vec![
        card(
            about(who, None, theme),
            Some(
                Button::new("signOut", t("main.account.signOut"))
                    .on_click(|_, cx| account::sign_out(cx))
                    .into_any_element(),
            ),
            false,
            theme,
        )
        .into_any_element(),
    ];
    if signed_in.may_upload == Some(false) {
        page.push(
            notice(t("main.account.mayNotUpload"), true, theme)
                .mt(px(10.0))
                .into_any_element(),
        );
    }
    let uploads = accounts.recent.clone();
    page.push(recent(this, &uploads, language, theme, cx).into_any_element());
    page
}

/// The user's latest uploads.
fn recent(
    this: &MainWindow,
    recent: &Recent,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> impl IntoElement {
    let t = |key: &str| -> SharedString { language.text(key, &[]).into() };
    let shown = match recent {
        Recent::Asking => div().child(Ring::new(22.0)).into_any_element(),
        Recent::Unreachable => {
            notice(t("main.account.recentUnreachable"), false, theme).into_any_element()
        }
        Recent::Got(uploads) if uploads.is_empty() => {
            hint_text(t("main.account.recentNone"), theme)
                .text_size(px(14.0))
                .into_any_element()
        }
        // (As the Tauri version's grid of 150px at least: five across the window at its narrowest,
        // each as wide as its column.)
        Recent::Got(uploads) => div()
            .grid()
            .grid_cols(COLUMNS)
            .gap(px(10.0))
            .children(
                uploads
                    .iter()
                    .map(|upload| shot(this, upload, language, theme, cx)),
            )
            .into_any_element(),
    };
    div()
        .mt(px(20.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(14.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(t("main.account.recent")),
        )
        .child(shown)
}

/// One upload: its picture, when it was made, and the button that copies its link.
fn shot(
    this: &MainWindow,
    upload: &Upload,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> AnyElement {
    let opened = upload.url.clone();
    let copied = upload.url.clone();
    let id = upload.id.clone();
    let is_copied = this.copied_link.as_deref() == Some(upload.id.as_str());
    let open_tip: SharedString = language.text("main.account.openPage", &[]).into();
    let copy_tip: SharedString = language.text("main.account.copyLink", &[]).into();
    let picture = div()
        .id(SharedString::from(format!("shot-{}", upload.id)))
        .w_full()
        .h(px(PICTURE_HEIGHT))
        .overflow_hidden()
        .bg(theme.background)
        .cursor(CursorStyle::PointingHand)
        .tooltip(move |_, cx| Tip::view(open_tip.clone(), cx))
        .on_click(move |_, _, cx| account::open_page(&opened, cx))
        .when_some(upload.picture.clone(), |picture, shown| {
            picture.child(img(shown).size_full().object_fit(ObjectFit::Cover))
        });
    let window = cx.entity().downgrade();
    let copy = div()
        .id(SharedString::from(format!("copy-{}", upload.id)))
        .flex_none()
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .cursor(CursorStyle::PointingHand)
        .hover(|button| button.bg(theme.surface_hover))
        .when(is_copied, |button| button.text_color(theme.good))
        .tooltip(move |_, cx| Tip::view(copy_tip.clone(), cx))
        .child(glyph(if is_copied { DONE } else { COPY }, 14.0))
        .on_click(move |_, _, cx| {
            if !account::copy_link(&copied, cx) {
                return;
            }
            let id = id.clone();
            let _ = window.update(cx, |this, cx| {
                this.copied_link = Some(id.clone());
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(COPIED_FOR).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.copied_link.as_deref() == Some(id.as_str()) {
                            this.copied_link = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            });
        });
    let when = upload
        .when
        .map(|when| written(when, language))
        .unwrap_or_default();
    div()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .rounded(px(8.0))
        .overflow_hidden()
        .child(picture)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(4.0))
                .py(px(2.0))
                .pl(px(8.0))
                .pr(px(2.0))
                .child(
                    hint_text(when, theme)
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis(),
                )
                .child(copy),
        )
        .into_any_element()
}

/// When an upload was made, as the browser wrote it for the Tauri version (`dateStyle: medium,
/// timeStyle: short`): `2026/11/07 15:12` in Japanese, `Nov 7, 2026, 3:12 PM` in English.
fn written([year, month, day, hour, minute]: [u16; 5], language: &Language) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if language.code().starts_with("ja") {
        return format!("{year}/{month:02}/{day:02} {hour}:{minute:02}");
    }
    let month = MONTHS
        .get(usize::from(month.saturating_sub(1)))
        .copied()
        .unwrap_or("?");
    let (twelve, half) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!("{month} {day}, {year}, {twelve}:{minute:02} {half}")
}

#[cfg(test)]
mod tests {
    use super::written;
    use perora_core::language::Languages;

    #[test]
    fn the_moment_is_written_as_the_browser_wrote_it() {
        let languages = Languages::load(None);
        let ja = languages.by_code("ja").unwrap();
        let en = languages.by_code("en").unwrap();
        assert_eq!(written([2026, 11, 7, 15, 2], &ja), "2026/11/07 15:02");
        assert_eq!(written([2026, 11, 7, 15, 2], &en), "Nov 7, 2026, 3:02 PM");
        assert_eq!(written([2026, 1, 31, 0, 5], &en), "Jan 31, 2026, 12:05 AM");
        assert_eq!(written([2026, 1, 31, 12, 0], &en), "Jan 31, 2026, 12:00 PM");
    }
}
