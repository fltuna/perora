//! What is selected, copied and put on the picture again (owner: 「CTRL + Cはクリップボードへの
//! コピーじゃなくて、要素のコピペにしたいかも」): Ctrl+C copies it, Ctrl+X takes it out, Ctrl+V puts
//! what was copied beside where it was (16 pixels of the screen further each time), on top of
//! everything, selected; Ctrl+D puts what is selected beside itself once more. Kept here and not on
//! the clipboard (which has the capture), for this capture only. A number put down again is the
//! next number (「番号は次の番号にしたいかもね」); one taken out and put back is the number it was.

use gpui::Context;
use perora_core::edit::Edit;

use super::Editor;
use crate::marks::mark_of;
use crate::tools::Kind;

/// What was copied, and how many steps beside it the next is put (0: where it was, taken out).
#[derive(Default)]
pub(super) struct Copied {
    edits: Vec<Edit>,
    times: u32,
}

impl Editor {
    /// What is selected (one, or several together).
    fn picked(&self) -> Vec<Edit> {
        self.selected
            .iter()
            .filter_map(|index| self.drawing.edits().get(*index).cloned())
            .collect()
    }

    /// Those put on the picture, `times` steps beside where they are. Whether they were (not when
    /// the drawing holds no more).
    fn put(&mut self, those: &[Edit], times: u32, cx: &mut Context<Self>) -> bool {
        if those.is_empty() {
            return false;
        }
        let scale = self.scale_now();
        let by = (16.0 / scale).round().max(1.0) * times as f32;
        let edits = self.drawing.edits().to_vec();
        let mut numbers: Vec<u32> = edits
            .iter()
            .filter_map(|edit| match edit {
                Edit::Stamp { number, .. } => Some(*number),
                _ => None,
            })
            .collect();
        let mut next = edits.clone();
        for edit in those {
            let mut one = mark_of(edit).map_or(edit.clone(), |mark| mark.moved(edit, [by, by]));
            if let Edit::Stamp { number, .. } = &mut one {
                if numbers.contains(number) {
                    *number = numbers.iter().max().copied().unwrap_or(0) + 1;
                }
                numbers.push(*number);
            }
            next.push(one);
        }
        if !perora_core::drawing::fits(&next) {
            self.full = true;
            cx.notify();
            return false;
        }
        if self.tool != Kind::Select {
            self.by_button = None;
            self.hold(Kind::Select, cx);
        }
        let start = edits.len();
        self.change(next, cx);
        self.selected = (start..start + those.len()).collect();
        cx.notify();
        true
    }

    pub(super) fn copy_selected(&mut self) {
        let those = self.picked();
        if !those.is_empty() {
            self.copied = Copied {
                edits: those,
                times: 1,
            };
        }
    }

    /// Copied and taken away: put back, the first time, where it was.
    pub(super) fn take_out(&mut self, cx: &mut Context<Self>) {
        let those = self.picked();
        if those.is_empty() {
            return;
        }
        self.copied = Copied {
            edits: those,
            times: 0,
        };
        self.remove(cx);
    }

    pub(super) fn paste(&mut self, cx: &mut Context<Self>) {
        let those = self.copied.edits.clone();
        if those.is_empty() {
            return;
        }
        // (Where it was only if it is no longer there: brought back by undoing, it would lie on
        // itself, and nothing would be seen of it.)
        let there = those.iter().any(|one| self.drawing.edits().contains(one));
        let from = if self.copied.times == 0 && there {
            1
        } else {
            self.copied.times
        };
        if self.put(&those, from, cx) {
            self.copied.times = from + 1;
        }
    }

    pub(super) fn duplicate(&mut self, cx: &mut Context<Self>) {
        let those = self.picked();
        self.put(&those, 1, cx);
    }
}
