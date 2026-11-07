//! What the main window shows of updates (`crate::update`): the band at the top of every page that
//! offers a newer version, the "Updates" part of the general page, and what changed in the version
//! offered, as `CHANGELOG/v<version>_CHANGELOGS*.md` says it.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, CursorStyle, Div, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};
use perora_core::changelog::{self, Block};
use perora_core::language::Language;
use perora_ui::{Button, Theme, Tip, glyph};

use super::rows::{about, card, hint_text, mix, section};
use crate::update::{self, Asked, View};

/// Segoe Fluent Icons, as the Tauri version has them.
const DOWNLOAD: &str = "\u{E896}";
const CLOSE: &str = "\u{E711}";
const OPEN: &str = "\u{E70D}";
const SHUT: &str = "\u{E70E}";

/// How tall what changed may be in the band before it scrolls.
const CHANGES_AT_MOST: f32 = 240.0;

fn said(language: &Language) -> impl Fn(&str) -> SharedString + use<> {
    let language = language.clone();
    move |key: &str| language.text(key, &[]).into()
}

/// How far the download is, as the band says it.
fn downloading(view: &View, language: &Language) -> String {
    match (view.got, view.of) {
        (Some(got), Some(of)) if of > 0 => {
            let percent = ((got as f64 / of as f64) * 100.0).floor().min(100.0) as u64;
            language.text("main.update.downloading", &[("percent", &percent.to_string())])
        }
        _ => language.text("main.update.downloadingSome", &[]),
    }
}

/// The band that offers a newer version, above the pages: none when there is none to offer, or
/// the user skipped it or closed it.
pub fn band(view: &View, language: &Language, theme: &Theme, cx: &mut App) -> Option<AnyElement> {
    let _ = cx;
    if !view.band {
        return None;
    }
    let version = view.available.clone()?;
    let t = said(language);
    // What it says under its title, the first of these that holds.
    let status: Option<SharedString> = if view.installing {
        Some(t("main.update.installing"))
    } else if view.getting() {
        Some(downloading(view, language).into())
    } else if view.capture_in_hand {
        Some(t("main.update.captureInHand"))
    } else {
        view.failure.clone().map(SharedString::from)
    };
    let title = language.text("main.update.title", &[("version", &version)]);
    let buttons = (!view.getting()).then(|| {
        let skipped = version.clone();
        let closed = version.clone();
        let close_tip = t("main.update.close");
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .when(view.changes.is_some(), |buttons| {
                buttons.child(
                    Button::new("updateChanges", t("main.update.changes"))
                        .icon(if view.changes_open { SHUT } else { OPEN })
                        .on_click(|_, cx| update::toggle_changes(cx)),
                )
            })
            .child(
                Button::new("updateInstall", t("main.update.install"))
                    .accent()
                    .enabled(!view.capture_in_hand)
                    .on_click(|_, cx| update::install(cx)),
            )
            .child(
                Button::new("updateSkip", t("main.update.skip"))
                    .on_click(move |_, cx| update::skip(&skipped, cx)),
            )
            .child(
                div()
                    .id("updateClose")
                    .flex_none()
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius_small)
                    .cursor(CursorStyle::PointingHand)
                    .hover(|button| button.bg(theme.surface_hover))
                    .tooltip(move |_, cx| Tip::view(close_tip.clone(), cx))
                    .child(glyph(CLOSE, 12.0))
                    .on_click(move |_, _, cx| update::close(&closed, cx)),
            )
    });
    let open_changes = view
        .changes
        .as_deref()
        .filter(|_| view.changes_open && !view.getting());
    Some(
        div()
            .flex_none()
            .flex()
            .flex_col()
            .bg(mix(theme.accent, theme.surface, 0.10))
            .border_b_1()
            .border_color(mix(theme.accent, theme.border, 0.35))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .child(div().flex_none().text_color(theme.accent).child(glyph(DOWNLOAD, 16.0)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                            .when_some(status, |text, status| text.child(hint_text(status, theme))),
                    )
                    .when_some(buttons, |band, buttons| band.child(buttons)),
            )
            .when_some(open_changes, |band, markdown| {
                band.child(
                    div()
                        .id("updateChangesShown")
                        .max_h(px(CHANGES_AT_MOST))
                        .overflow_y_scroll()
                        .pl(px(44.0))
                        .pr(px(16.0))
                        .pb(px(12.0))
                        .child(changes(markdown, Some(&version), theme)),
                )
            })
            .into_any_element(),
    )
}

