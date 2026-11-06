//! How the second bar and the panels look, and how they come and go: they come down into their
//! place (0.22 s, from 14 pixels over it, growing solid), and go back up (0.16 s) the way they came,
//! what they showed staying with them meanwhile (`Lingering` in editor.tsx). Another bar does not
//! cross the one before: that one is gone at once, and the other comes down. None of it moves for a
//! user who has the system move nothing.
//!
//! In a panel, the colours offered and "その他の色" (any other colour): pressed, it slides up over the
//! colours (0.28 s) and the place where a colour is mixed comes in under it, in the same room (owner:
//! 「カスタムカラーピッカーつけたときはプリセットカラー部分を非表示にしてUI領域を節約したい +
//! HEX以外にRGBも入れられるようにしたいかも」).

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnchoredPositionMode, AnyElement, AppContext, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, MouseDownEvent, ParentElement, Pixels, Point, SharedString,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use perora_core::colours::{Hsv, NONE_TAKES, PALETTE};
use perora_core::edit::Colour;
use perora_core::platform;
use perora_ui::motion::{self, Curve};
use perora_ui::swatch::{self, swatch};
use perora_ui::{Shades, Slider, TextField, TextFieldEvent, Theme};

use super::bars::{
    LINE_SAMPLE, Offered, PANEL, PEN_SAMPLE, Panel, Pick, STEPS, Set, SetStep, button, recorded,
    rule, shadow,
};
use super::{Editor, tip};
use crate::icons;
use crate::paint;
use crate::tools::Kind;
use crate::tools::pen::RED;

/// How a bar or a panel comes down, and how it goes back up (`come-down`, `go-up` in style.css).
const COMING: Duration = Duration::from_millis(220);
const COMES: Curve = Curve(0.1, 0.9, 0.2, 1.0);
const GOING: Duration = Duration::from_millis(160);
const GOES: Curve = Curve(0.7, 0.0, 0.84, 0.0);
/// How far over its place it comes down from.
const FROM_OVER: f32 = 14.0;
/// How the colours give way to the place where one is mixed.
const SLIDING: Duration = Duration::from_millis(280);
const SLIDES: Curve = Curve(0.2, 0.8, 0.2, 1.0);
/// The place where a colour is mixed is never shorter than this (a marker's six colours are one
/// row: it grows downwards for it).
const MIXED_LEAST: f32 = 234.0;

/// What a second bar showed: enough to show it again while it goes.
#[derive(Clone)]
pub(super) struct BarLook {
    offered: Offered,
    tool: Kind,
    outline: Colour,
    inside: (Colour, f32),
    fills: bool,
    stroke: Colour,
    open: Option<Panel>,
}

/// What a panel showed.
#[derive(Clone)]
pub(super) struct PanelLook {
    panel: Panel,
    /// Its corner in the window.
    at: Point<Pixels>,
    offered: Vec<Option<Colour>>,
    /// The colour in use (none: no colour), and where mixing starts while there is none.
    colour: Option<Colour>,
    fallback: Colour,
    step: u32,
    /// How solid an inside is (0: none), and how solid it is when given a colour.
    fill: f32,
    solid: f32,
}

/// What came or is going, and since when.
#[derive(Default)]
pub(super) struct Motions {
    bar: Option<(Offered, Instant)>,
    bar_leaving: Option<(BarLook, Instant)>,
    bar_look: Option<BarLook>,
    panel: Option<(Panel, Instant)>,
    panel_leaving: Option<(PanelLook, Instant)>,
    panel_look: Option<PanelLook>,
}

/// Where a colour is mixed: whether it is open (instead of the colours offered), since when it slid
/// so, the hue kept apart from the colour (black has none of its own: the field is not to jump to
/// red at its bottom), and the boxes the colour is written in, with the colour they were last made
/// to show (what is typed stays while it is still that colour).
pub(super) struct Mixing {
    open: bool,
    slid: Option<Instant>,
    kept: Option<Hsv>,
    hex: Entity<TextField>,
    parts: [Entity<TextField>; 3],
    hex_of: Option<Colour>,
    parts_of: Option<Colour>,
}

