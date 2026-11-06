//! A choice of one among several: the browser's `<select>` in the Tauri version, which GPUI has no
//! counterpart of. A button that says what is chosen, as wide as the longest choice, and a list that
//! opens under it (above it, where there is no room under it), with headings for groups of choices.
//!
//! The pointer opens it, picks a choice, or closes it by pressing anywhere else. The keyboard, while
//! it has the button: Up and Down (Home, End) choose at once while it is closed, as the system's
//! own lists do; Enter, Space, F4 or Alt + Down open it; open, Up and Down go through the choices,
//! Enter or Space take one, Esc closes it as it was.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Anchor, App, Bounds, BoxShadow, ElementId, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, RenderOnce, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, TextRun, Window, anchored, canvas, deferred,
    div, font, point, px, rgba,
};

use crate::Theme;
use crate::button::{focus_of, glyph};

/// Segoe Fluent Icons' chevron, pointing down.
const CHEVRON: &str = "\u{E70D}";
/// The list is at most this tall; more choices than fit are scrolled to.
const LIST_HEIGHT: f32 = 360.0;
/// How many choices Page Up and Page Down go by.
const PAGE: usize = 8;

/// One line of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// What can be chosen, as it is said.
    Choice(SharedString),
    /// The heading of the choices after it (`<optgroup>`): not to be chosen.
    Group(SharedString),
}

type OnChange = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    items: Vec<Item>,
    /// Which of the choices (counted without the headings) is chosen.
    chosen: Option<usize>,
    enabled: bool,
    least_width: Pixels,
    on_change: Option<OnChange>,
}

impl Select {
    /// `chosen` counts the choices only, without the headings.
    pub fn new(id: impl Into<ElementId>, items: Vec<Item>, chosen: Option<usize>) -> Self {
        Self {
            id: id.into(),
            items,
            chosen,
            enabled: true,
            least_width: px(160.0),
            on_change: None,
        }
    }

    /// Choices that are all said with nothing else (no headings).
    pub fn of(
        id: impl Into<ElementId>,
        choices: impl IntoIterator<Item = impl Into<SharedString>>,
        chosen: Option<usize>,
    ) -> Self {
        let items = choices
            .into_iter()
            .map(|choice| Item::Choice(choice.into()))
            .collect();
        Self::new(id, items, chosen)
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// At least this wide (160 unless said: `.page select` in the style sheet), however short the
    /// choices are.
    pub fn least_width(mut self, width: Pixels) -> Self {
        self.least_width = width;
        self
    }

    /// Told of the choice taken (counted without the headings), when it is not the one chosen.
    pub fn on_change(mut self, on_change: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(on_change));
        self
    }
}