/// The "Updates" part of the general page: this version, what the last look came to, and the
/// buttons to install a newer one and to look again; what changed in a newer one under it.
pub fn general(view: &View, language: &Language, theme: &Theme) -> Vec<AnyElement> {
    let t = said(language);
    let hint: SharedString = if let Some(failure) = &view.failure {
        failure.clone().into()
    } else if view.installing {
        t("main.update.installing")
    } else if view.getting() {
        downloading(view, language).into()
    } else if view.asked == Asked::Looking {
        t("main.general.versionHint.looking")
    } else if let Some(version) = &view.available {
        let available = language.text("main.general.versionHint.available", &[("version", version)]);
        if view.capture_in_hand {
            format!("{available} {}", t("main.update.captureInHand")).into()
        } else {
            available.into()
        }
    } else {
        match view.asked {
            Asked::UpToDate => t("main.general.versionHint.upToDate"),
            Asked::Failed => t("main.general.versionHint.failed"),
            _ => t("main.general.versionHint"),
        }
    };
    let title = language.text("main.general.version", &[("version", update::VERSION)]);
    let buttons = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .when(view.available.is_some() && !view.getting(), |buttons| {
            buttons.child(
                Button::new("generalInstall", t("main.update.install"))
                    .accent()
                    .enabled(!view.capture_in_hand)
                    .on_click(|_, cx| update::install(cx)),
            )
        })
        .child(
            Button::new("generalCheck", t("main.general.check"))
                .enabled(view.asked != Asked::Looking && !view.getting())
                .turning(view.asked == Asked::Looking)
                .on_click(|_, cx| update::look_now(cx)),
        );
    let mut part = vec![
        section(t("main.general.section.update"), false, theme).into_any_element(),
        card(
            about(title, Some(hint), theme),
            Some(buttons.into_any_element()),
            false,
            theme,
        )
        .into_any_element(),
    ];
    if let (Some(version), Some(markdown)) = (&view.available, &view.changes) {
        part.push(
            div()
                .flex_none()
                .mt(px(4.0))
                .px(px(16.0))
                .py(px(12.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius_small)
                .child(changes(markdown, Some(version), theme))
                .into_any_element(),
        );
    }
    part
}

/// What changed in every version up to this one, as built in (`CHANGELOG/CHANGELOGS.md`, and
/// `.en.md` for English, made by `CHANGELOG/combine.ps1`).
const CHANGELOG_JA: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../CHANGELOG/CHANGELOGS.md"));
const CHANGELOG_EN: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../CHANGELOG/CHANGELOGS.en.md"));

/// The changelog page: a part for each version, newest first (as the file has them), in the
/// window's language, or the other where it says nothing.
pub fn changelog(language: &Language, theme: &Theme) -> Vec<AnyElement> {
    let (asked, other) = if language.code().starts_with("ja") {
        (CHANGELOG_JA, CHANGELOG_EN)
    } else {
        (CHANGELOG_EN, CHANGELOG_JA)
    };
    let mut versions = changelog::by_version(asked);
    if versions.is_empty() {
        versions = changelog::by_version(other);
    }
    let mut part = Vec::new();
    for (index, (title, markdown)) in versions.into_iter().enumerate() {
        part.push(section(title, index == 0, theme).into_any_element());
        part.push(
            div()
                .flex_none()
                .px(px(16.0))
                .py(px(12.0))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius_small)
                .child(changes(markdown, None, theme))
                .into_any_element(),
        );
    }
    part
}

/// What changed, as the Markdown says it: headings, items of lists (set in by how deep they are),
/// paragraphs. A heading that opens it with the version (`## v0.2.0`) is left out where the version
/// is said already, unless more versions follow it (what changed since the one installed, a
/// heading each).
fn changes(markdown: &str, version: Option<&str>, theme: &Theme) -> Div {
    let mut blocks = changelog::blocks(markdown);
    let versions = blocks
        .iter()
        .filter(|block| matches!(block, Block::Heading { level: ..=2, .. }))
        .count();
    if let (Some(Block::Heading { text, .. }), Some(version)) = (blocks.first(), version)
        && versions == 1
        && text
            .trim_start_matches('v')
            .split_whitespace()
            .next()
            .is_some_and(|first| first == version)
    {
        blocks.remove(0);
    }
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .text_size(px(13.0))
        .children(blocks.into_iter().enumerate().map(|(index, block)| match block {
            Block::Heading { level, text } => div()
                .when(index > 0, |heading| heading.mt(px(8.0)))
                .text_size(px(if level <= 2 { 15.0 } else { 13.0 }))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
            Block::Item { depth, text } => div()
                .flex()
                .gap(px(8.0))
                .pl(px(4.0 + 18.0 * depth as f32))
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.muted)
                        .child(if depth == 0 { "•" } else { "◦" }),
                )
                .child(div().flex_1().min_w_0().child(text)),
            Block::Paragraph(text) => div().when(index > 0, |text| text.mt(px(4.0))).child(text),
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_changelogs_name_this_version() {
        // (A prerelease need not say what changed in it, as the release workflow has it.)
        if env!("CARGO_PKG_VERSION").contains('-') {
            return;
        }
        let this = format!("v{}", env!("CARGO_PKG_VERSION"));
        for combined in [CHANGELOG_JA, CHANGELOG_EN] {
            assert!(changelog::by_version(combined).iter().any(|(title, _)| *title == this));
        }
    }
}
