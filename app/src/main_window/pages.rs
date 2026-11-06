//! The pages of the main window but the shortcut's: home, capture, save, editor, general and account,
//! in the order the Tauri version has their settings (owner, 2026-10-14: 「デカいセパレータ、小さい
//! 見出し + セパレータ + 設定の順番」).
//!
//! What a switch or a box stands for is changed as it is when it is pressed, not as it was drawn:
//! two quick clicks are on and off again.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, SharedString, Styled, Window, div, px,
};
use perora_core::language::Language;
use perora_core::notice::{Corner, Sound};
use perora_core::output::{FITS, Format, Quality};
use perora_core::selection::Mode;
use perora_core::settings::{After, Hider, Leaves, Look, Settings};
use perora_ui::{Button, Check, Item, Reveal, Select, Switch, Theme};

use super::rows::{about, card, hint_text, notice, section, setting};
use super::system::{self, Occasion};
use super::{MainWindow, reveal_area};
use crate::state::Perora;

/// Segoe Fluent Icons, as the Tauri version has them.
const CAMERA: &str = "\u{E722}";
const CLIPBOARD: &str = "\u{E77F}";
const PEN: &str = "\u{E70F}";

/// The texts of a language, by key.
fn said(language: &Language) -> impl Fn(&str) -> SharedString + use<> {
    let language = language.clone();
    move |key: &str| language.text(key, &[]).into()
}

/// A switch for a setting that is on or off: pressed, it is turned the other way, as it is then.
fn switch(id: &'static str, on: bool, flip: fn(&mut Settings)) -> Switch {
    Switch::new(id, on).on_toggle(move |_, cx| system::set(cx, flip))
}

/// A choice among the values of a setting, each said as `label` says it.
fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    values: Vec<T>,
    current: T,
    label: impl Fn(T) -> SharedString,
    set: impl Fn(T, &mut App) + 'static,
) -> Select {
    let labels: Vec<SharedString> = values.iter().map(|value| label(*value)).collect();
    let chosen = values.iter().position(|value| *value == current);
    Select::of(id, labels, chosen).on_change(move |index, _, cx| {
        if let Some(value) = values.get(index) {
            set(*value, cx);
        }
    })
}

pub fn home(
    settings: &Settings,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> Vec<AnyElement> {
    let _ = settings;
    let t = said(language);
    let perora = Perora::get(cx);
    let shown = perora.shortcut_shown();
    let registered = perora.shortcut_label().is_some();
    let hero = |hint: String, right: AnyElement| {
        card(
            hint_text(hint, theme).text_size(px(14.0)),
            Some(right),
            false,
            theme,
        )
    };
    let mut page = vec![
        hero(
            language.text("main.home.captureHint", &[("shortcut", &shown)]),
            Button::new("capture", t("main.home.capture"))
                .icon(CAMERA)
                .accent()
                .on_click(|_, cx| system::capture(cx))
                .into_any_element(),
        )
        .into_any_element(),
        // The editor, for an image of the user's own: chosen or dropped there, or dropped on this
        // window (owner, 2026-11-02).
        hero(
            language.text("main.home.openHint", &[]),
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    Button::new("openClipboard", t("main.home.openClipboard"))
                        .icon(CLIPBOARD)
                        .on_click(|_, cx| system::open_clipboard(cx)),
                )
                .child(
                    Button::new("openEditor", t("main.home.openEditor"))
                        .icon(PEN)
                        .on_click(|_, cx| system::open_editor(cx)),
                )
                .into_any_element(),
        )
        .into_any_element(),
    ];
    if !registered {
        page.push(
            notice(
                language.text("main.home.shortcutTaken", &[("shortcut", &shown)]),
                true,
                theme,
            )
            .into_any_element(),
        );
    }
    page
}

/// What a capture is made into as it leaves one way: the kind of file (several at once, ticked,
/// for saving), and the square it is made to fit when it is larger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaving {
    Copy,
    Upload,
    Save,
    SaveOnCapture,
}

