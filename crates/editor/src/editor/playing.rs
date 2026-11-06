//! What the steps of the tour have the editor do while they are on: a tool used on the picture as it
//! would be (the pen drawing a wave, the text tool writing "Perora"...), the wheel going through the
//! tools, the settings opened where the other button was pressed twice, the picture looked at closer.
//! The pointer and the mouse are drawn where it is done. Played again and again, from what the editor
//! was, for as long as the step is on; and the editor is as it was once the step is over: what was
//! drawn meanwhile is not the user's (it is not told to the window, nor among what is undone).
//! While something is under way that it would end (something written, selected, cut down, a press),
//! nothing is done: the mouse alone shows it (as the Tauri version's tour, `tours.tsx`).

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{Context, IntoElement, Pixels, Point, ScrollDelta, ScrollEvent, TouchPhase, point, px};
use perora_core::drawing::Drawing;
use perora_core::edit::{Edit, Place};
use perora_ui::hand::{Held, Wheel};

use super::bars::Panel;
use super::touring::{Played, Showing};
use super::{Editor, mark_of};
use crate::tools::{Kind, Press};
use crate::view::{Area, Looking};

/// How often a play looks at what is to be done next.
const BEAT: Duration = Duration::from_millis(40);

/// A place on the picture, as a share of it across and down.
type Part = (f32, f32);

