//! Perora's colours, sizes and type: the values of the Tauri version's `style.css`, so that the GPUI
//! version looks the same. Dark is the base; light follows the system or the setting.

use gpui::{App, Global, Hsla, Pixels, Rgba, SharedString, WindowAppearance, px, rgb};
use perora_core::settings::Look;

/// Which of the two looks is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    /// The look the setting asks for, or the system's when it asks for none.
    pub fn of(look: Look, system: WindowAppearance) -> Self {
        match look {
            Look::Light => Mode::Light,
            Look::Dark => Mode::Dark,
            Look::System => match system {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Mode::Light,
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Mode::Dark,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub mode: Mode,
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    /// The system's accent colour: as it is on light, lightened on dark (as Windows does).
    pub accent: Hsla,
    /// Text on the accent colour.
    pub accent_text: Hsla,
    pub good: Hsla,
    pub bad: Hsla,
    pub warning: Hsla,
    pub radius_small: Pixels,
    pub radius: Pixels,
    pub font: SharedString,
    /// The size of body text in the main window; the preview windows use `text_size_small`.
    pub text_size: Pixels,
    pub text_size_small: Pixels,
}

impl Global for Theme {}

/// Windows' own blue, when the system's accent colour cannot be read.
const DEFAULT_ACCENT: (u8, u8, u8) = (0x00, 0x78, 0xd4);

impl Theme {
    pub fn new(mode: Mode, system_accent: Option<(u8, u8, u8)>) -> Self {
        let (r, g, b) = system_accent.unwrap_or(DEFAULT_ACCENT);
        let colour = |hex: u32| -> Hsla { rgb(hex).into() };
        let accent: Hsla = match mode {
            Mode::Light => colour(hex(r, g, b)),
            // `color-mix(in srgb, accent 55%, white)` in style.css.
            Mode::Dark => colour(hex(mix(r), mix(g), mix(b))),
        };
        let (background, surface, surface_hover, border, text, muted) = match mode {
            Mode::Dark => (0x202020, 0x2b2b2b, 0x323232, 0x3a3a3a, 0xffffff, 0xc5c5c5),
            Mode::Light => (0xf3f3f3, 0xfbfbfb, 0xf6f6f6, 0xe5e5e5, 0x1b1b1b, 0x5f5f5f),
        };
        let (accent_text, good, bad, warning) = match mode {
            Mode::Dark => (0x000000, 0x6ccb5f, 0xff99a4, 0xfce100),
            Mode::Light => (0xffffff, 0x0f7b0f, 0xc42b1c, 0x9d5d00),
        };
        Self {
            mode,
            background: colour(background),
            surface: colour(surface),
            surface_hover: colour(surface_hover),
            border: colour(border),
            text: colour(text),
            muted: colour(muted),
            accent,
            accent_text: colour(accent_text),
            good: colour(good),
            bad: colour(bad),
            warning: colour(warning),
            radius_small: px(4.0),
            radius: px(8.0),
            // The first of style.css's stack that GPUI finds: Segoe UI Variable is Windows 11's.
            font: "Segoe UI Variable Text".into(),
            text_size: px(14.0),
            text_size_small: px(13.0),
        }
    }

    pub fn get(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }
}

fn mix(channel: u8) -> u8 {
    (channel as f32 * 0.55 + 255.0 * 0.45).round() as u8
}

fn hex(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// For colours given as a whole `0xRRGGBBAA`.
pub fn rgba(hex: u32) -> Rgba {
    gpui::rgba(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accent_is_lightened_on_dark_as_in_the_style_sheet() {
        // 55 % of Windows' blue with 45 % white.
        assert_eq!((mix(0x00), mix(0x78), mix(0xd4)), (115, 181, 231));
        let dark = Theme::new(Mode::Dark, None);
        let light = Theme::new(Mode::Light, None);
        assert_ne!(dark.accent, light.accent);
        assert_eq!(light.accent, Hsla::from(rgb(0x0078d4)));
    }

    #[test]
    fn the_setting_wins_over_the_system() {
        assert_eq!(Mode::of(Look::Dark, WindowAppearance::Light), Mode::Dark);
        assert_eq!(Mode::of(Look::Light, WindowAppearance::Dark), Mode::Light);
        assert_eq!(Mode::of(Look::System, WindowAppearance::Dark), Mode::Dark);
        assert_eq!(
            Mode::of(Look::System, WindowAppearance::VibrantLight),
            Mode::Light
        );
    }
}