/// Whether the list is open, which choice the keys and the pointer are at meanwhile, which is
/// chosen (as it was drawn, or taken since: two presses of a key before it is drawn again are two
/// steps), and where the button was last drawn (what the list opens beside).
struct Opened {
    open: bool,
    at: usize,
    chosen: Option<usize>,
    button: Option<Bounds<Pixels>>,
    scroll: ScrollHandle,
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let focus = focus_of(&self.id, window, cx);
        let opened = window.use_keyed_state((self.id.clone(), "opened"), cx, |_, _| Opened {
            open: false,
            at: 0,
            chosen: None,
            button: None,
            scroll: ScrollHandle::new(),
        });
        let choices: Vec<SharedString> = self
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Choice(choice) => Some(choice.clone()),
                Item::Group(_) => None,
            })
            .collect();
        // Where each choice is among the lines of the list.
        let lines: Rc<Vec<usize>> = Rc::new(
            self.items
                .iter()
                .enumerate()
                .filter(|(_, item)| matches!(item, Item::Choice(_)))
                .map(|(line, _)| line)
                .collect(),
        );
        let count = choices.len();
        let chosen = self.chosen.filter(|&chosen| chosen < count);
        opened.update(cx, |opened, _| opened.chosen = chosen);
        let open = self.enabled && opened.read(cx).open;
        if !self.enabled && opened.read(cx).open {
            opened.update(cx, |opened, _| opened.open = false);
        }

        // As wide as the longest choice, as the browser's are.
        let widest = choices
            .iter()
            .map(|choice| {
                let run = TextRun {
                    len: choice.len(),
                    font: font(theme.font.clone()),
                    color: theme.text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line(choice.clone(), theme.text_size, &[run], None)
                    .width
            })
            .fold(px(0.0), |widest, width| widest.max(width));
        // (The room on both sides, the chevron and the room before it, and the edges.)
        let width = self
            .least_width
            .max((widest + px(24.0 + 8.0 + 12.0 + 2.0)).ceil());

        let on_change = self.on_change.clone();
        let choose: Rc<dyn Fn(usize, &mut Window, &mut App)> = {
            let opened = opened.clone();
            Rc::new(move |choice: usize, window: &mut Window, cx: &mut App| {
                let before = opened.update(cx, |opened, _| {
                    opened.open = false;
                    opened.chosen.replace(choice)
                });
                window.refresh();
                if Some(choice) != before
                    && let Some(on_change) = &on_change
                {
                    on_change(choice, window, cx);
                }
            })
        };
        let open_at = {
            let opened = opened.clone();
            let lines = lines.clone();
            move |at: usize, window: &mut Window, cx: &mut App| {
                opened.update(cx, |opened, _| {
                    opened.open = true;
                    opened.at = at;
                    if let Some(&line) = lines.get(at) {
                        opened.scroll.scroll_to_item(line);
                    }
                });
                window.refresh();
            }
        };

        let label = chosen
            .and_then(|chosen| choices.get(chosen).cloned())
            .unwrap_or_default();
        let measured = opened.clone();
        let button = div()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(32.0))
            .w(width)
            .px(px(12.0))
            .rounded(theme.radius_small)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface_hover)
            .text_color(theme.text)
            .whitespace_nowrap()
            .when(!self.enabled, |button| button.opacity(0.5))
            .when(self.enabled, |button| {
                let hover = theme.muted;
                let toggled = {
                    let opened = opened.clone();
                    let open_at = open_at.clone();
                    let focus = focus.clone();
                    move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        window.focus(&focus, cx);
                        if opened.read(cx).open {
                            opened.update(cx, |opened, _| opened.open = false);
                            window.refresh();
                        } else {
                            open_at(chosen.unwrap_or(0), window, cx);
                        }
                    }
                };
                let keys = {
                    let opened = opened.clone();
                    let choose = choose.clone();
                    let lines = lines.clone();
                    move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                        if count == 0 {
                            return;
                        }
                        let keystroke = &event.keystroke;
                        let key = keystroke.key.as_str();
                        let (open, at) = {
                            let opened = opened.read(cx);
                            (opened.open, opened.at)
                        };
                        let last = count - 1;
                        if open {
                            let to = match key {
                                "up" => at.saturating_sub(1),
                                "down" => (at + 1).min(last),
                                "pageup" => at.saturating_sub(PAGE),
                                "pagedown" => (at + PAGE).min(last),
                                "home" => 0,
                                "end" => last,
                                "enter" | "space" => {
                                    cx.stop_propagation();
                                    choose(at, window, cx);
                                    return;
                                }
                                "escape" => {
                                    cx.stop_propagation();
                                    opened.update(cx, |opened, _| opened.open = false);
                                    window.refresh();
                                    return;
                                }
                                // (Away with the keyboard: closed, and the keyboard goes on.)
                                "tab" => {
                                    opened.update(cx, |opened, _| opened.open = false);
                                    window.refresh();
                                    return;
                                }
                                _ => return,
                            };
                            cx.stop_propagation();
                            opened.update(cx, |opened, _| {
                                opened.at = to;
                                if let Some(&line) = lines.get(to) {
                                    opened.scroll.scroll_to_item(line);
                                }
                            });
                            window.refresh();
                        } else {
                            let chosen = opened.read(cx).chosen;
                            let now = chosen.unwrap_or(0);
                            let to = match key {
                                "down" if keystroke.modifiers.alt => {
                                    cx.stop_propagation();
                                    open_at(now, window, cx);
                                    return;
                                }
                                "enter" | "space" | "f4" => {
                                    cx.stop_propagation();
                                    open_at(now, window, cx);
                                    return;
                                }
                                "up" => now.saturating_sub(1),
                                "down" if chosen.is_none() => 0,
                                "down" => (now + 1).min(last),
                                "home" => 0,
                                "end" => last,
                                _ => return,
                            };
                            cx.stop_propagation();
                            choose(to, window, cx);
                        }
                    }
                };
                button
                    .track_focus(&focus)
                    .cursor_pointer()
                    .hover(move |style| style.border_color(hover))
                    .focus_visible(|style| style.border_color(theme.text))
                    .on_mouse_down(MouseButton::Left, toggled)
                    .on_key_down(keys)
            })
            .child(div().flex_1().min_w_0().overflow_hidden().child(label))
            .child(glyph(CHEVRON, 12.0))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        measured.update(cx, |opened, _| opened.button = Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );

        let below = opened.read(cx).button;
        let scroll = opened.read(cx).scroll.clone();
        let at = opened.read(cx).at;
        button.when_some(below.filter(|_| open), |button, below| {
            let closed = opened.clone();
            let mut choice = 0usize;
            let rows = self
                .items
                .into_iter()
                .enumerate()
                .map(|(line, item)| match item {
                    Item::Group(heading) => div()
                        .id(line)
                        .px(px(12.0))
                        .pt(px(8.0))
                        .pb(px(4.0))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted)
                        .child(heading),
                    Item::Choice(said) => {
                        let this = choice;
                        choice += 1;
                        let pointed = opened.clone();
                        let choose = choose.clone();
                        div()
                            .id(line)
                            .relative()
                            .flex_none()
                            .flex()
                            .items_center()
                            .h(px(32.0))
                            .px(px(12.0))
                            .rounded(theme.radius_small)
                            .whitespace_nowrap()
                            .cursor_pointer()
                            .when(this == at, |row| row.bg(theme.surface_hover))
                            .when(Some(this) == chosen, |row| {
                                // The one chosen, as the system's lists mark it: a short bar in the
                                // accent colour at its left.
                                row.child(
                                    div()
                                        .absolute()
                                        .left(px(0.0))
                                        .top(px(9.0))
                                        .bottom(px(9.0))
                                        .w(px(3.0))
                                        .rounded(px(2.0))
                                        .bg(theme.accent),
                                )
                            })
                            .on_mouse_move(move |_, window, cx| {
                                if pointed.read(cx).at != this {
                                    pointed.update(cx, |opened, _| opened.at = this);
                                    window.refresh();
                                }
                            })
                            .on_click(move |_, window, cx| choose(this, window, cx))
                            .child(said)
                    }
                });
            let list = div()
                .id((self.id.clone(), "list"))
                .occlude()
                .min_w(width)
                .max_h(px(LIST_HEIGHT))
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .p(px(4.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .text_color(theme.text)
                .text_size(theme.text_size)
                .font_family(theme.font.clone())
                .shadow(vec![BoxShadow {
                    color: rgba(0x0000_0040).into(),
                    offset: point(px(0.0), px(8.0)),
                    blur_radius: px(16.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .on_mouse_down_out(move |event, window, cx| {
                    // (A press on the button is the button's: it closes the list itself.)
                    if closed
                        .read(cx)
                        .button
                        .is_some_and(|button| button.contains(&event.position))
                    {
                        return;
                    }
                    closed.update(cx, |opened, _| opened.open = false);
                    window.refresh();
                })
                .children(rows);
            button.child(
                deferred(
                    anchored()
                        .attach(below, Anchor::BottomLeft)
                        .offset(point(px(0.0), px(2.0)))
                        .snap_to_window_with_margin(px(8.0))
                        .child(list),
                )
                .with_priority(1),
            )
        })
    }
}