/// What a beat of a play does.
#[derive(Debug, Clone)]
enum Act {
    /// The tool in hand pressed there, or dragged on to there (as the pointer would).
    Begin(Part),
    GoOn(Part),
    /// What the text tool writes at that place.
    Write(Part, &'static str),
    /// The stamp put down there.
    Stamp(Part),
    /// The frame to cut the image down to, from there to the far corner.
    Frame(Part),
    /// What was drawn first selected; moved by so much.
    Select,
    Move(Part),
    /// The settings there, as the other button pressed twice opens them.
    Settings(Part),
}

#[derive(Debug, Clone)]
struct Beat {
    after: u32,
    hand: Option<(Part, Option<Held>)>,
    act: Option<Act>,
}

fn beat(after: u32, at: Part, held: Option<Held>, act: Option<Act>) -> Beat {
    Beat {
        after,
        hand: Some((at, held)),
        act,
    }
}

/// The hand dragged from one place to another over so long, a beat every 40 ms, the tool going on
/// with it (begun at the first).
fn dragged(from: Part, to: Part, start: u32, lasts: u32) -> Vec<Beat> {
    let steps = (lasts / 40).max(1);
    (0..=steps)
        .map(|step| {
            let along = step as f32 / steps as f32;
            let at = (
                from.0 + (to.0 - from.0) * along,
                from.1 + (to.1 - from.1) * along,
            );
            let act = if step == 0 {
                Act::Begin(at)
            } else {
                Act::GoOn(at)
            };
            beat(start + step * 40, at, Some(Held::Left), Some(act))
        })
        .collect()
}

/// The points of a wave from one place to another, as far as so much of it (0 to 1).
fn wave(from: Part, to: Part, much: f32) -> Part {
    let along = much;
    (
        from.0 + (to.0 - from.0) * along,
        from.1 + (to.1 - from.1) * along + (along * std::f32::consts::PI * 2.0).sin() * 0.06,
    )
}

/// A tool used on the picture: the tool it takes, its beats, and how long a round of it is.
fn play_of(played: Played) -> (Kind, Vec<Beat>, u32) {
    match played {
        Played::Pen => {
            let (from, to) = ((0.3, 0.6), (0.7, 0.6));
            let mut beats = vec![beat(0, from, None, None)];
            for step in 0..=30 {
                let at = wave(from, to, step as f32 / 30.0);
                let act = if step == 0 {
                    Act::Begin(at)
                } else {
                    Act::GoOn(at)
                };
                beats.push(beat(400 + step * 40, at, Some(Held::Left), Some(act)));
            }
            beats.push(beat(1700, to, None, None));
            (Kind::Pen, beats, 3400)
        }
        Played::Marker => {
            let (from, to) = ((0.3, 0.55), (0.7, 0.55));
            let mut beats = vec![beat(0, from, None, None)];
            beats.extend(dragged(from, to, 400, 1000));
            beats.push(beat(1500, to, None, None));
            (Kind::Marker, beats, 3200)
        }
        Played::Shapes => {
            let (from, to) = ((0.32, 0.42), (0.66, 0.78));
            let mut beats = vec![beat(0, from, None, None)];
            beats.extend(dragged(from, to, 400, 1000));
            beats.push(beat(1500, to, None, None));
            (Kind::Rectangle, beats, 3200)
        }
        Played::Text => {
            let at = (0.35, 0.55);
            let mut beats = vec![
                beat(0, at, None, None),
                beat(300, at, Some(Held::Left), None),
                // (Then out of the way of what is written.)
                beat(450, at, None, None),
                beat(550, (0.3, 0.75), None, None),
            ];
            for (index, end) in [1, 2, 3, 4, 5, 6].into_iter().enumerate() {
                beats.push(Beat {
                    after: 700 + index as u32 * 160,
                    hand: None,
                    act: Some(Act::Write(at, &"Perora"[..end])),
                });
            }
            (Kind::Text, beats, 3400)
        }
        Played::Stamp => {
            let places = [(0.35, 0.5), (0.5, 0.62), (0.65, 0.5)];
            let beats = places
                .into_iter()
                .enumerate()
                .flat_map(|(number, at)| {
                    let start = number as u32 * 700;
                    [
                        beat(start, at, None, None),
                        beat(start + 250, at, Some(Held::Left), Some(Act::Stamp(at))),
                        beat(start + 400, at, None, None),
                    ]
                })
                .collect();
            (Kind::Stamp, beats, 3600)
        }
        Played::Hide => {
            let (from, to) = ((0.34, 0.45), (0.66, 0.72));
            let mut beats = vec![beat(0, from, None, None)];
            beats.extend(dragged(from, to, 400, 1000));
            beats.push(beat(1500, to, None, None));
            (Kind::Mosaic, beats, 3200)
        }
        Played::Crop => {
            let (from, to) = ((0.02, 0.02), (0.25, 0.22));
            let mut beats = vec![beat(0, from, None, None)];
            beats.extend(dragged(from, to, 400, 900).into_iter().map(|mut one| {
                if let Some((at, _)) = one.hand {
                    one.act = Some(Act::Frame(at));
                }
                one
            }));
            beats.push(beat(1400, to, None, None));
            (Kind::Crop, beats, 3200)
        }
        Played::Select => {
            // What there is to select: drawn at once (as the shapes would), then pressed on,
            // selected and dragged along; then the other button pressed twice on it.
            let mut beats = vec![
                beat(0, (0.5, 0.75), None, Some(Act::Begin((0.3, 0.45)))),
                Beat {
                    after: 10,
                    hand: None,
                    act: Some(Act::GoOn((0.5, 0.7))),
                },
                beat(300, (0.5, 0.6), None, None),
                beat(800, (0.5, 0.6), Some(Held::Left), Some(Act::Select)),
            ];
            let (from, to) = ((0.5, 0.6), (0.62, 0.52));
            let steps = 700 / 40;
            for step in 1..=steps {
                let along = step as f32 / steps as f32;
                let at = (
                    from.0 + (to.0 - from.0) * along,
                    from.1 + (to.1 - from.1) * along,
                );
                beats.push(beat(
                    1000 + step * 40,
                    at,
                    Some(Held::Left),
                    Some(Act::Move((at.0 - from.0, at.1 - from.1))),
                ));
            }
            beats.extend([
                beat(1800, to, None, None),
                beat(2300, to, Some(Held::Twice), None),
                beat(3200, to, None, Some(Act::Settings(to))),
                beat(3400, (0.55, 0.85), None, None),
            ]);
            (Kind::Select, beats, 6200)
        }
    }
}

/// What the editor was as the step began: what it is again once the step is over (and before each
/// round of a play).
#[derive(Clone)]
struct Was {
    drawing: Drawing,
    tool: Kind,
    figure: Kind,
    hider: Kind,
    looking: Looking,
    selected: Vec<usize>,
    frame: Option<Area>,
    asked: Option<Panel>,
    afloat: Option<Point<Pixels>>,
    shut: bool,
    by_button: Option<Kind>,
    just_drawn: bool,
}

/// Where the hand is drawn, and what it does.
#[derive(Debug, Clone, Copy)]
pub(super) struct HandNow {
    pub at: Point<Pixels>,
    pub held: Option<Held>,
    pub wheel: Option<Wheel>,
    /// Since when it does that (a press twice, a turn of the wheel).
    pub since: Instant,
}

pub(super) struct Play {
    showing: Showing,
    /// When this round began.
    began: Instant,
    /// What the editor was: `None` while something was under way (nothing is done then).
    was: Option<Was>,
    round: u32,
    /// The beats done this round, and what the first act drew (its place in the list).
    done: usize,
    drawn: Option<usize>,
    /// What was drawn first, as it was when it was selected (it is moved from there).
    held: Option<Edit>,
    hand: Option<HandNow>,
    /// Which play this is: a beat of one that is over does nothing.
    number: u64,
}

impl Editor {
    /// Whether something is under way that the tour's plays would end: then they only show the mouse.
    fn busy(&self) -> bool {
        self.typing.is_some()
            || self.gesture.is_some()
            || !self.selected.is_empty()
            || (self.tool == Kind::Crop && self.frame.is_some())
    }