impl Mixing {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        let hex = cx.new(|cx| TextField::new(cx).longest(7).lowercase());
        cx.subscribe(&hex, |this: &mut Editor, _, event: &TextFieldEvent, cx| {
            let TextFieldEvent::Changed(text) = event;
            this.hex_written(text, cx);
        })
        .detach();
        let parts = [(); 3].map(|_| cx.new(|cx| TextField::new(cx).longest(3).to_the_right()));
        for part in &parts {
            cx.subscribe(part, |this: &mut Editor, _, _: &TextFieldEvent, cx| {
                this.parts_written(cx);
            })
            .detach();
        }
        Self {
            open: false,
            slid: None,
            kept: None,
            hex,
            parts,
            hex_of: None,
            parts_of: None,
        }
    }

    /// Whether one of its boxes has the keyboard.
    pub(super) fn has_keyboard(&self, window: &Window, cx: &gpui::App) -> bool {
        std::iter::once(&self.hex)
            .chain(self.parts.iter())
            .any(|field| gpui::Focusable::focus_handle(field.read(cx), cx).is_focused(window))
    }
}

/// How far a motion that began at `since` has come (0 to 1), or gone (1 to 0) when `leaving`.
fn along(since: Instant, leaving: bool, window: &mut Window) -> f32 {
    if !platform::animations_enabled() {
        return if leaving { 0.0 } else { 1.0 };
    }
    if leaving {
        1.0 - GOES.at(motion::along(since, Duration::ZERO, GOING, window))
    } else {
        COMES.at(motion::along(since, Duration::ZERO, COMING, window))
    }
}

/// What comes down (or goes up), as far as it has come.
fn coming(element: impl IntoElement, share: f32) -> gpui::Div {
    div()
        .relative()
        .top(px(-FROM_OVER * (1.0 - share)))
        .opacity(share)
        .child(element)
}

/// How tall the colours offered are, six to a row.
fn tall_of(colours: usize) -> f32 {
    let rows = colours.div_ceil(6).max(1) as f32;
    rows * 36.0 + (rows - 1.0) * 12.0 + 6.0
}