impl Leaving {
    const fn name(self) -> &'static str {
        match self {
            Leaving::Copy => "copy",
            Leaving::Upload => "upload",
            Leaving::Save => "save",
            Leaving::SaveOnCapture => "saveOnCapture",
        }
    }

    fn row(
        self,
        settings: &Settings,
        language: &Language,
        theme: &Theme,
        under: bool,
    ) -> AnyElement {
        let t = said(language);
        let name = self.name();
        let fit = match self {
            Leaving::Copy => settings.fits[Leaves::Copy as usize],
            Leaving::Upload => settings.fits[Leaves::Upload as usize],
            Leaving::Save => settings.fits[Leaves::Save as usize],
            Leaving::SaveOnCapture => settings.as_taken_fit,
        };
        let mut sides = vec![0];
        sides.extend(FITS);
        let fit_said = {
            let language = language.clone();
            move |side: u32| -> SharedString {
                if side == 0 {
                    language.text("main.output.fit.none", &[]).into()
                } else {
                    language
                        .text("main.output.fit.within", &[("side", &side.to_string())])
                        .into()
                }
            }
        };
        let fits = choice(
            match self {
                Leaving::Copy => "copyFit",
                Leaving::Upload => "uploadFit",
                Leaving::Save => "saveFit",
                Leaving::SaveOnCapture => "saveOnCaptureFit",
            },
            sides,
            fit.unwrap_or(0),
            fit_said,
            move |side, cx| {
                let side = (side != 0).then_some(side);
                system::set(cx, |settings| match self {
                    Leaving::Copy => settings.fits[Leaves::Copy as usize] = side,
                    Leaving::Upload => settings.fits[Leaves::Upload as usize] = side,
                    Leaving::Save => settings.fits[Leaves::Save as usize] = side,
                    Leaving::SaveOnCapture => settings.as_taken_fit = side,
                });
            },
        )
        .least_width(px(0.0));
        let format_said = {
            let language = language.clone();
            move |format: Format| -> SharedString {
                language
                    .text(&format!("main.output.format.{}", format.name()), &[])
                    .into()
            }
        };
        let controls = div().flex_none().flex().gap(px(8.0));
        let controls = match self {
            Leaving::Copy | Leaving::Upload => {
                let leaves = if self == Leaving::Copy {
                    Leaves::Copy
                } else {
                    Leaves::Upload
                };
                controls.child(
                    choice(
                        if self == Leaving::Copy {
                            "copyFormat"
                        } else {
                            "uploadFormat"
                        },
                        Format::ALL.to_vec(),
                        settings.formats[leaves as usize],
                        format_said,
                        move |format, cx| {
                            system::set(cx, |settings| settings.formats[leaves as usize] = format)
                        },
                    )
                    .least_width(px(0.0)),
                )
            }
            // Saved as several kinds at once, one file of each: ticked. The last one stays ticked.
            Leaving::Save | Leaving::SaveOnCapture => {
                let ticked = if self == Leaving::Save {
                    &settings.save_formats
                } else {
                    &settings.as_taken_formats
                };
                controls.children(Format::ALL.map(|format| {
                    Check::new(
                        SharedString::from(format!("{name}-{}", format.name())),
                        format_said(format),
                        ticked.contains(&format),
                    )
                    .on_toggle(move |_, cx| {
                        system::set(cx, |settings| {
                            let now = if self == Leaving::Save {
                                &settings.save_formats
                            } else {
                                &settings.as_taken_formats
                            };
                            let next: Vec<Format> = Format::ALL
                                .into_iter()
                                .filter(|one| now.contains(one) != (*one == format))
                                .collect();
                            if next.is_empty() {
                                return;
                            }
                            if self == Leaving::Save {
                                settings.save_as(&next);
                            } else {
                                settings.save_as_taken_as(&next);
                            }
                        })
                    })
                }))
            }
        };
        setting(
            t(&format!("main.output.{name}")),
            t(&format!("main.output.{name}Hint")),
            controls.child(fits),
            under,
            theme,
        )
        .into_any_element()
    }
}

/// The corner of the screen something appears in.
fn corner_choice(
    id: &'static str,
    current: Corner,
    language: &Language,
    set: fn(Corner, &mut App),
) -> Select {
    let language = language.clone();
    choice(
        id,
        Corner::ALL.to_vec(),
        current,
        move |corner| {
            language
                .text(&format!("main.general.corner.{}", corner.name()), &[])
                .into()
        },
        set,
    )
}