    fn was_now(&self) -> Was {
        Was {
            drawing: self.drawing.clone(),
            tool: self.tool,
            figure: self.figures.figure,
            hider: self.hiders.kind,
            looking: self.looking,
            selected: self.selected.clone(),
            frame: self.frame,
            asked: self.asked,
            afloat: self.afloat,
            shut: self.shut,
            by_button: self.by_button,
            just_drawn: self.just_drawn,
        }
    }

    /// The editor as it was.
    fn back_to(&mut self, was: Was, cx: &mut Context<Self>) {
        // (Every tool: the select tool's play draws with the shapes' one.)
        for tool in [
            &mut self.pen as &mut dyn crate::tools::Tool,
            &mut self.marker,
            &mut self.figures,
            &mut self.stamps,
            &mut self.hiders,
        ] {
            tool.give_up();
        }
        self.drawing = was.drawing;
        self.shown = Rc::new(self.drawing.edits().to_vec());
        self.tool = was.tool;
        self.figures.figure = was.figure;
        self.hiders.kind = was.hider;
        self.looking = was.looking;
        self.selected = was.selected;
        self.frame = was.frame;
        self.asked = was.asked;
        self.afloat = was.afloat;
        self.shut = was.shut;
        self.by_button = was.by_button;
        self.just_drawn = was.just_drawn;
        self.was = (self.tool, self.chosen());
        self.right_let_go(cx);
        cx.notify();
    }

    /// A tool in hand for the tour: as `hold` takes it, but not remembered as the user's choice
    /// (the way to make a part unreadable the editor starts with stays as it was).
    pub(super) fn tour_hold(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.crop_held(kind);
        self.tool = kind;
        self.pointing = None;
        self.just_drawn = false;
        if kind.is_figure() {
            self.figures.figure = kind;
        }
        if kind.is_hider() {
            self.hiders.kind = kind;
        }
        if kind != Kind::Select {
            self.selected.clear();
        }
        self.asked = None;
        self.by_button = None;
        cx.notify();
    }

    /// The drawing as it was when the step began, while a play draws on it: what the window is told
    /// of (what the tour draws is not the user's).
    pub(super) fn drawing_before_play(&self) -> Option<&[Edit]> {
        self.play
            .as_ref()
            .and_then(|play| play.was.as_ref())
            .map(|was| was.drawing.edits())
    }

