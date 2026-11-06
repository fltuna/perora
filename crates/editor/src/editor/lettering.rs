//! The settings of what is written, in a box by it (owner: 「テキストは入力したボックス付近にBox出して
//! フォントやサイズをpx単位で入れられるようにしたい。色もそこだね」): the font, the size of the letters
//! in pixels of the image (it takes at each digit: 「行ったら即座に反映って無理？」), bold, slanted,
//! struck through, the inside of the letters and the line around them. Over what is written, so that
//! it stays where it is while that grows; under it where there is no room over it, beside it where
//! there is none either way; never out of the window. What it opens (the fonts, the inside, the line
//! around) opens away from what is written, as the panels do (`panels`).
//!
//! As editor.tsx's `Lettering` and `setLettering`.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnchoredPositionMode, AnyElement, Bounds, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, Pixels, SharedString, StatefulInteractiveElement, Styled, Window, anchored,
    deferred, div, point, px,
};
use perora_core::edit::{Colour, Edit};
use perora_core::editor_settings::EditorSettings;
use perora_ui::swatch::swatch;
use perora_ui::{TextField, TextFieldEvent, Theme};

use super::bars::{Panel, recorded, shadow};
use super::{Editor, tip};
use crate::icons;
use crate::paint;
use crate::writing::{self, SIZES, Writing, first_edge, how_of, reach_of, written};

/// How wide the box is, and how far from what is written.
const WIDE: f32 = 372.0;
const AWAY: f32 = 14.0;
const EDGE: f32 = 8.0;

/// A setting of what is written, as the box changes it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum LetterSet {
    Font(String),
    Size(f32),
    Bold(bool),
    Italic(bool),
    Strike(bool),
    /// The colour of the inside of the letters (given an inside, where they had none).
    Colour(Colour),
    /// How solid the inside is (0: none).
    Fill(f32),
    /// The line around the letters (none: no line).
    Outline(Option<Colour>),
    /// How far the line reaches out of them.
    Edge(f32),
}

impl LetterSet {
    fn name(&self) -> &'static str {
        match self {
            LetterSet::Font(_) => "font",
            LetterSet::Size(_) => "size",
            LetterSet::Bold(_) => "bold",
            LetterSet::Italic(_) => "italic",
            LetterSet::Strike(_) => "strike",
            LetterSet::Colour(_) => "color",
            LetterSet::Fill(_) => "fill",
            LetterSet::Outline(_) => "outline",
            LetterSet::Edge(_) => "edge",
        }
    }
}

/// The parts of the box that are kept between drawings: the field of the size, and the fonts.
pub(super) struct Lettering {
    pub size: gpui::Entity<TextField>,
    pub sought: gpui::Entity<TextField>,
    /// The fonts the system has (asked for when the list is first opened), and which of those shown
    /// the keys are at.
    pub fonts: Option<Rc<Vec<String>>>,
    pub at: usize,
    pub scroll: gpui::UniformListScrollHandle,
    /// The size the field was last made to show.
    pub shown: String,
}

impl Lettering {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        let size = cx.new(|cx| TextField::new(cx).longest(7).to_the_right());
        cx.subscribe(&size, |this: &mut Editor, _, event: &TextFieldEvent, cx| {
            let TextFieldEvent::Changed(text) = event;
            this.size_written(text, cx);
        })
        .detach();
        let sought = cx.new(|cx| TextField::new(cx).longest(64));
        cx.subscribe(&sought, |this: &mut Editor, _, _: &TextFieldEvent, cx| {
            this.lettering.at = 0;
            cx.notify();
        })
        .detach();
        Self {
            size,
            sought,
            fonts: None,
            at: 0,
            scroll: gpui::UniformListScrollHandle::new(),
            shown: String::new(),
        }
    }
}

use gpui::AppContext;

impl Editor {
    /// How what the box is about is written: what is selected of what was written, or what is being
    /// written (and what is written next).
    pub(super) fn letters_now(&self) -> Writing {
        match self.chosen_edit() {
            Some(edit @ Edit::Text { .. }) if self.typing.is_none() => {
                how_of(edit).unwrap_or_else(|| self.letters.clone())
            }
            _ => self.letters.clone(),
        }
    }

