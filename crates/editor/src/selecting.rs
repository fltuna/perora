//! Selecting what was drawn, whatever kind it is (the editor's, not a tool's): what a press with the
//! select tool means, which hold of the one selected it takes, what a part marked out takes. The
//! rules are `editor.tsx`'s (`onDown` for the select tool, `holdAt`, `seenAt`, the marquee in `onUp`).
//!
//! Click is a thing, drag is a part (owner: 「クリックなら対象を、ドラッグなら範囲を」); Ctrl and a click
//! adds or takes away (「CTRL + クリックがいいかな複数選択系は」).

use perora_core::edit::{Edit, Place};

use crate::marks::{self, Hold, mark_of};
use crate::view::Area;

/// How far from a thing a press still means it, in pixels of the screen.
pub const NEAR: f32 = 6.0;
/// Smaller than this on the screen, a thing pressed on is moved, not made larger: its holds lie on it.
pub const SMALL: f32 = 24.0;
/// A part marked out less than this across (pixels of the screen) was a press, not a part.
pub const LEAST_MARKED: f32 = 4.0;

/// Whether a press at `at` means this edit, where the picture is (`view`, `near` around it): beside
/// it, what is drawn is cut off, and no press there is on it.
pub fn seen_at(edit: &Edit, at: Place, near: f32, view: &Area) -> bool {
    at[0] >= view.x - near
        && at[0] <= view.right() + near
        && at[1] >= view.y - near
        && at[1] <= view.bottom() + near
        && mark_of(edit).is_some_and(|mark| mark.is_at(edit, at, near))
}

/// The edit on top of those a press at `at` means.
pub fn topmost_at(edits: &[Edit], at: Place, near: f32, view: &Area) -> Option<usize> {
    (0..edits.len())
        .rev()
        .find(|&index| seen_at(&edits[index], at, near, view))
}

/// Whether `at` is in the frame around an edit (that shows it selected).
pub fn in_frame(edit: &Edit, at: Place, scale: f32) -> bool {
    let (area, around) = (marks::box_of(edit), marks::frame_around(edit, scale));
    at[0] >= area.x - around
        && at[0] <= area.right() + around
        && at[1] >= area.y - around
        && at[1] <= area.bottom() + around
}

/// The hold of a selected edit a press at `at` takes: the nearest of them, when the press is on one.
/// Not on a thing so small that its holds lie on it: pressed on, it is taken hold of whole.
pub fn hold_at(edit: &Edit, at: Place, scale: f32) -> Option<(Hold, Place)> {
    let mark = mark_of(edit)?;
    let area = marks::box_of(edit);
    if area.width.max(area.height) * scale < SMALL && mark.is_at(edit, at, 0.0) {
        return None;
    }
    let far = |place: &Place| (place[0] - at[0]).hypot(place[1] - at[1]);
    mark.holds(edit, marks::frame_around(edit, scale))
        .into_iter()
        .filter(|(_, place)| far(place) <= NEAR / scale)
        .min_by(|a, b| far(&a.1).total_cmp(&far(&b.1)))
}

/// What a press with the select tool begins.
#[derive(Debug, Clone, PartialEq)]
pub enum Pressed {
    /// These are selected now (Ctrl and a click: one added or taken away).
    Selection(Vec<usize>),
    /// A part is marked out from there; what is wholly in it is selected along with these.
    Marking { with: Vec<usize> },
    /// These are taken hold of and moved (one pressed on is selected first).
    Moving(Vec<usize>),
    /// The one selected is taken hold of by a hold, to change its size.
    Resizing(usize, Hold),
}

/// What a press at `at` begins with the select tool, `selected` being selected (one, or several
/// together), `scale` pixels of the screen to one of the image, the picture `view`.
pub fn press(
    edits: &[Edit],
    selected: &[usize],
    at: Place,
    scale: f32,
    view: &Area,
    ctrl: bool,
) -> Pressed {
    let near = NEAR / scale;
    let seen = |index: usize| {
        edits
            .get(index)
            .is_some_and(|edit| seen_at(edit, at, near, view))
    };
    let nothing_seen = || topmost_at(edits, at, near, view).is_none();
    // With Ctrl: the one on top under the pointer is selected along with the rest, or no longer if
    // it was. Where there is none, what is selected stays, and what is marked out is added.
    if ctrl {
        return match topmost_at(edits, at, near, view) {
            None => Pressed::Marking {
                with: selected.to_vec(),
            },
            Some(index) => {
                let mut those: Vec<usize> = selected
                    .iter()
                    .copied()
                    .filter(|&one| one != index)
                    .collect();
                if those.len() == selected.len() {
                    those.push(index);
                    those.sort_unstable();
                }
                Pressed::Selection(those)
            }
        };
    }
    // On one of those selected together, or anywhere in the frame of one: all of them are moved.
    // (What else is seen in their frames is still there to be pressed on.)
    if selected.len() > 1 {
        let framed = selected.iter().any(|&index| {
            edits
                .get(index)
                .is_some_and(|edit| in_frame(edit, at, scale))
        });
        if framed && (selected.iter().any(|&index| seen(index)) || nothing_seen()) {
            return Pressed::Moving(selected.to_vec());
        }
    }
    // On a hold of the one selected: that is pulled. Anywhere in its frame where nothing else is:
    // it is moved (its frame is what there is to take hold of it by: a stroke is thin).
    if let [index] = selected
        && let Some(edit) = edits.get(*index)
    {
        if let Some((hold, _)) = hold_at(edit, at, scale) {
            return Pressed::Resizing(*index, hold);
        }
        if in_frame(edit, at, scale) && (seen(*index) || nothing_seen()) {
            return Pressed::Moving(vec![*index]);
        }
    }
    // The one on top under the pointer: selected, and moved if the pointer goes on. Where there is
    // none: nothing is selected any more, and what is marked out will be.
    match topmost_at(edits, at, near, view) {
        Some(index) => Pressed::Moving(vec![index]),
        None => Pressed::Marking { with: Vec::new() },
    }
}