/// The sound something comes with (a notice, a capture): none, one of the system's, or a file of
/// the user's own, which is chosen under it.
fn sound_setting(
    of: Occasion,
    settings: &Settings,
    language: &Language,
    theme: &Theme,
) -> Vec<AnyElement> {
    let t = said(language);
    let (id, title, hint, sound, file) = match of {
        Occasion::Notice => (
            "noticeSound",
            "main.general.noticeSound",
            "main.general.noticeSoundHint",
            &settings.notice_sound,
            &settings.notice_sound_file,
        ),
        Occasion::Capture => (
            "captureSound",
            "main.capture.sound",
            "main.capture.soundHint",
            &settings.capture_sound,
            &settings.capture_sound_file,
        ),
    };
    let system_sounds = perora_core::platform::system_sounds();
    let mut values = vec![Sound::None, Sound::Custom];
    let mut items = vec![
        Item::Choice(t("main.general.sound.none")),
        Item::Choice(t("main.general.sound.custom")),
        Item::Group(t("main.general.sound.system")),
    ];
    for name in &system_sounds {
        // A sound of the system's is called what its file is, without the "Windows" they all begin
        // with.
        let called = name.strip_prefix("Windows ").unwrap_or(name);
        items.push(Item::Choice(called.to_owned().into()));
        values.push(Sound::System(name.clone()));
    }
    // (A sound of the system's that this system does not have is shown as none, as it is heard.)
    let shown = sound.shown();
    let chosen = values.iter().position(|value| value.name() == shown);
    let custom = matches!(sound, Sound::Custom);
    let select = Select::new(id, items, chosen).on_change(move |index, _, cx| {
        if let Some(sound) = values.get(index) {
            system::set_sound(of, sound.clone(), cx);
        }
    });
    let path: SharedString = file
        .as_ref()
        .map(|file| file.display().to_string())
        .unwrap_or_else(|| language.text("main.general.noticeSoundFileNone", &[]))
        .into();
    vec![
        setting(t(title), t(hint), select, false, theme).into_any_element(),
        Reveal::new(SharedString::from(format!("{id}-file")), custom)
            .child(card(
                div()
                    .flex()
                    .flex_col()
                    .child(t("main.general.noticeSoundFile"))
                    .child(
                        hint_text(path, theme)
                            .mt(px(2.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis_start(),
                    ),
                Some(
                    Button::new(
                        SharedString::from(format!("{id}-choose")),
                        t("main.general.noticeSoundChoose"),
                    )
                    .on_click(move |_, cx| system::choose_sound(of, cx))
                    .into_any_element(),
                ),
                true,
                theme,
            ))
            .into_any_element(),
    ]
}

pub fn capture(
    this: &MainWindow,
    settings: &Settings,
    language: &Language,
    theme: &Theme,
    _: &mut Window,
    cx: &mut Context<MainWindow>,
) -> Vec<AnyElement> {
    let _ = cx;
    let t = said(language);
    // Uploading is an extra, to be switched on: which takes a sign-in. What goes with it is shown
    // under it, while it is on. (No sign-in yet in this version: P5 adds it later.)
    let signed_in = false;
    let uploads = signed_in && settings.upload_enabled;
    // Captures are copied: as they are shown, or straight after being taken (then with no preview,
    // and nothing to discard).
    let only_copied = settings.after_capture == After::Copy;
    let copies = settings.copy_to_clipboard || only_copied;
    let modes = {
        let language = language.clone();
        move |mode: Mode| -> SharedString {
            language
                .text(&format!("overlay.mode.{}", mode.name()), &[])
                .into()
        }
    };
    let afters = {
        let language = language.clone();
        move |after: After| -> SharedString {
            language
                .text(&format!("main.capture.after.{}", after.name()), &[])
                .into()
        }
    };
    let qualities = {
        let language = language.clone();
        move |quality: Quality| -> SharedString {
            language
                .text(&format!("main.output.quality.{}", quality.name()), &[])
                .into()
        }
    };

    let mut page = vec![
        section(t("main.capture.section.general"), true, theme).into_any_element(),
        setting(
            t("main.capture.startMode"),
            t("main.capture.startModeHint"),
            choice(
                "startMode",
                Mode::ALL.to_vec(),
                settings.start_mode,
                modes,
                |mode, cx| system::set(cx, |settings| settings.start_mode = mode),
            ),
            false,
            theme,
        )
        .into_any_element(),
        setting(
            t("main.capture.hideOwnWindows"),
            t("main.capture.hideOwnWindowsHint"),
            switch("hideOwnWindows", settings.hide_own_windows, |settings| {
                settings.hide_own_windows = !settings.hide_own_windows
            }),
            false,
            theme,
        )
        .into_any_element(),
        setting(
            t("main.capture.copy"),
            t("main.capture.copyHint"),
            switch("copyToClipboard", settings.copy_to_clipboard, |settings| {
                settings.copy_to_clipboard = !settings.copy_to_clipboard
            })
            .enabled(!only_copied),
            false,
            theme,
        )
        .into_any_element(),
        // What goes with copying, under it: shown while captures are copied (owner, 2026-10-14).
        Reveal::new("copies-more", copies)
            .child(Leaving::Copy.row(settings, language, theme, true))
            .child(setting(
                t("main.capture.clear"),
                t("main.capture.clearHint"),
                switch(
                    "clearClipboardOnDiscard",
                    settings.clear_clipboard_on_discard,
                    |settings| {
                        settings.clear_clipboard_on_discard = !settings.clear_clipboard_on_discard
                    },
                )
                .enabled(!only_copied),
                true,
                theme,
            ))
            .into_any_element(),
        section(t("main.capture.section.screen"), false, theme).into_any_element(),
        setting(
            t("main.capture.barHideSeconds"),
            t("main.capture.barHideSecondsHint"),
            this.seconds.bar_hide.clone(),
            false,
            theme,
        )
        .into_any_element(),
        Reveal::new("barHideSeconds-more", settings.bar_hide_seconds > 0.0)
            .child(setting(
                t("main.capture.barRevealArea"),
                t("main.capture.barRevealAreaHint"),
                reveal_area::preview(settings.bar_reveal_area, language, theme),
                true,
                theme,
            ))
            .into_any_element(),
        setting(
            t("main.capture.showCrosshair"),
            t("main.capture.showCrosshairHint"),
            switch("showCrosshair", settings.show_crosshair, |settings| {
                settings.show_crosshair = !settings.show_crosshair
            }),
            false,
            theme,
        )
        .into_any_element(),
        section(t("main.capture.section.after"), false, theme).into_any_element(),
        setting(
            t("main.capture.after"),
            t("main.capture.afterHint"),
            choice(
                "afterCapture",
                After::ALL.to_vec(),
                settings.after_capture,
                afters,
                |after, cx| system::set(cx, |settings| settings.after_capture = after),
            ),
            false,
            theme,
        )
        .into_any_element(),
        Reveal::new(
            "afterCapture-more",
            settings.after_capture == After::Preview,
        )
        .child(setting(
            t("main.capture.previewSeconds"),
            t("main.capture.previewSecondsHint"),
            this.seconds.preview.clone(),
            true,
            theme,
        ))
        .child(setting(
            t("main.capture.previewCorner"),
            t("main.capture.previewCornerHint"),
            corner_choice(
                "previewCorner",
                settings.preview_corner,
                language,
                system::set_preview_corner,
            ),
            true,
            theme,
        ))
        .into_any_element(),
    ];
    page.extend(sound_setting(Occasion::Capture, settings, language, theme));
    page.extend([
        section(t("main.capture.section.other"), false, theme).into_any_element(),
        setting(
            t("main.output.quality"),
            t("main.output.qualityHint"),
            choice(
                "quality",
                Quality::ALL.to_vec(),
                settings.quality,
                qualities,
                |quality, cx| system::set(cx, |settings| settings.quality = quality),
            ),
            false,
            theme,
        )
        .into_any_element(),
        setting(
            t("main.capture.enableUpload"),
            t(if signed_in {
                "main.capture.enableUploadHint"
            } else {
                "main.capture.enableUploadSignedOut"
            }),
            switch("enableUpload", uploads, |settings| {
                settings.upload_enabled = !settings.upload_enabled
            })
            .enabled(signed_in),
            false,
            theme,
        )
        .into_any_element(),
        Reveal::new("enableUpload-more", uploads)
            .child(setting(
                t("main.capture.uploadImmediately"),
                t("main.capture.uploadImmediatelyHint"),
                switch(
                    "uploadImmediately",
                    settings.upload_immediately,
                    |settings| settings.upload_immediately = !settings.upload_immediately,
                ),
                true,
                theme,
            ))
            .child(Leaving::Upload.row(settings, language, theme, true))
            .into_any_element(),
    ]);
    page
}

pub fn save(
    settings: &Settings,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> Vec<AnyElement> {
    let _ = cx;
    let t = said(language);
    let folder: SharedString = perora_core::saving::save_directory(settings)
        .map(|folder| folder.display().to_string())
        .unwrap_or_default()
        .into();
    let is_default = settings.save_directory.is_none();
    let custom = settings.as_taken_custom;
    let as_said = {
        let language = language.clone();
        move |custom: bool| -> SharedString {
            language
                .text(
                    if custom {
                        "main.save.onCaptureAs.custom"
                    } else {
                        "main.save.onCaptureAs.save"
                    },
                    &[],
                )
                .into()
        }
    };
    vec![
        section(t("main.save.section.place"), true, theme).into_any_element(),
        card(
            div().flex().flex_col().child(t("main.save.folder")).child(
                hint_text(folder, theme)
                    .mt(px(2.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis_start(),
            ),
            Some(
                div()
                    .flex_none()
                    .flex()
                    .gap(px(8.0))
                    .when(!is_default, |buttons| {
                        buttons.child(
                            Button::new("saveReset", t("main.save.reset"))
                                .on_click(|_, cx| system::choose_save_directory(true, cx)),
                        )
                    })
                    .child(
                        Button::new("saveChoose", t("main.save.choose"))
                            .on_click(|_, cx| system::choose_save_directory(false, cx)),
                    )
                    .into_any_element(),
            ),
            false,
            theme,
        )
        .into_any_element(),
        setting(
            t("main.save.ask"),
            t("main.save.askHint"),
            switch("askWhereToSave", settings.ask_where_to_save, |settings| {
                settings.ask_where_to_save = !settings.ask_where_to_save
            }),
            false,
            theme,
        )
        .into_any_element(),
        section(t("main.save.section.detail"), false, theme).into_any_element(),
        Leaving::Save.row(settings, language, theme, false),
        section(t("main.save.section.onCapture"), false, theme).into_any_element(),
        setting(
            t("main.save.onCapture"),
            t("main.save.onCaptureHint"),
            switch("saveOnCapture", settings.save_on_capture, |settings| {
                settings.save_on_capture = !settings.save_on_capture
            }),
            false,
            theme,
        )
        .into_any_element(),
        // What goes with it, under it, while it is on: what the files are (as saving has them, or
        // their own, chosen under that), and whether discarding takes them away (owner,
        // 2026-10-16).
        Reveal::new("saveOnCapture-more", settings.save_on_capture)
            .child(setting(
                t("main.save.onCaptureAs"),
                t("main.save.onCaptureAsHint"),
                choice(
                    "saveOnCaptureCustom",
                    vec![false, true],
                    custom,
                    as_said,
                    |custom, cx| system::set(cx, |settings| settings.as_taken_custom = custom),
                ),
                true,
                theme,
            ))
            .child(
                Reveal::new("saveOnCaptureCustom-more", custom)
                    .child(Leaving::SaveOnCapture.row(settings, language, theme, true)),
            )
            .child(setting(
                t("main.save.deleteOnDiscard"),
                t("main.save.deleteOnDiscardHint"),
                switch(
                    "deleteSavedOnDiscard",
                    settings.delete_saved_on_discard,
                    |settings| settings.delete_saved_on_discard = !settings.delete_saved_on_discard,
                ),
                true,
                theme,
            ))
            .into_any_element(),
    ]
}

/// How the editor is, where a capture is drawn on (owner, 2026-10-19: a page of its own).
pub fn editor(settings: &Settings, language: &Language, theme: &Theme) -> Vec<AnyElement> {
    let t = said(language);
    let hiders = {
        let language = language.clone();
        move |hider: Hider| -> SharedString {
            language
                .text(&format!("main.editor.defaultHider.{}", hider.name()), &[])
                .into()
        }
    };
    vec![
        section(t("main.editor.section.tools"), true, theme).into_any_element(),
        setting(
            t("main.editor.defaultHider"),
            t("main.editor.defaultHiderHint"),
            choice(
                "defaultHider",
                Hider::ALL.to_vec(),
                settings.default_hider,
                hiders,
                |hider, cx| system::set(cx, |settings| settings.default_hider = hider),
            ),
            false,
            theme,
        )
        .into_any_element(),
        section(t("main.editor.section.open"), false, theme).into_any_element(),
        setting(
            t("main.editor.openFromExplorer"),
            t("main.editor.openFromExplorerHint"),
            Switch::new("openFromExplorer", settings.open_from_explorer).on_toggle(|_, cx| {
                let on = !Perora::get(cx).settings().open_from_explorer;
                system::open_from_explorer(on, cx);
            }),
            false,
            theme,
        )
        .into_any_element(),
    ]
}

pub fn general(
    this: &MainWindow,
    settings: &Settings,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> Vec<AnyElement> {
    let t = said(language);
    let perora = Perora::get(cx);
    let languages: Vec<(String, SharedString)> = perora
        .languages
        .all()
        .iter()
        .map(|language| (language.code().to_owned(), language.name().into()))
        .collect();
    let current = languages
        .iter()
        .position(|(code, _)| code == language.code());
    let codes: Vec<String> = languages.iter().map(|(code, _)| code.clone()).collect();
    let looks = {
        let language = language.clone();
        move |look: Look| -> SharedString {
            language
                .text(&format!("main.general.theme.{}", look.name()), &[])
                .into()
        }
    };
    // (Whether Perora starts at sign-in is the system's to say, not the settings'.)
    let started = system::starts_at_sign_in();
    let mut page = vec![
        section(t("main.general.section.ui"), true, theme).into_any_element(),
        setting(
            t("main.general.language"),
            t("main.general.languageHint"),
            Select::of(
                "language",
                languages.into_iter().map(|(_, name)| name),
                current,
            )
            .on_change(move |index, _, cx| {
                if let Some(code) = codes.get(index) {
                    system::set_language(code, cx);
                }
            }),
            false,
            theme,
        )
        .into_any_element(),
        setting(
            t("main.general.theme"),
            t("main.general.themeHint"),
            choice(
                "theme",
                Look::ALL.to_vec(),
                settings.look,
                looks,
                system::set_look,
            ),
            false,
            theme,
        )
        .into_any_element(),
        section(t("main.general.section.notice"), false, theme).into_any_element(),
        setting(
            t("main.general.noticeSeconds"),
            t("main.general.noticeSecondsHint"),
            this.seconds.notice.clone(),
            false,
            theme,
        )
        .into_any_element(),
        setting(
            t("main.general.noticeCorner"),
            t("main.general.noticeCornerHint"),
            corner_choice(
                "noticeCorner",
                settings.notice_corner,
                language,
                system::set_notice_corner,
            ),
            false,
            theme,
        )
        .into_any_element(),
    ];
    page.extend(sound_setting(Occasion::Notice, settings, language, theme));
    page.extend([
        section(t("main.general.section.other"), false, theme).into_any_element(),
        setting(
            t("main.general.startAtSignIn"),
            t("main.general.startAtSignInHint"),
            // Switched on, it starts in the tray, until the user would rather see the window.
            Switch::new("startAtSignIn", started.is_some()).on_toggle(|_, cx| {
                let on = system::starts_at_sign_in().is_none();
                system::start_at_sign_in(on.then_some(true), cx);
            }),
            false,
            theme,
        )
        .into_any_element(),
        Reveal::new("startAtSignIn-more", started.is_some())
            .child(setting(
                t("main.general.startMinimized"),
                t("main.general.startMinimizedHint"),
                Switch::new("startMinimized", started.unwrap_or(true)).on_toggle(|_, cx| {
                    // (How it starts is only to be said while it does.)
                    if let Some(quietly) = system::starts_at_sign_in() {
                        system::start_at_sign_in(Some(!quietly), cx);
                    }
                }),
                true,
                theme,
            ))
            .into_any_element(),
        // (The version and its updates go here: P7, the updater.)
    ]);
    page
}

pub fn account(language: &Language, theme: &Theme) -> Vec<AnyElement> {
    let t = said(language);
    // Signed out only, for now: signing in (and what is shown while the browser is awaited, and
    // once signed in: who, the recent uploads) comes later in P5. The button is there, and does
    // nothing yet.
    vec![
        card(
            about(
                t("main.account.signedOut"),
                Some(t("main.account.signedOutHint")),
                theme,
            ),
            Some(
                Button::new("signIn", t("main.account.signIn"))
                    .accent()
                    .enabled(false)
                    .into_any_element(),
            ),
            false,
            theme,
        )
        .into_any_element(),
    ]
}