    /// A setting of what is written: for what is being written (and written next), or what is
    /// selected of what was written (written anew so).
    pub(super) fn set_lettering(&mut self, set: LetterSet, dragged: bool, cx: &mut Context<Self>) {
        if self.frozen {
            return;
        }
        let now = self.letters_now();
        let mut next = now.clone();
        match &set {
            LetterSet::Font(font) => next.font = writing::family(font),
            LetterSet::Size(size) => next.size = *size,
            LetterSet::Bold(on) => next.bold = *on,
            LetterSet::Italic(on) => next.italic = *on,
            LetterSet::Strike(on) => next.strike = *on,
            LetterSet::Colour(colour) => {
                next.color = *colour;
                if next.fill == 0.0 {
                    next.fill = 1.0;
                }
            }
            LetterSet::Fill(fill) => next.fill = *fill,
            LetterSet::Outline(outline) => next.outline = *outline,
            LetterSet::Edge(edge) => next.edge = *edge,
        }
        // A line around the letters has a width from the start: a twelfth of their size.
        if next.outline.is_some() && next.edge <= 0.0 {
            next.edge = first_edge(next.size);
        }
        // Letters are never without both: with no inside they have a line around them (in the
        // colour their inside had), and with no line around them an inside.
        if next.fill == 0.0 && next.outline.is_none() {
            if set == LetterSet::Outline(None) {
                next.fill = 1.0;
            } else {
                next.outline = Some(next.color);
                if next.edge <= 0.0 {
                    next.edge = first_edge(next.size);
                }
            }
        }
        // The letters stay where they are: with more or less room around them for the line, the
        // corner of what is written is that much further out or in.
        let nearer = reach_of(&now) - reach_of(&next);
        let about = if self.typing.is_some() {
            None
        } else {
            self.about()
        };
        let anew = match about.and_then(|index| self.drawing.edits().get(index).cloned()) {
            Some(Edit::Text { from, text, .. }) => {
                match written([from[0] + nearer, from[1] + nearer], &text, &next) {
                    Some(anew) => Some((about.unwrap_or(0), anew)),
                    None => {
                        // (Too large to be written so: nothing changes, nor does what is written
                        // next.)
                        self.full = true;
                        cx.notify();
                        return;
                    }
                }
            }
            _ => None,
        };
        self.letters = next.clone();
        if let Some(typing) = self.typing.as_mut()
            && nearer != 0.0
        {
            typing.at = [typing.at[0] + nearer, typing.at[1] + nearer];
        }
        if let LetterSet::Font(_) = set {
            let font = next.font.clone();
            EditorSettings::update(|kept| kept.font = font);
        }
        if let Some((index, anew)) = anew {
            let mut edits = self.drawing.edits().to_vec();
            if edits.get(index) != Some(&anew) {
                edits[index] = anew;
                if dragged {
                    self.change_dragged(edits, &format!("{index}:{}", set.name()), cx);
                } else {
                    self.change(edits, cx);
                }
            }
        }
        cx.notify();
    }

    /// The size was typed: taken at once, when it is one (4 to 2000, to a tenth).
    fn size_written(&mut self, text: &str, cx: &mut Context<Self>) {
        let Ok(asked) = text.trim().parse::<f32>() else {
            return;
        };
        let size = (asked * 10.0).round() / 10.0;
        if !(SIZES.0..=SIZES.1).contains(&asked)
            || size == (self.letters_now().size * 10.0).round() / 10.0
        {
            return;
        }
        self.set_lettering(LetterSet::Size(size), true, cx);
    }

    /// Where what the box is about is, on the screen: what is written without the line around it
    /// (so that the box stays where it is while that line comes and goes). Not while anything is
    /// moved or pulled, nor when it is out of sight.
    fn lettered_at(&self, window: &Window) -> Option<Bounds<Pixels>> {
        if self.frozen || self.gesture.is_some() {
            return None;
        }
        let (area, how) = match (&self.typing, self.chosen_edit()) {
            (Some(_), _) => (self.typing_room()?, self.letters.clone()),
            (None, Some(edit @ Edit::Text { .. })) => (crate::marks::box_of(edit), how_of(edit)?),
            _ => return None,
        };
        let (stage, look) = self.look(window)?;
        let view = self.view()?;
        let part = look.part;
        // (Out of sight: looked at so closely that it is beside what is shown, or past the picture.)
        let seen = area.x < part.right().min(view.right())
            && area.right() > part.x.max(view.x)
            && area.y < part.bottom().min(view.bottom())
            && area.bottom() > part.y.max(view.y);
        if !seen {
            return None;
        }
        let reach = reach_of(&how);
        let inner = crate::view::Area::new(
            area.x + reach,
            area.y + reach,
            (area.width - reach * 2.0).max(0.0),
            (area.height - reach * 2.0).max(0.0),
        );
        let painter = paint::Painter {
            origin: stage.origin,
            offset: [part.x, part.y],
            scale: look.scale,
        };
        Some(painter.bounds(inner))
    }