/// What a part marked out from `from` to `to` takes: what is wholly in it and in sight (`in_sight`
/// says of a box), along with `with`; sorted, each once.
pub fn marked(
    edits: &[Edit],
    from: Place,
    to: Place,
    with: &[usize],
    in_sight: impl Fn(&Area) -> bool,
) -> Vec<usize> {
    let (left, top) = (from[0].min(to[0]), from[1].min(to[1]));
    let (right, bottom) = (from[0].max(to[0]), from[1].max(to[1]));
    let mut those: Vec<usize> = with.to_vec();
    for (index, edit) in edits.iter().enumerate() {
        if mark_of(edit).is_none() || matches!(edit, Edit::Crop { .. }) {
            continue;
        }
        let area = marks::box_of(edit);
        if in_sight(&area)
            && area.x >= left
            && area.y >= top
            && area.right() <= right
            && area.bottom() <= bottom
        {
            those.push(index);
        }
    }
    those.sort_unstable();
    those.dedup();
    those
}

#[cfg(test)]
mod tests {
    use super::*;
    use perora_core::edit::Colour;

    fn pen(points: &[Place]) -> Edit {
        Edit::Pen {
            points: points.to_vec(),
            color: Colour {
                red: 0,
                green: 0,
                blue: 0,
            },
            width: 2.0,
        }
    }

    const VIEW: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 1000.0,
    };

    fn edits() -> Vec<Edit> {
        vec![
            pen(&[[100.0, 100.0], [200.0, 100.0]]),
            pen(&[[150.0, 50.0], [150.0, 150.0]]),
            pen(&[[500.0, 500.0], [600.0, 600.0]]),
        ]
    }

    #[test]
    fn a_click_is_the_thing_on_top_and_a_drag_from_nothing_is_a_part() {
        let edits = edits();
        // Where the two cross, the later one.
        assert_eq!(
            press(&edits, &[], [150.0, 100.0], 1.0, &VIEW, false),
            Pressed::Moving(vec![1])
        );
        assert_eq!(
            press(&edits, &[], [120.0, 103.0], 1.0, &VIEW, false),
            Pressed::Moving(vec![0])
        );
        assert_eq!(
            press(&edits, &[], [300.0, 300.0], 1.0, &VIEW, false),
            Pressed::Marking { with: vec![] }
        );
        // Beside the picture, nothing is pressed on.
        let small = Area::new(0.0, 0.0, 120.0, 120.0);
        assert_eq!(
            press(&edits, &[], [150.0, 100.0], 1.0, &small, false),
            Pressed::Marking { with: vec![] }
        );
    }

    #[test]
    fn ctrl_adds_and_takes_away() {
        let edits = edits();
        assert_eq!(
            press(&edits, &[0], [550.0, 550.0], 1.0, &VIEW, true),
            Pressed::Selection(vec![0, 2])
        );
        assert_eq!(
            press(&edits, &[0, 2], [550.0, 550.0], 1.0, &VIEW, true),
            Pressed::Selection(vec![0])
        );
        assert_eq!(
            press(&edits, &[0], [300.0, 300.0], 1.0, &VIEW, true),
            Pressed::Marking { with: vec![0] }
        );
    }

    #[test]
    fn the_one_selected_is_taken_hold_of_anywhere_in_its_frame() {
        let edits = edits();
        // In the frame of the diagonal, beside its line, where nothing else is.
        assert_eq!(
            press(&edits, &[2], [590.0, 510.0], 1.0, &VIEW, false),
            Pressed::Moving(vec![2])
        );
        // Not selected: that press is on nothing.
        assert_eq!(
            press(&edits, &[], [590.0, 510.0], 1.0, &VIEW, false),
            Pressed::Marking { with: vec![] }
        );
        // Several together: in the frame of any of them, all are moved.
        assert_eq!(
            press(&edits, &[0, 2], [590.0, 510.0], 1.0, &VIEW, false),
            Pressed::Moving(vec![0, 2])
        );
    }

    #[test]
    fn a_part_takes_what_is_wholly_in_it_and_in_sight() {
        let edits = edits();
        let all = |_: &Area| true;
        assert_eq!(
            marked(&edits, [90.0, 40.0], [210.0, 160.0], &[], all),
            vec![0, 1]
        );
        // Only crossed: not.
        assert_eq!(
            marked(&edits, [90.0, 90.0], [210.0, 110.0], &[], all),
            vec![0]
        );
        assert_eq!(
            marked(&edits, [90.0, 90.0], [210.0, 110.0], &[2, 0], all),
            vec![0, 2]
        );
        let none = |_: &Area| false;
        assert!(marked(&edits, [0.0, 0.0], [999.0, 999.0], &[], none).is_empty());
    }
}
