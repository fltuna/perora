//! Cutting the image down (C): the frame it is cut down to, over all of the picture while the tool is
//! in hand (what would be cut away dimmed), pulled by its corners and sides, moved by its inside, a
//! new one drawn from outside it; held to a shape when one is chosen. Enter (or the tick) cuts it
//! down, the turning arrow is all of it again, Esc (or the cross) never mind. One crop in the list,
//! always done last: the other edits keep the places of the whole image. As editor.tsx's `pulled`,
//! `gripAt`, `cut`, `keptOf` and `fitted`.

use gpui::{Context, CursorStyle, Window, fill, point, px, size};
use perora_core::edit::{Edit, Place};

use super::Editor;
use crate::paint::{self, Painter};
use crate::selecting::NEAR;
use crate::tools::Kind;
use crate::view::{Area, Looking};

/// What of the frame a press takes: a corner or a side (-1 the left or top one, 1 the right or bottom
/// one), all of it to move, or nothing of it (a new frame is drawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Grip {
    Side { x: i8, y: i8 },
    Move,
    New,
}

/// The shapes the frame can be held to (none: free), by their names.
pub(super) const RATIOS: [(&str, Option<f32>); 8] = [
    ("free", None),
    ("1:1", Some(1.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("3:2", Some(3.0 / 2.0)),
    ("16:9", Some(16.0 / 9.0)),
    ("3:4", Some(3.0 / 4.0)),
    ("2:3", Some(2.0 / 3.0)),
    ("9:16", Some(9.0 / 16.0)),
];

/// The part of the image the last crop among the edits leaves, in whole pixels; none without one (or
/// with one that leaves nothing).
pub fn kept_of(edits: &[Edit], whole: Area) -> Option<Area> {
    let crop = edits
        .iter()
        .rev()
        .find(|edit| matches!(edit, Edit::Crop { .. }))?;
    let area = crate::marks::box_of(crop);
    let left = area.x.floor().max(0.0);
    let top = area.y.floor().max(0.0);
    let right = area.right().ceil().min(whole.width);
    let bottom = area.bottom().ceil().min(whole.height);
    (right > left && bottom > top).then(|| Area::new(left, top, right - left, bottom - top))
}

/// The largest frame of a shape (as wide to as tall as `ratio`) inside a box, in its middle.
pub(super) fn fitted(area: Area, ratio: f32) -> Area {
    let width = (area.width.min(area.height * ratio)).round().max(1.0);
    let height = area.height.min((width / ratio).round()).max(1.0);
    Area::new(
        area.x + ((area.width - width) / 2.0).round(),
        area.y + ((area.height - height) / 2.0).round(),
        width,
        height,
    )
}

/// What of the frame `was` is at a place, `scale` pixels of the screen to one of the image.
pub(super) fn grip_at(was: Area, at: Place, scale: f32) -> Grip {
    let near = NEAR * 1.5 / scale;
    let (right, bottom) = (was.right(), was.bottom());
    let across = at[0] >= was.x - near && at[0] <= right + near;
    let along = at[1] >= was.y - near && at[1] <= bottom + near;
    let x = if along && (at[0] - was.x).abs() <= near {
        -1
    } else if along && (at[0] - right).abs() <= near {
        1
    } else {
        0
    };
    let y = if across && (at[1] - was.y).abs() <= near {
        -1
    } else if across && (at[1] - bottom).abs() <= near {
        1
    } else {
        0
    };
    let inside = at[0] > was.x && at[0] < right && at[1] > was.y && at[1] < bottom;
    if x != 0 || y != 0 {
        Grip::Side { x, y }
    } else if inside {
        Grip::Move
    } else {
        Grip::New
    }
}

/// The pointer that says what a press would do: the arrows of the way a side or a corner is pulled,
/// moving (an arrow: GPUI on Windows has no pointer of moving), a cross for a new frame.
pub(super) fn pointer_of(grip: Grip) -> CursorStyle {
    match grip {
        Grip::Move => CursorStyle::Arrow,
        Grip::New => CursorStyle::Crosshair,
        Grip::Side { x, y } if x != 0 && y != 0 => {
            if x == y {
                CursorStyle::ResizeUpLeftDownRight
            } else {
                CursorStyle::ResizeUpRightDownLeft
            }
        }
        Grip::Side { x, .. } if x != 0 => CursorStyle::ResizeLeftRight,
        Grip::Side { .. } => CursorStyle::ResizeUpDown,
    }
}

/// The frame `was`, pulled by `grip` from `from` to `at`, within the image `whole`; held to `ratio`
/// when there is one.
pub(super) fn pulled(
    grip: Grip,
    was: Area,
    from: Place,
    at: Place,
    whole: Area,
    ratio: Option<f32>,
) -> Area {
    let within = |value: f32, least: f32, most: f32| value.max(least).min(most.max(least)).round();
    let (across, down) = (at[0] - from[0], at[1] - from[1]);
    if grip == Grip::Move {
        return Area::new(
            within(was.x + across, 0.0, whole.width - was.width),
            within(was.y + down, 0.0, whole.height - was.height),
            was.width,
            was.height,
        );
    }
    if let Some(ratio) = ratio {
        let (right, bottom) = (was.right(), was.bottom());
        // (From an edge of the image there is room only inwards: that way it goes.)
        let inwards = |held: f32, most: f32, asked: bool| {
            if held <= 0.0 {
                true
            } else if held >= most {
                false
            } else {
                asked
            }
        };
        let corner = match grip {
            Grip::New => true,
            Grip::Side { x, y } => x != 0 && y != 0,
            Grip::Move => false,
        };
        if corner {
            let (held, to) = match grip {
                Grip::Side { x, y } => (
                    [
                        if x == 1 { was.x } else { right },
                        if y == 1 { was.y } else { bottom },
                    ],
                    [
                        (if x == 1 { right } else { was.x }) + across,
                        (if y == 1 { bottom } else { was.y }) + down,
                    ],
                ),
                _ => (
                    [
                        within(from[0], 0.0, whole.width),
                        within(from[1], 0.0, whole.height),
                    ],
                    at,
                ),
            };
            let (wide, tall) = (to[0] - held[0], to[1] - held[1]);
            let (gx, gy) = match grip {
                Grip::Side { x, y } => (x, y),
                _ => (0, 0),
            };
            let new = grip == Grip::New;
            let to_right = inwards(
                held[0],
                whole.width,
                if wide == 0.0 && !new {
                    gx == 1
                } else {
                    wide >= 0.0
                },
            );
            let to_bottom = inwards(
                held[1],
                whole.height,
                if tall == 0.0 && !new {
                    gy == 1
                } else {
                    tall >= 0.0
                },
            );
            let room_across = if to_right {
                whole.width - held[0]
            } else {
                held[0]
            };
            let room_down = if to_bottom {
                whole.height - held[1]
            } else {
                held[1]
            };
            let asked = wide.abs().max(tall.abs() * ratio);
            let width = asked
                .min(room_across)
                .min(room_down * ratio)
                .round()
                .max(1.0);
            let height = room_down.min((width / ratio).round()).max(1.0);
            return Area::new(
                within(
                    if to_right { held[0] } else { held[0] - width },
                    0.0,
                    whole.width - width,
                ),
                within(
                    if to_bottom { held[1] } else { held[1] - height },
                    0.0,
                    whole.height - height,
                ),
                width,
                height,
            );
        }
        if let Grip::Side { x, .. } = grip
            && x != 0
        {
            let held = if x == 1 { was.x } else { right };
            let wide = (if x == 1 { right } else { was.x }) + across - held;
            let to_right = inwards(
                held,
                whole.width,
                if wide == 0.0 { x == 1 } else { wide > 0.0 },
            );
            // (Grown past the room above or below its middle, it moves over to where there is room.)
            let middle = was.y + was.height / 2.0;
            let width = wide
                .abs()
                .min(if to_right { whole.width - held } else { held })
                .min(whole.height * ratio)
                .round()
                .max(1.0);
            let height = whole.height.min((width / ratio).round()).max(1.0);
            return Area::new(
                within(
                    if to_right { held } else { held - width },
                    0.0,
                    whole.width - width,
                ),
                within(middle - height / 2.0, 0.0, whole.height - height),
                width,
                height,
            );
        }
        let y = match grip {
            Grip::Side { y, .. } => y,
            _ => 1,
        };
        let held = if y == 1 { was.y } else { bottom };
        let tall = (if y == 1 { bottom } else { was.y }) + down - held;
        let to_bottom = inwards(
            held,
            whole.height,
            if tall == 0.0 { y == 1 } else { tall > 0.0 },
        );
        let middle = was.x + was.width / 2.0;
        let height = tall
            .abs()
            .min(if to_bottom { whole.height - held } else { held })
            .min(whole.width / ratio)
            .round()
            .max(1.0);
        let width = whole.width.min((height * ratio).round()).max(1.0);
        return Area::new(
            within(middle - width / 2.0, 0.0, whole.width - width),
            within(
                if to_bottom { held } else { held - height },
                0.0,
                whole.height - height,
            ),
            width,
            height,
        );
    }
    let (mut left, mut top, mut right, mut bottom) = (was.x, was.y, was.right(), was.bottom());
    match grip {
        Grip::New => (left, top, right, bottom) = (from[0], from[1], at[0], at[1]),
        Grip::Side { x, y } => {
            if x == -1 {
                left += across;
            }
            if x == 1 {
                right += across;
            }
            if y == -1 {
                top += down;
            }
            if y == 1 {
                bottom += down;
            }
        }
        Grip::Move => {}
    }
    // Pulled past the opposite edge, the frame turns over rather than vanish.
    let x = within(left.min(right), 0.0, whole.width - 1.0);
    let y = within(top.min(bottom), 0.0, whole.height - 1.0);
    Area::new(
        x,
        y,
        (within(left.max(right), 0.0, whole.width) - x).max(1.0),
        (within(top.max(bottom), 0.0, whole.height) - y).max(1.0),
    )
}

/// Draws the frame over the picture: what would be cut away dimmed, a white line, and a grip at each
/// corner and side.
pub(super) fn paint_frame(window: &mut Window, painter: &Painter, whole: Area, frame: Area) {
    let dim = gpui::hsla(0.0, 0.0, 0.0, 0.6);
    for part in [
        Area::new(whole.x, whole.y, whole.width, frame.y - whole.y),
        Area::new(
            whole.x,
            frame.bottom(),
            whole.width,
            whole.bottom() - frame.bottom(),
        ),
        Area::new(whole.x, frame.y, frame.x - whole.x, frame.height),
        Area::new(
            frame.right(),
            frame.y,
            whole.right() - frame.right(),
            frame.height,
        ),
    ] {
        if part.width > 0.0 && part.height > 0.0 {
            window.paint_quad(fill(painter.bounds(part), dim));
        }
    }
    let bounds = painter.bounds(frame);
    paint::rectangle_line(window, bounds, 1.5, None, gpui::white());
    let side = px(9.0);
    for x in [0.0, 0.5, 1.0] {
        for y in [0.0, 0.5, 1.0] {
            if x == 0.5 && y == 0.5 {
                continue;
            }
            let at = painter.at([frame.x + frame.width * x, frame.y + frame.height * y]);
            window.paint_quad(
                fill(
                    gpui::Bounds::new(
                        point(at.x - side / 2.0, at.y - side / 2.0),
                        size(side, side),
                    ),
                    gpui::white(),
                )
                .border_widths(px(1.0))
                .border_color(gpui::hsla(0.0, 0.0, 0.0, 0.7)),
            );
        }
    }
}

impl Editor {
    /// The whole image.
    pub(super) fn whole(&self) -> Option<Area> {
        self.capture.as_ref().map(|capture| capture.picture.whole())
    }

    /// The image is cut down to the frame as it is set, and the tool put down: one step to undo
    /// (all of it is no crop).
    pub(super) fn cut(&mut self, cx: &mut Context<Self>) {
        if self.tool != Kind::Crop || self.frozen {
            return;
        }
        let Some(whole) = self.whole() else {
            return;
        };
        let kept = self.frame.unwrap_or(whole);
        let mut next: Vec<Edit> = self
            .drawing
            .edits()
            .iter()
            .filter(|edit| !matches!(edit, Edit::Crop { .. }))
            .cloned()
            .collect();
        if kept.width < whole.width || kept.height < whole.height {
            next.push(Edit::Crop {
                from: [kept.x, kept.y],
                to: [kept.right(), kept.bottom()],
            });
        }
        if next.as_slice() != self.drawing.edits() && perora_core::drawing::fits(&next) {
            self.change(next, cx);
        }
        self.selected.clear();
        self.by_button = None;
        self.hold(Kind::Select, cx);
    }

    /// The frame is all of the image again (of no shape).
    pub(super) fn crop_whole(&mut self, cx: &mut Context<Self>) {
        self.ratio = None;
        self.frame = self.whole();
        cx.notify();
    }

    /// A shape for the frame: the largest frame of it inside the frame as it is.
    pub(super) fn crop_ratio(&mut self, ratio: Option<f32>, cx: &mut Context<Self>) {
        if let (Some(ratio), Some(whole)) = (ratio, self.whole())
            && ratio != self.ratio.unwrap_or(0.0)
        {
            self.frame = Some(fitted(self.frame.unwrap_or(whole), ratio));
        }
        self.ratio = ratio;
        self.asked = None;
        cx.notify();
    }

    /// The tool is taken up or put down: the frame starts from what the image is cut down to
    /// already (or all of it), and all of it is looked at.
    pub(super) fn crop_held(&mut self, kind: Kind) {
        let was = self.tool == Kind::Crop;
        self.frame = if kind == Kind::Crop {
            self.whole()
                .map(|whole| kept_of(self.drawing.edits(), whole).unwrap_or(whole))
        } else {
            None
        };
        self.ratio = None;
        if was != (kind == Kind::Crop) {
            self.looking = Looking::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHOLE: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 500.0,
    };

    #[test]
    fn a_frame_is_pulled_moved_and_drawn_within_the_image() {
        let was = Area::new(100.0, 100.0, 200.0, 100.0);
        let pulled_right = pulled(
            Grip::Side { x: 1, y: 0 },
            was,
            [300.0, 150.0],
            [350.4, 150.0],
            WHOLE,
            None,
        );
        assert_eq!((pulled_right.x, pulled_right.width), (100.0, 250.0));
        let moved = pulled(
            Grip::Move,
            was,
            [150.0, 150.0],
            [2000.0, 150.0],
            WHOLE,
            None,
        );
        assert_eq!((moved.x, moved.width), (800.0, 200.0));
        let new = pulled(Grip::New, was, [900.0, 400.0], [800.0, 300.0], WHOLE, None);
        assert_eq!(
            (new.x, new.y, new.width, new.height),
            (800.0, 300.0, 100.0, 100.0)
        );
    }

    #[test]
    fn a_frame_held_to_a_shape_keeps_it() {
        let was = Area::new(100.0, 100.0, 200.0, 100.0);
        let square = pulled(
            Grip::Side { x: 1, y: 1 },
            was,
            [300.0, 200.0],
            [400.0, 220.0],
            WHOLE,
            Some(1.0),
        );
        assert_eq!(square.width, square.height);
        assert_eq!((square.x, square.y), (100.0, 100.0));
        // No larger than the image has room for.
        let large = pulled(
            Grip::New,
            was,
            [0.0, 0.0],
            [5000.0, 5000.0],
            WHOLE,
            Some(16.0 / 9.0),
        );
        assert!(large.height <= 500.0 && (large.width / large.height - 16.0 / 9.0).abs() < 0.01);
        assert_eq!(fitted(WHOLE, 1.0), Area::new(250.0, 0.0, 500.0, 500.0));
    }

    #[test]
    fn the_last_crop_says_what_is_kept() {
        let edits = [Edit::Crop {
            from: [10.4, 20.6],
            to: [100.2, 80.0],
        }];
        assert_eq!(
            kept_of(&edits, WHOLE),
            Some(Area::new(10.0, 20.0, 91.0, 60.0))
        );
        assert_eq!(kept_of(&[], WHOLE), None);
        assert_eq!(
            grip_at(Area::new(100.0, 100.0, 200.0, 100.0), [300.0, 150.0], 1.0),
            Grip::Side { x: 1, y: 0 }
        );
        assert_eq!(
            grip_at(Area::new(100.0, 100.0, 200.0, 100.0), [200.0, 150.0], 1.0),
            Grip::Move
        );
        assert_eq!(
            grip_at(Area::new(100.0, 100.0, 200.0, 100.0), [500.0, 400.0], 1.0),
            Grip::New
        );
    }
}