    /// Whether the box is over what is written (or beside it): what it opens, opens upwards then.
    pub(super) fn lettering_up(&self) -> bool {
        self.lettering_side != Side::Under
    }

    /// The box, by what is written.
    pub(super) fn lettering_box(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(text) = self.lettered_at(window) else {
            self.lettering_shown = false;
            return None;
        };
        self.lettering_shown = true;
        let now = self.letters_now();
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        // The size as it is: anew when it changes (unless what is typed comes to it already: a
        // second digit after the point is not typed over), and once the field is left.
        let rounded = format!("{}", (now.size * 10.0).round() / 10.0);
        let typed = self.lettering.size.read(cx).text().to_string();
        let focused =
            gpui::Focusable::focus_handle(self.lettering.size.read(cx), cx).is_focused(window);
        let comes_to = |text: &str| {
            text.trim().parse::<f32>().is_ok_and(|typed| {
                (typed * 10.0).round() / 10.0 == (now.size * 10.0).round() / 10.0
            })
        };
        let anew = rounded != self.lettering.shown;
        if (anew && !comes_to(&typed)) || (!focused && typed != rounded) {
            self.lettering
                .size
                .update(cx, |field, cx| field.set_text(&rounded, cx));
        }
        self.lettering.shown = rounded;
        // Where it goes: over what is written, under it, or beside it; in the window.
        let tall = self
            .openers
            .borrow()
            .get(&Panel::Fonts)
            .map_or(80.0, |bounds| bounds.size.height.as_f32());
        let viewport = window.viewport_size();
        let (width, height) = (viewport.width.as_f32(), viewport.height.as_f32());
        let within = |value: f32, most: f32| value.min(most).max(EDGE).round();
        let (left, top) = (text.origin.x.as_f32(), text.origin.y.as_f32());
        let (right, bottom) = (text.right().as_f32(), text.bottom().as_f32());
        let over = top - tall - AWAY;
        let under = bottom + AWAY;
        let across = within(left, width - WIDE - EDGE);
        let (at, side) = if over >= EDGE {
            ((across, within(over, height - tall - EDGE)), Side::Over)
        } else if under + tall <= height - EDGE {
            ((across, within(under, height - tall - EDGE)), Side::Under)
        } else {
            let (after, before) = (right + AWAY, left - WIDE - AWAY);
            let x = if after + WIDE <= width - EDGE || before < EDGE {
                after
            } else {
                before
            };
            (
                (
                    within(x, width - WIDE - EDGE),
                    within(top, height - tall - EDGE),
                ),
                Side::Beside,
            )
        };
        self.lettering_side = side;
        let open = self.panel();
        let toggle = |label: &'static str, on: bool, theme: &Theme| {
            div()
                .id(label)
                .w(px(30.0))
                .h(px(30.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius_small)
                .border_1()
                .text_size(px(14.0))
                .map(|button| {
                    if on {
                        let mut tint = theme.accent;
                        tint.a = 0.16;
                        let mut edge = theme.accent;
                        edge.a = 0.45;
                        button.text_color(theme.accent).bg(tint).border_color(edge)
                    } else {
                        button
                            .border_color(gpui::transparent_black())
                            .hover(|style| style.bg(theme.surface_hover))
                    }
                })
        };
        let font_name: SharedString = if now.font.is_empty() {
            t("editor.font.own")
        } else {
            now.font.clone().into()
        };
        let font_family: SharedString = if now.font.is_empty() {
            theme.font.clone()
        } else {
            now.font.clone().into()
        };
        let fill_swatch = (now.fill > 0.0).then(|| {
            let mut colour = paint::colour(now.color);
            colour.a = now.fill;
            colour
        });
        let colour_button = |id: &'static str, name: SharedString, panel: Panel, theme: &Theme| {
            let opened = open == Some(panel);
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(30.0))
                .px(px(8.0))
                .rounded(theme.radius_small)
                .when(opened, |button| button.bg(theme.surface_hover))
                .hover(|style| style.bg(theme.surface_hover))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.muted)
                        .child(name),
                )
        };
        let ring = {
            let colour = now.outline.map(paint::colour);
            div()
                .size(px(18.0))
                .rounded_full()
                .map(|ring| match colour {
                    Some(colour) => ring.border_4().border_color(colour),
                    None => ring,
                })
                .when(colour.is_none(), |ring| {
                    ring.child(swatch(None, 18.0, false, theme))
                })
        };
        let content = div()
            .id("lettering")
            .occlude()
            .w(px(WIDE))
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .text_size(px(13.0))
            .text_color(theme.text)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow(shadow(4.0, 14.0, 0x0000_004d))
            .cursor_default()
            .child(recorded(&self.openers, Panel::Fonts))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .id("font")
                            .flex_1()
                            .min_w_0()
                            .h(px(30.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(theme.radius_small)
                            .bg(theme.background)
                            .border_1()
                            .border_color(if open == Some(Panel::Fonts) {
                                theme.accent
                            } else {
                                theme.border
                            })
                            .tooltip(tip(t("editor.font")))
                            .on_click(cx.listener(|this, _, _, cx| this.open(Panel::Fonts, cx)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .font_family(font_family)
                                    .child(font_name),
                            )
                            .child(icons::caret(theme.text)),
                    )
                    .child(
                        div()
                            .id("size")
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_color(theme.muted)
                            .tooltip(tip(t("editor.size")))
                            .child(div().w(px(68.0)).child(self.lettering.size.clone()))
                            .child("px"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        toggle("bold", now.bold, theme)
                            .font_weight(FontWeight::BOLD)
                            .tooltip(tip(t("editor.bold")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let on = this.letters_now().bold;
                                this.set_lettering(LetterSet::Bold(!on), false, cx)
                            }))
                            .child("B"),
                    )
                    .child(
                        toggle("italic", now.italic, theme)
                            .font_family("Georgia")
                            .italic()
                            .tooltip(tip(t("editor.italic")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let on = this.letters_now().italic;
                                this.set_lettering(LetterSet::Italic(!on), false, cx)
                            }))
                            .child("I"),
                    )
                    .child(
                        toggle("strike", now.strike, theme)
                            .line_through()
                            .tooltip(tip(t("editor.strike")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let on = this.letters_now().strike;
                                this.set_lettering(LetterSet::Strike(!on), false, cx)
                            }))
                            .child("S"),
                    )
                    .child(div().flex_1())
                    .child(
                        colour_button("fill", t("editor.fill"), Panel::LetterFill, theme)
                            .tooltip(tip(t("editor.fill")))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.open(Panel::LetterFill, cx)),
                            )
                            .child(swatch(fill_swatch, 18.0, false, theme))
                            .child(icons::caret(theme.text)),
                    )
                    .child(
                        colour_button(
                            "outline",
                            t("editor.text.outline"),
                            Panel::LetterOutline,
                            theme,
                        )
                        .tooltip(tip(t("editor.text.outline")))
                        .on_click(cx.listener(|this, _, _, cx| this.open(Panel::LetterOutline, cx)))
                        .child(ring)
                        .child(icons::caret(theme.text)),
                    ),
            );
        Some(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .position(point(px(at.0), px(at.1)))
                    .snap_to_window_with_margin(px(EDGE))
                    .child(content),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// Where the box is, by what is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Side {
    #[default]
    Over,
    Under,
    Beside,
}