impl Editor {
    /// The colours a panel offers, the one in use there (none: no colour), and where mixing starts
    /// while there is none.
    fn colours_of(&self, panel: Panel) -> (Vec<Option<Colour>>, Option<Colour>, Colour) {
        let palette = || PALETTE.iter().copied().map(Some).collect::<Vec<_>>();
        match panel {
            Panel::Pen | Panel::Stroke => (palette(), Some(self.stroke_now().0), RED),
            Panel::Outline => (palette(), Some(self.outline_now().0), RED),
            Panel::Fill => {
                let (colour, fill) = self.inside_now();
                // "None" takes the place of the darkest grey: the rows stay six wide.
                let offered = std::iter::once(None)
                    .chain(
                        PALETTE
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| *index != NONE_TAKES)
                            .map(|(_, colour)| Some(*colour)),
                    )
                    .collect();
                (offered, (fill > 0.0).then_some(colour), colour)
            }
        }
    }

    /// A colour chosen in a panel, for what it sets (`dragged`: one of many on the way to the one
    /// meant, gathered into one step to undo).
    fn pick_for(
        &mut self,
        panel: Panel,
        colour: Option<Colour>,
        dragged: bool,
        cx: &mut Context<Self>,
    ) {
        match (panel, colour) {
            (Panel::Pen | Panel::Stroke, Some(colour)) => {
                self.set_stroke(Set::Colour(colour), dragged, cx)
            }
            (Panel::Outline, Some(colour)) => self.set_outline(Set::Colour(colour), dragged, cx),
            (Panel::Fill, colour) => self.set_inside(Set::Inside(colour), dragged, cx),
            _ => {}
        }
    }

    /// A colour mixed in the field of shades or by its hue.
    fn mix(&mut self, panel: Panel, next: Hsv, cx: &mut Context<Self>) {
        self.mixing.kept = Some(next);
        self.pick_for(panel, Some(next.colour()), true, cx);
    }

    /// What the colour in use is mixed from: the colour, or where mixing starts without one.
    fn mixed_value(&self, panel: Panel) -> Colour {
        let (_, colour, fallback) = self.colours_of(panel);
        colour.unwrap_or(fallback)
    }

    /// Something was typed in the box of `#rrggbb`: a colour once it is one (`#` may be left out).
    fn hex_written(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(panel) = self.panel() else {
            return;
        };
        let value = self.mixed_value(panel);
        let text = text.trim().to_lowercase();
        let written = if text.starts_with('#') {
            text
        } else {
            format!("#{text}")
        };
        let colour = Colour::parse(&written);
        self.mixing.hex_of = Some(colour.unwrap_or(value));
        if colour.is_some() {
            self.pick_for(panel, colour, false, cx);
        }
    }

    /// Something was typed in a box of red, green or blue: a colour once all three are numbers up
    /// to 255.
    fn parts_written(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.panel() else {
            return;
        };
        let value = self.mixed_value(panel);
        let parts: Vec<Option<u8>> = self
            .mixing
            .parts
            .iter()
            .map(|field| {
                let text = field.read(cx).text().trim();
                let digits =
                    (1..=3).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
                digits
                    .then(|| text.parse::<u16>().ok())
                    .flatten()
                    .and_then(|n| u8::try_from(n).ok())
            })
            .collect();
        let colour = match parts.as_slice() {
            [Some(red), Some(green), Some(blue)] => Some(Colour {
                red: *red,
                green: *green,
                blue: *blue,
            }),
            _ => None,
        };
        self.mixing.parts_of = Some(colour.unwrap_or(value));
        if let Some(colour) = colour
            && colour != value
        {
            self.pick_for(panel, Some(colour), false, cx);
        }
    }

    /// The bars and the panels follow what changed since they were last drawn: what came is to come
    /// down, what went is to go back up; a panel opens anew (mixing, if the colour in use is none of
    /// those offered); a box of a panel gone gives the keyboard back to the picture.
    pub(super) fn follow_motions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let moving = platform::animations_enabled();
        let second = self.second();
        if second != self.motions.bar.as_ref().map(|(offered, _)| *offered) {
            self.motions.bar_leaving = match (second, self.motions.bar_look.take()) {
                (None, Some(look)) if moving => Some((look, now)),
                _ => None,
            };
            self.motions.bar = second.map(|offered| (offered, now));
        }
        let panel = self.panel();
        if panel != self.motions.panel.as_ref().map(|(panel, _)| *panel) {
            self.motions.panel_leaving = match self.motions.panel_look.take() {
                Some(look) if moving => Some((look, now)),
                _ => None,
            };
            self.motions.panel = panel.map(|panel| (panel, now));
            let open = panel.is_some_and(|panel| {
                let (offered, colour, _) = self.colours_of(panel);
                colour.is_some_and(|colour| !offered.contains(&Some(colour)))
            });
            self.mixing.open = open;
            self.mixing.slid = None;
            self.mixing.kept = None;
            self.mixing.hex_of = None;
            self.mixing.parts_of = None;
            if self.mixing.has_keyboard(window, cx) {
                window.focus(&self.focus, cx);
            }
        }
    }

    fn bar_look(&self, offered: Offered) -> BarLook {
        BarLook {
            offered,
            tool: self.tool,
            outline: self.outline_now().0,
            inside: self.inside_now(),
            fills: self.fills(),
            stroke: self.stroke_now().0,
            open: self.panel(),
        }
    }

    /// The second bar, there or on its way out.
    pub(super) fn second_bar(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let live = self.second().map(|offered| self.bar_look(offered));
        self.motions.bar_look = live.clone();
        let (look, since, leaving) = match live {
            Some(look) => {
                let since = self
                    .motions
                    .bar
                    .as_ref()
                    .map_or_else(Instant::now, |(_, since)| *since);
                (look, since, false)
            }
            None => {
                let (look, since) = self.motions.bar_leaving.clone()?;
                if since.elapsed() >= GOING {
                    self.motions.bar_leaving = None;
                    return None;
                }
                (look, since, true)
            }
        };
        let share = along(since, leaving, window);
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let mut bar = div()
            .id("second")
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .py(px(5.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow(shadow(4.0, 14.0, 0x0000_004d))
            // (On its way out, it is not there to be pressed: what is under it is.)
            .when(!leaving, |bar| bar.occlude());
        if look.offered == Offered::Figures {
            let (fill_colour, fill) = look.inside;
            let inside = (fill > 0.0).then(|| (paint::colour(fill_colour), fill));
            for kind in [Kind::Rectangle, Kind::Ellipse, Kind::Line, Kind::Arrow] {
                let named = format!("{} ({})", t(kind.name()), kind.key().to_uppercase());
                bar = bar.child(
                    button(
                        format!("second-{}", kind.name()),
                        look.tool == kind,
                        true,
                        theme,
                    )
                    .tooltip(tip(named))
                    .when(!leaving, |button| {
                        button.on_click(cx.listener(move |this, _, _, cx| this.pick(kind, cx)))
                    })
                    .child(icons::icon(
                        icons::of_tool(kind, Some(paint::colour(look.outline)), inside),
                        16.0,
                        theme.text,
                    )),
                );
            }
            bar = bar.child(rule(theme));
        }
        match look.offered {
            Offered::Stroke => {
                bar = bar.child(self.opener(
                    Panel::Stroke,
                    t("editor.stroke"),
                    Some(look.stroke),
                    true,
                    &look,
                    leaving,
                    theme,
                    cx,
                ));
            }
            Offered::Figures | Offered::Shape | Offered::Line => {
                let (fill_colour, fill) = look.inside;
                let shown = (look.fills && fill > 0.0).then_some(fill_colour);
                bar = bar
                    .child(self.opener(
                        Panel::Fill,
                        t("editor.fill"),
                        shown,
                        look.fills,
                        &look,
                        leaving,
                        theme,
                        cx,
                    ))
                    .child(self.opener(
                        Panel::Outline,
                        t("editor.outline"),
                        Some(look.outline),
                        true,
                        &look,
                        leaving,
                        theme,
                        cx,
                    ));
            }
        }
        // Over the top of the picture, under the first bar; no room is made for it (owner: 「押した
        // ときに下にpadding作られるの消してほしいー」).
        Some(
            deferred(
                div()
                    .absolute()
                    .top(px(16.0 + 40.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(coming(bar, share)),
            )
            .into_any_element(),
        )
    }

    /// A button of the second bar that opens a panel: the colour that is set (none: no colour),
    /// what it is the colour of, and the mark that there is more.
    #[allow(clippy::too_many_arguments)]
    fn opener(
        &self,
        panel: Panel,
        label: SharedString,
        colour: Option<Colour>,
        enabled: bool,
        look: &BarLook,
        leaving: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let open = look.open == Some(panel);
        div()
            .id(SharedString::from(format!("opener-{panel:?}")))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(px(34.0))
            .pl(px(8.0))
            .pr(px(10.0))
            .rounded(theme.radius_small)
            .border_1()
            .border_color(gpui::transparent_black())
            .when(open, |opener| opener.bg(theme.surface_hover))
            .when(enabled && !open, |opener| {
                opener.hover(|style| style.bg(theme.surface_hover))
            })
            .when(!enabled, |opener| opener.opacity(0.4))
            .when(enabled && !leaving, |opener| {
                opener.on_click(cx.listener(move |this, _, _, cx| this.open(panel, cx)))
            })
            .child(swatch(colour.map(paint::colour), 20.0, false, theme))
            .child(label)
            .child(icons::caret(theme.text))
            .when(!leaving, |opener| {
                opener.child(recorded(&self.openers, panel))
            })
    }

    fn panel_look(&self, panel: Panel) -> Option<PanelLook> {
        let under = *self.openers.borrow().get(&panel)?;
        let (offered, colour, fallback) = self.colours_of(panel);
        let step = match panel {
            Panel::Pen | Panel::Stroke => self.stroke_now().1,
            Panel::Outline => self.outline_now().1,
            Panel::Fill => 0,
        };
        Some(PanelLook {
            panel,
            at: point(under.center().x - px(PANEL / 2.0), under.bottom() + px(8.0)),
            offered,
            colour,
            fallback,
            step,
            fill: self.inside_now().1,
            solid: self.solid,
        })
    }

    /// The panel that is open, under its button, or on its way out.
    pub(super) fn panel_view(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let live = self.panel().and_then(|panel| self.panel_look(panel));
        self.motions.panel_look = live.clone();
        let (look, since, leaving) = match live {
            Some(look) => {
                let since = self
                    .motions
                    .panel
                    .as_ref()
                    .map_or_else(Instant::now, |(_, since)| *since);
                (look, since, false)
            }
            None => {
                let (look, since) = self.motions.panel_leaving.clone()?;
                if since.elapsed() >= GOING {
                    self.motions.panel_leaving = None;
                    return None;
                }
                (look, since, true)
            }
        };
        if !leaving {
            self.show_in_boxes(look.colour.unwrap_or(look.fallback), cx);
        }
        let share = along(since, leaving, window);
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let panel = look.panel;
        let mut settings: Vec<AnyElement> = vec![
            self.colours(&look, leaving, theme, window, cx)
                .into_any_element(),
        ];
        match panel {
            Panel::Pen | Panel::Stroke | Panel::Outline => {
                let (sample, set): (&'static str, SetStep) = if panel == Panel::Outline {
                    (
                        LINE_SAMPLE,
                        Rc::new(|this: &mut Editor, step, cx: &mut Context<Editor>| {
                            this.set_outline(Set::Step(step), true, cx)
                        }),
                    )
                } else {
                    (
                        PEN_SAMPLE,
                        Rc::new(|this: &mut Editor, step, cx: &mut Context<Editor>| {
                            this.set_stroke(Set::Step(step), true, cx)
                        }),
                    )
                };
                let colour = look.colour.unwrap_or(look.fallback);
                settings.push(
                    self.size(sample, colour, look.step, leaving, theme, cx, set)
                        .into_any_element(),
                );
            }
            Panel::Fill => {
                let some = look.fill > 0.0;
                let shown = ((if some { look.fill } else { look.solid }) * 100.0).round();
                let this = cx.entity().downgrade();
                settings.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .when(!some, |setting| setting.opacity(0.4))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t("editor.opacity")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Slider::new("opacity", shown, 5.0, 100.0)
                                            .step(5.0)
                                            .enabled(some)
                                            .inert(leaving)
                                            .on_change(move |value, _, cx| {
                                                let _ = this.update(cx, |this, cx| {
                                                    this.set_inside(
                                                        Set::Solid(value / 100.0),
                                                        true,
                                                        cx,
                                                    )
                                                });
                                            }),
                                    ),
                                )
                                .child(
                                    div()
                                        .min_w(px(34.0))
                                        .flex()
                                        .justify_end()
                                        .child(format!("{}%", (look.fill * 100.0).round())),
                                ),
                        )
                        .into_any_element(),
                );
            }
        }
        let openers = self.openers.clone();
        let content = div()
            .id("panel")
            .w(px(PANEL))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .pt(px(14.0))
            .px(px(16.0))
            .pb(px(16.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow(shadow(8.0, 24.0, 0x0000_0059))
            .cursor_default()
            .when(!leaving, |content| {
                content
                    .occlude()
                    // A press anywhere but in it and on what opened it puts it away.
                    .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let on_opener = openers
                            .borrow()
                            .get(&panel)
                            .is_some_and(|bounds| bounds.contains(&event.position));
                        if !on_opener && this.asked == Some(panel) {
                            this.asked = None;
                            cx.notify();
                        }
                    }))
            })
            .children(settings);
        Some(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .position(look.at)
                    .snap_to_window_with_margin(px(8.0))
                    .child(coming(content, share)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The boxes the colour is written in show `colour`, unless what is typed in them is it already.
    fn show_in_boxes(&mut self, colour: Colour, cx: &mut Context<Self>) {
        if self.mixing.hex_of != Some(colour) {
            self.mixing.hex_of = Some(colour);
            let written = colour.written();
            self.mixing
                .hex
                .update(cx, |field, cx| field.set_text(&written, cx));
        }
        if self.mixing.parts_of != Some(colour) {
            self.mixing.parts_of = Some(colour);
            for (field, part) in
                self.mixing
                    .parts
                    .iter()
                    .zip([colour.red, colour.green, colour.blue])
            {
                field.update(cx, |field, cx| field.set_text(&part.to_string(), cx));
            }
        }
    }

    /// The colours to choose from, six to a row (none: no colour), and under them "その他の色": the
    /// place to mix any other. Choosing one of the colours offered is the choice the panel was
    /// opened for: it goes. (Mixing is many choices on the way to one: it stays.)
    fn colours(
        &self,
        look: &PanelLook,
        leaving: bool,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let none = t("editor.none");
        let panel = look.panel;
        let mixing = self.mixing.open;
        let pick: Pick = Rc::new(move |this: &mut Editor, colour, cx: &mut Context<Editor>| {
            this.pick_for(panel, colour, false, cx)
        });
        let rows = look.offered.chunks(6).enumerate().map(|(row, colours)| {
            div()
                .flex()
                .justify_between()
                .children(colours.iter().enumerate().map(|(at, colour)| {
                    let colour = *colour;
                    let pick = pick.clone();
                    let named: SharedString = match colour {
                        Some(colour) => colour.written().into(),
                        None => none.clone(),
                    };
                    div()
                        .id(("colour", row * 6 + at))
                        .cursor_pointer()
                        .rounded_full()
                        .tooltip(tip(named))
                        .when(!leaving && !mixing, |disc| {
                            disc.on_click(cx.listener(move |this, _, _, cx| {
                                pick(this, colour, cx);
                                this.asked = None;
                                cx.notify();
                            }))
                        })
                        .child(swatch(
                            colour.map(paint::colour),
                            36.0,
                            colour == look.colour,
                            theme,
                        ))
                }))
        });
        // How far it has slid to the mixing (0: the colours, 1: the mixing).
        let slid = match self.mixing.slid {
            Some(since) if platform::animations_enabled() => {
                SLIDES.at(motion::along(since, Duration::ZERO, SLIDING, window))
            }
            _ => 1.0,
        };
        let toward = if mixing { slid } else { 1.0 - slid };
        let tall = tall_of(look.offered.len());
        let mixed = tall.max(MIXED_LEAST);
        let height = tall + (mixed - tall) * toward + 10.0 + 34.0;
        let value = look.colour.unwrap_or(look.fallback);
        let more = div()
            .id("more")
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .w_full()
            .h(px(34.0))
            .px(px(8.0))
            .rounded(theme.radius_small)
            .text_color(theme.muted)
            .hover(|style| style.bg(theme.surface_hover))
            .when(!leaving, |more| {
                more.on_click(cx.listener(|this, _, _, cx| {
                    this.mixing.open = !this.mixing.open;
                    this.mixing.slid = Some(Instant::now());
                    cx.notify();
                }))
            })
            .child(swatch::any(20.0, theme))
            .child(t("editor.colours.more"))
            .child(div().flex_1())
            .child(icons::caret_turned(theme.text, mixing));
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("editor.colours")),
            )
            .child(
                div().relative().h(px(height)).overflow_hidden().child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(-(tall + 10.0) * toward))
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        // (The ring of the colour in use reaches a little past it.)
                        .child(
                            div()
                                .h(px(tall))
                                .flex()
                                .flex_col()
                                .gap(px(12.0))
                                .p(px(3.0))
                                .children(rows),
                        )
                        .child(more)
                        .child(
                            div()
                                .h(px(mixed))
                                .child(self.mixer(panel, value, leaving, theme, cx)),
                        ),
                ),
            )
    }

    /// Any colour: a field of every shade of one hue, the hues under it, and the colour written out
    /// (`#rrggbb`, and its red, green and blue, 0 to 255 each).
    fn mixer(
        &self,
        panel: Panel,
        value: Colour,
        leaving: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        let mixed = match self.mixing.kept {
            Some(kept) if kept.colour() == value => kept,
            _ => Hsv::of(value),
        };
        let live = !leaving && self.mixing.open;
        let shades = {
            let this = cx.entity().downgrade();
            let field = Shades::new(
                "shades",
                mixed.hue,
                mixed.saturation,
                mixed.value,
                paint::colour(value),
            );
            if live {
                field.on_change(move |saturation, shade, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.mix(
                            panel,
                            Hsv {
                                saturation,
                                value: shade,
                                ..mixed
                            },
                            cx,
                        )
                    });
                })
            } else {
                field
            }
        };
        let hues = {
            let this = cx.entity().downgrade();
            Slider::new("hues", mixed.hue.round().min(359.0), 0.0, 359.0)
                .hues()
                .inert(!live)
                .on_change(move |hue, _, cx| {
                    let _ = this.update(cx, |this, cx| this.mix(panel, Hsv { hue, ..mixed }, cx));
                })
        };
        let letters = ["R", "G", "B"];
        let names = [
            "editor.colours.red",
            "editor.colours.green",
            "editor.colours.blue",
        ];
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(shades)
            .child(hues)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .id("written")
                            .flex_1()
                            .min_w_0()
                            .tooltip(tip(t("editor.colours.written")))
                            .child(self.mixing.hex.clone()),
                    )
                    .children(self.mixing.parts.iter().enumerate().map(|(index, field)| {
                        div()
                            .id(("part", index))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_color(theme.muted)
                            .tooltip(tip(t(names[index])))
                            .child(letters[index])
                            .child(div().w(px(44.0)).child(field.clone()))
                    })),
            )
    }

    /// How wide the pen or a line is: a stroke as wide as it would be drawn, and a slider.
    #[allow(clippy::too_many_arguments)]
    fn size(
        &self,
        sample: &'static str,
        colour: Colour,
        step: u32,
        leaving: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
        set: SetStep,
    ) -> impl IntoElement + use<> {
        let t = |key: &str| SharedString::from(self.language.text(key, &[]));
        // On the screen, a step is about a pixel.
        let wide = (step as f32 * 1.2 + 1.0).min(40.0);
        let this = cx.entity().downgrade();
        let shown = step.clamp(STEPS.0, STEPS.1);
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("editor.size")),
            )
            .child(
                icons::drawing(
                    vec![icons::sample(
                        sample,
                        wide,
                        paint::colour(colour),
                        1.0,
                        false,
                    )],
                    (240.0, 64.0),
                    theme.text,
                )
                .w_full()
                .h(px(64.0)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div().flex_1().min_w_0().child(
                            Slider::new("size", shown as f32, STEPS.0 as f32, STEPS.1 as f32)
                                .inert(leaving)
                                .on_change(move |value, _, cx| {
                                    let _ = this
                                        .update(cx, |this, cx| set(this, value.round() as u32, cx));
                                }),
                        ),
                    )
                    .child(
                        div()
                            .min_w(px(34.0))
                            .flex()
                            .justify_end()
                            .child(step.to_string()),
                    ),
            )
    }
}