    /// What the step shows, set going.
    pub(super) fn start_playing(&mut self, showing: Showing, cx: &mut Context<Self>) {
        if showing == Showing::Nothing || showing == Showing::Choosing {
            return;
        }
        let acting = !self.busy();
        let number = self.plays + 1;
        self.plays = number;
        self.play = Some(Play {
            showing,
            began: Instant::now(),
            was: acting.then(|| self.was_now()),
            round: 0,
            done: 0,
            drawn: None,
            held: None,
            hand: None,
            number,
        });
        self.new_round(cx);
        let moves = perora_core::platform::animations_enabled();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BEAT).await;
                let going = this
                    .update(cx, |editor, cx| editor.beat(number, moves, cx))
                    .unwrap_or(false);
                if !going {
                    return;
                }
            }
        })
        .detach();
    }

    /// What the step set going, stopped; and the editor as it was.
    pub(super) fn stop_playing(&mut self, cx: &mut Context<Self>) {
        if let Some(play) = self.play.take()
            && let Some(was) = play.was
        {
            self.back_to(was, cx);
        }
    }

    /// The hand, where a play has it.
    pub(super) fn hand_view(&self) -> Option<impl IntoElement + use<>> {
        let hand = self.play.as_ref()?.hand?;
        Some(perora_ui::hand::hand(
            hand.at,
            hand.held,
            hand.wheel,
            hand.since,
            perora_core::platform::animations_enabled(),
        ))
    }

    /// A round of the play from its start: from what the editor was, the tool in hand, all of the
    /// picture in sight.
    fn new_round(&mut self, cx: &mut Context<Self>) {
        let Some(play) = self.play.as_mut() else {
            return;
        };
        play.round += 1;
        play.began = Instant::now();
        play.done = 0;
        play.drawn = None;
        play.held = None;
        let (showing, was, round) = (play.showing, play.was.clone(), play.round);
        let Some(was) = was else {
            return;
        };
        if round > 1 {
            self.back_to(was, cx);
        }
        match showing {
            Showing::Tool(played) => {
                let (kind, _, _) = play_of(played);
                self.tour_hold(kind, cx);
                self.looking = Looking::default();
            }
            Showing::Asking => self.tour_hold(Kind::Pen, cx),
            Showing::Looking => self.looking = Looking::default(),
            _ => {}
        }
    }

    /// A beat: what is due by now is done. Whether the play goes on.
    fn beat(&mut self, number: u64, moves: bool, cx: &mut Context<Self>) -> bool {
        let Some(play) = self.play.as_ref() else {
            return false;
        };
        if play.number != number {
            return false;
        }
        let elapsed = play.began.elapsed().as_millis() as u32;
        let acting = play.was.is_some();
        let (done, has_hand) = (play.done, play.hand.is_some());
        match play.showing {
            Showing::Tool(played) => {
                let (_, beats, length) = play_of(played);
                let due: Vec<Beat> = beats
                    .iter()
                    .enumerate()
                    .skip(done)
                    // (With less motion asked for, all of it at once, and left so.)
                    .take_while(|(_, one)| !moves || one.after <= elapsed)
                    .map(|(_, one)| one.clone())
                    .collect();
                for one in &due {
                    if let Some((at, held)) = one.hand
                        && let Some(at) = self.page_of(at)
                    {
                        let since = match (self.play.as_ref().and_then(|play| play.hand), held) {
                            (Some(was), Some(Held::Twice)) if was.held == Some(Held::Twice) => {
                                was.since
                            }
                            _ => Instant::now(),
                        };
                        if let Some(play) = self.play.as_mut() {
                            play.hand = Some(HandNow {
                                at,
                                held,
                                wheel: None,
                                since,
                            });
                        }
                    }
                    if acting && let Some(act) = one.act.clone() {
                        self.act(act, cx);
                    }
                }
                if let Some(play) = self.play.as_mut() {
                    play.done += due.len();
                }
                if !moves {
                    return false;
                }
                if elapsed >= length {
                    self.new_round(cx);
                }
            }
            Showing::Turning => {
                // The other button held, the wheel turned: three tools on, three back, and again.
                let Some(at) = self.page_of((0.35, 0.4)) else {
                    return true;
                };
                let turns = if elapsed < 500 {
                    0
                } else if moves {
                    1 + (elapsed - 500) / 750
                } else {
                    1
                } as usize;
                if turns > done {
                    let by = [1, 1, 1, -1, -1, -1][done % 6];
                    if acting {
                        self.turning.down = true;
                        let event = ScrollEvent {
                            position: at,
                            delta: ScrollDelta::Lines(point(0.0, -1.5 * by as f32)),
                            touch_phase: TouchPhase::Moved,
                            ..Default::default()
                        };
                        self.turned(&event, cx);
                    }
                    if let Some(play) = self.play.as_mut() {
                        // (One turn a beat: a late beat does not skip one of the way.)
                        play.done += 1;
                        play.hand = Some(HandNow {
                            at,
                            held: Some(Held::Right),
                            wheel: Some(if by == 1 { Wheel::Down } else { Wheel::Up }),
                            since: Instant::now(),
                        });
                    }
                } else if let Some(play) = self.play.as_mut()
                    && play.hand.is_none()
                {
                    play.hand = Some(HandNow {
                        at,
                        held: Some(Held::Right),
                        wheel: None,
                        since: Instant::now(),
                    });
                }
                if !moves && turns > 0 {
                    return false;
                }
            }
            Showing::Asking => {
                // The other button pressed twice: the settings of the pen, there; a while later put
                // away, and again.
                let Some(at) = self.page_of((0.22, 0.1)) else {
                    return true;
                };
                if !has_hand {
                    let since = Instant::now();
                    if let Some(play) = self.play.as_mut() {
                        play.hand = Some(HandNow {
                            at,
                            held: Some(Held::Twice),
                            wheel: None,
                            since,
                        });
                    }
                }
                if done == 0 && (elapsed >= 1100 || !moves) {
                    if acting {
                        self.afloat = Some(at);
                        self.asked = Some(Panel::of_stroke(Kind::Pen));
                    }
                    if let Some(play) = self.play.as_mut() {
                        play.done = 1;
                    }
                    if !moves {
                        return false;
                    }
                }
                if moves && elapsed >= 3700 {
                    if acting {
                        self.afloat = None;
                        self.asked = None;
                    }
                    if let Some(play) = self.play.as_mut() {
                        play.began = Instant::now();
                        play.done = 0;
                        play.hand = None;
                    }
                }
            }
            Showing::Looking => {
                // Ten notches in, a rest, ten out, a rest: a notch every 90 ms.
                let Some(at) = self.page_of((0.45, 0.45)) else {
                    return true;
                };
                if !moves {
                    if acting {
                        self.zoom_by(at, 10.0 * 2.4);
                    }
                    if let Some(play) = self.play.as_mut() {
                        play.hand = Some(HandNow {
                            at,
                            held: None,
                            wheel: None,
                            since: Instant::now(),
                        });
                    }
                    return false;
                }
                let notches = (elapsed / 90) as usize;
                while self.play.as_ref().is_some_and(|play| play.done < notches) {
                    let done = self.play.as_ref().map_or(0, |play| play.done);
                    let lines = match done % 36 {
                        0..10 => 2.4,
                        18..28 => -2.4,
                        _ => 0.0,
                    };
                    if acting && lines != 0.0 {
                        self.zoom_by(at, lines);
                    }
                    if let Some(play) = self.play.as_mut() {
                        play.done += 1;
                        let wheel = if lines > 0.0 {
                            Some(Wheel::Up)
                        } else if lines < 0.0 {
                            Some(Wheel::Down)
                        } else {
                            None
                        };
                        let since = match play.hand {
                            Some(hand) if hand.wheel == wheel => hand.since,
                            _ => Instant::now(),
                        };
                        play.hand = Some(HandNow {
                            at,
                            held: None,
                            wheel,
                            since,
                        });
                    }
                }
            }
            Showing::Nothing | Showing::Choosing => return false,
        }
        cx.notify();
        true
    }

    /// One thing a play does, on the picture.
    fn act(&mut self, act: Act, cx: &mut Context<Self>) {
        let Some(size) = self
            .capture
            .as_ref()
            .map(|capture| (capture.picture.width as f32, capture.picture.height as f32))
        else {
            return;
        };
        let place = |at: Part| -> Place { [at.0 * size.0, at.1 * size.1] };
        let scale = self.scale_now();
        let press = |at: Part, editor: &Editor| Press {
            at: place(at),
            scale,
            unit: editor.unit(),
            shift: false,
            next_number: perora_core::drawing::next_number(editor.drawing.edits()),
        };
        match act {
            Act::Begin(at) => {
                let press = press(at, self);
                // (The select tool draws what there is to select as the shapes would.)
                if self.tool == Kind::Select {
                    self.figures.figure = Kind::Rectangle;
                    crate::tools::Tool::begin(&mut self.figures, &press);
                } else if let Some(tool) = self.tool_mut() {
                    tool.begin(&press);
                }
                self.sketch_shaping(cx);
            }
            Act::GoOn(at) => {
                let press = press(at, self);
                if self.tool == Kind::Select {
                    crate::tools::Tool::go_on(&mut self.figures, &press);
                } else if let Some(tool) = self.tool_mut() {
                    tool.go_on(&press);
                }
                self.sketch_shaping(cx);
            }
            Act::Stamp(at) => {
                let press = press(at, self);
                let made = self.tool_mut().and_then(|tool| {
                    tool.begin(&press);
                    tool.end(&press)
                });
                if let Some(edit) = made {
                    let mut edits = self.drawing.edits().to_vec();
                    edits.push(edit);
                    self.sketch(edits, cx);
                }
            }
            Act::Write(at, text) => {
                if let Some(edit) = crate::writing::written(place(at), text, &self.letters) {
                    self.sketch_first(edit, cx);
                }
            }
            Act::Frame(at) => {
                let from = place(at);
                self.frame = Some(Area::new(
                    from[0],
                    from[1],
                    size.0 - from[0],
                    size.1 - from[1],
                ));
                cx.notify();
            }
            Act::Select => {
                if let Some(play) = self.play.as_mut()
                    && let Some(index) = play.drawn
                {
                    play.held = self.drawing.edits().get(index).cloned();
                    self.selected = vec![index];
                }
                cx.notify();
            }
            Act::Move(by) => {
                let Some((index, held)) = self
                    .play
                    .as_ref()
                    .and_then(|play| Some((play.drawn?, play.held.clone()?)))
                else {
                    return;
                };
                let Some(moved) = mark_of(&held).map(|mark| mark.moved(&held, place(by))) else {
                    return;
                };
                let mut edits = self.drawing.edits().to_vec();
                if let Some(edit) = edits.get_mut(index) {
                    *edit = moved;
                }
                self.sketch(edits, cx);
            }
            Act::Settings(at) => {
                if let Some(at) = self.page_of(at) {
                    self.afloat = Some(at);
                    self.asked = None;
                }
                cx.notify();
            }
        }
    }

    /// What the tool in hand is shaping, drawn as what the play drew first.
    fn sketch_shaping(&mut self, cx: &mut Context<Self>) {
        let shaping = if self.tool == Kind::Select {
            crate::tools::Tool::shaping(&self.figures)
        } else {
            self.tool_ref().and_then(|tool| tool.shaping())
        };
        if let Some(edit) = shaping {
            self.sketch_first(edit, cx);
        }
    }

    /// What the play drew first, drawn (or changed to this).
    fn sketch_first(&mut self, edit: Edit, cx: &mut Context<Self>) {
        let mut edits = self.drawing.edits().to_vec();
        let drawn = self.play.as_ref().and_then(|play| play.drawn);
        match drawn {
            Some(index) if index < edits.len() => edits[index] = edit,
            _ => {
                edits.push(edit);
                if let Some(play) = self.play.as_mut() {
                    play.drawn = Some(edits.len() - 1);
                }
            }
        }
        self.sketch(edits, cx);
    }

    /// The drawing as the play has it: shown, and not told to the window (it is given back as it was
    /// once the step is over).
    fn sketch(&mut self, edits: Vec<Edit>, cx: &mut Context<Self>) {
        if self.drawing.change(edits).is_ok() {
            self.shown = Rc::new(self.drawing.edits().to_vec());
        }
        cx.notify();
    }

    /// Closer (`lines` more than 0) or further away, as the wheel would have it with the pointer at
    /// `at`.
    fn zoom_by(&mut self, at: Point<Pixels>, lines: f32) {
        let (Some(stage), Some(view)) = (self.stage.get(), self.view()) else {
            return;
        };
        let size = (stage.size.width.as_f32(), stage.size.height.as_f32());
        if let Some(looking) = crate::view::wheeled(
            size,
            view,
            self.looking,
            self.dense,
            Self::local(stage, at),
            lines,
        ) {
            self.looking = looking;
        }
    }

    /// Where a place of the picture (a share of it across and down) is in the window, as it is shown
    /// now.
    fn page_of(&self, at: Part) -> Option<Point<Pixels>> {
        let capture = self.capture.as_ref()?;
        let stage = self.stage.get()?;
        let view = self.view()?;
        let size = (stage.size.width.as_f32(), stage.size.height.as_f32());
        let look = crate::view::look(size, view, self.looking, self.dense);
        let (x, y) = look.on_screen([
            at.0 * capture.picture.width as f32,
            at.1 * capture.picture.height as f32,
        ]);
        Some(stage.origin + point(px(x), px(y)))
    }
}