impl Editor {
    /// The fonts shown: Perora's own first (unless something is sought), then those the system has
    /// with what is sought in their names.
    pub(super) fn fonts_shown(&self, cx: &gpui::App) -> Vec<String> {
        let sought = self.lettering.sought.read(cx).text().trim().to_lowercase();
        let all = self.lettering.fonts.clone().unwrap_or_default();
        let mut shown = Vec::with_capacity(all.len() + 1);
        if sought.is_empty() {
            shown.push(String::new());
        }
        shown.extend(
            all.iter()
                .filter(|font| font.to_lowercase().contains(&sought))
                .cloned(),
        );
        shown
    }

    /// The fonts opened: the system's are asked for (the first time), the search is empty and has
    /// the keyboard, and the keys are at the font in use.
    pub(super) fn fonts_opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.lettering.fonts.is_none() {
            #[cfg(windows)]
            let mut found = perora_core::platform::font_families();
            #[cfg(not(windows))]
            let mut found: Vec<String> = Vec::new();
            if found.is_empty() {
                found = FONTS.iter().map(|font| font.to_string()).collect();
            }
            self.lettering.fonts = Some(Rc::new(found));
        }
        self.lettering
            .sought
            .update(cx, |field, cx| field.set_text("", cx));
        let now = self.letters_now().font;
        let shown = self.fonts_shown(cx);
        self.lettering.at = shown.iter().position(|font| *font == now).unwrap_or(0);
        self.lettering
            .scroll
            .scroll_to_item(self.lettering.at, gpui::ScrollStrategy::Center);
        let focus = gpui::Focusable::focus_handle(self.lettering.sought.read(cx), cx);
        window.focus(&focus, cx);
    }

    /// A font is taken: what is written is written in it, and back to it.
    pub(super) fn pick_font(&mut self, font: String, window: &mut Window, cx: &mut Context<Self>) {
        self.set_lettering(LetterSet::Font(font), false, cx);
        self.asked = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The keys of the search: the arrows go through the fonts shown, Enter takes the one they are
    /// at, Esc puts the list away. Whether the key was one of those.
    pub(super) fn fonts_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let shown = self.fonts_shown(cx);
        let here = self.lettering.at.min(shown.len().saturating_sub(1));
        match key {
            "down" | "up" => {
                self.lettering.at = if key == "down" {
                    (here + 1).min(shown.len().saturating_sub(1))
                } else {
                    here.saturating_sub(1)
                };
                self.lettering
                    .scroll
                    .scroll_to_item(self.lettering.at, gpui::ScrollStrategy::Nearest);
                cx.notify();
                true
            }
            "enter" => {
                if let Some(font) = shown.get(here) {
                    self.pick_font(font.clone(), window, cx);
                }
                true
            }
            "escape" => {
                self.asked = None;
                window.focus(&self.focus, cx);
                cx.notify();
                true
            }
            _ => false,
        }
    }

    /// The list of fonts, under the search: each written in itself.
    pub(super) fn fonts_list(
        &self,
        now: &str,
        leaving: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let shown = Rc::new(self.fonts_shown(cx));
        let here = self.lettering.at.min(shown.len().saturating_sub(1));
        let own: SharedString = t("editor.font.own");
        let ours = theme.font.clone();
        let now = now.to_string();
        let (hover, current, accent) = (theme.surface_hover, theme.background, theme.accent);
        let radius = theme.radius_small;
        let count = shown.len();
        let list = gpui::uniform_list(
            "fonts",
            count,
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let font = shown[index].clone();
                        let named: SharedString = if font.is_empty() {
                            own.clone()
                        } else {
                            font.clone().into()
                        };
                        let family: SharedString = if font.is_empty() {
                            ours.clone()
                        } else {
                            font.clone().into()
                        };
                        let picked = font.clone();
                        div()
                            .id(("font", index))
                            .h(px(30.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .rounded(radius)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .font_family(family)
                            .when(index == here, |row| row.bg(hover))
                            .when(font == now, |row| {
                                row.border_1().border_color(accent).bg(current)
                            })
                            .hover(|style| style.bg(hover))
                            .when(!leaving, |row| {
                                row.on_click(cx.listener(move |this, _, window, cx| {
                                    this.pick_font(picked.clone(), window, cx)
                                }))
                            })
                            .child(named)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.lettering.scroll)
        .h(px(280.0));
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .id("seek")
                    .tooltip(tip(t("editor.font.seek")))
                    .child(self.lettering.sought.clone()),
            )
            .child(list)
            .when(count == 0, |list| {
                list.child(
                    div()
                        .p(px(10.0))
                        .text_color(theme.muted)
                        .child(t("editor.font.none")),
                )
            })
    }
}

/// Families to choose from until the system has said which it has (and should it never say).
const FONTS: [&str; 14] = [
    "Segoe UI",
    "Yu Gothic UI",
    "Meiryo",
    "BIZ UDPGothic",
    "BIZ UDPMincho",
    "Yu Mincho",
    "MS Gothic",
    "Consolas",
    "Cascadia Code",
    "Arial",
    "Times New Roman",
    "Georgia",
    "Impact",
    "Comic Sans MS",
];
