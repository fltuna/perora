//! Writing on the picture (the text tool): where it was pressed, a field comes, the middle of its
//! first line at the press; what is typed is seen as it will be written, at the size the picture is
//! looked at. The system's input puts letters together in it (Japanese as anywhere). Enter is done
//! with it, Shift+Enter a new line, Esc gives it up (what was written before stays as it was);
//! pressing elsewhere or taking another tool is done with it too. What was written before is written
//! anew by pressing it with the text tool, or twice with the select tool.
//!
//! Around the field, a band of 9 pixels takes hold of it: dragged, what is being written goes along
//! (at whole pixels, within the part that is shown); pressed and let go where it is, it is held as
//! a thing (framed in the accent, no caret), and Delete takes all of it away. The keyboard stays the
//! field's meanwhile: the first key typed into a field just given the keyboard would be a letter of
//! its own where letters are put together from several (owner: 「最初の枠押しはローマ字になっちゃう
//! ね。」).

use std::ops::Range;

use gpui::{
    Bounds, Context, EntityInputHandler, Hsla, KeyDownEvent, Pixels, Point, UTF16Selection, Window,
    fill, point, px, size,
};
use perora_core::edit::{Edit, Place};

use unicode_segmentation::UnicodeSegmentation;

use super::Editor;
use super::bars::Panel;
use crate::paint::{self, Painter};
use crate::writing::{self, Writing, laid_out, written};

/// How wide the band around the field is, on the screen.
pub(super) const BAND: f32 = 9.0;
/// A press on the band that moves less than this (screen pixels) holds what is written as a thing.
const STILL: f32 = 4.0;
/// How many changes of what is being written can be undone.
const UNDONE: usize = 100;

/// What a key is while something is being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Keyed {
    /// The writing's, done with here.
    Taken,
    /// A letter: through to the system's input, which brings it (a key taken here brings none).
    Letter,
    /// The capture's (Ctrl+S, and the rest): for the editor's keys.
    NotMine,
}

/// What is being written.
pub(super) struct Typing {
    /// Which of the edits it is, written anew (none: new).
    pub index: Option<usize>,
    /// The top-left corner of its room, in pixels of the image.
    pub at: Place,
    pub text: String,
    /// What is selected of it (bytes), the caret at its end (or its start, `reversed`).
    pub selected: Range<usize>,
    pub reversed: bool,
    /// What the system's input is putting together, not yet typed.
    pub marked: Option<Range<usize>>,
    /// Held as a thing, by its band.
    pub held: bool,
    /// The text as it was before each change, and after each undone.
    past: Vec<(String, Range<usize>)>,
    future: Vec<(String, Range<usize>)>,
    /// The band is held by the pointer: from where on the screen, where it was, whether it went far.
    pub carrying: Option<(Point<Pixels>, Place, bool)>,
    /// The pointer is selecting letters in the field.
    pub selecting: bool,
    /// Where the letters typed one after another end: one more there is the same step to undo.
    run_end: Option<usize>,
}

impl Typing {
    fn new(index: Option<usize>, at: Place, text: String) -> Self {
        let end = text.len();
        Self {
            index,
            at,
            text,
            selected: end..end,
            reversed: false,
            marked: None,
            held: false,
            past: Vec::new(),
            future: Vec::new(),
            carrying: None,
            selecting: false,
            run_end: None,
        }
    }

    fn caret(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    /// The text changed by the user: the change before it can be undone.
    fn remember(&mut self) {
        self.past.push((self.text.clone(), self.selected.clone()));
        if self.past.len() > UNDONE {
            self.past.remove(0);
        }
        self.future.clear();
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.remember();
        self.text.replace_range(range.clone(), with);
        let end = range.start + with.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        self.run_end = None;
    }

    /// Letters typed (or put together by the system's input, `composed`): with the letters typed
    /// just before them, and with what they were put together from, one step to undo (as a field
    /// of the browser had it).
    fn type_in(&mut self, range: Range<usize>, with: &str, composed: bool) {
        let goes_on = composed || (range.is_empty() && self.run_end == Some(range.start));
        if !goes_on {
            self.remember();
        }
        self.text.replace_range(range.clone(), with);
        let end = range.start + with.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = None;
        self.run_end = Some(end);
    }

    fn move_to(&mut self, at: usize, extend: bool) {
        if extend {
            let anchor = if self.reversed {
                self.selected.end
            } else {
                self.selected.start
            };
            if at < anchor {
                self.selected = at..anchor;
                self.reversed = true;
            } else {
                self.selected = anchor..at;
                self.reversed = false;
            }
        } else {
            self.selected = at..at;
            self.reversed = false;
        }
    }

    fn before(&self, at: usize) -> usize {
        self.text[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn after(&self, at: usize) -> usize {
        self.text[at..]
            .graphemes(true)
            .next()
            .map_or(at, |letter| at + letter.len())
    }

    /// Where the words begin (what is not space between them).
    fn word_starts(&self) -> impl Iterator<Item = usize> + '_ {
        self.text
            .split_word_bound_indices()
            .filter(|(_, part)| !part.trim().is_empty())
            .map(|(index, _)| index)
    }

    /// The beginning of the word before `at` (of the one it is in): what Ctrl+← goes to.
    fn word_before(&self, at: usize) -> usize {
        self.word_starts()
            .take_while(|&start| start < at)
            .last()
            .unwrap_or(0)
    }

    /// The beginning of the word after `at`, or the end: what Ctrl+→ goes to.
    fn word_after(&self, at: usize) -> usize {
        self.word_starts()
            .find(|&start| start > at)
            .unwrap_or(self.text.len())
    }

    /// The word (or the space between words) at `at`.
    fn word_at(&self, at: usize) -> Range<usize> {
        self.text
            .split_word_bound_indices()
            .map(|(index, part)| index..index + part.len())
            .find(|word| word.contains(&at) || (word.end == at && at == self.text.len()))
            .unwrap_or(at..at)
    }

    /// The line a byte is on, and where that line begins.
    fn line_of(&self, at: usize) -> (usize, usize) {
        let before = &self.text[..at];
        let line = before.matches('\n').count();
        let start = before.rfind('\n').map_or(0, |index| index + 1);
        (line, start)
    }

    fn line_range(&self, line: usize) -> Range<usize> {
        let mut start = 0;
        for (index, part) in self.text.split('\n').enumerate() {
            if index == line {
                return start..start + part.len();
            }
            start += part.len() + 1;
        }
        self.text.len()..self.text.len()
    }

    fn to_utf16(&self, at: usize) -> usize {
        self.text[..at.min(self.text.len())].encode_utf16().count()
    }

    fn byte_of_utf16(&self, at: usize) -> usize {
        let mut units = 0;
        for (index, letter) in self.text.char_indices() {
            if units >= at {
                return index;
            }
            units += letter.len_utf16();
        }
        self.text.len()
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.byte_of_utf16(range.start)..self.byte_of_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.to_utf16(range.start)..self.to_utf16(range.end)
    }
}

impl Editor {
    /// Writing starts where the picture was pressed: the middle of its first line there.
    pub(super) fn start_typing(&mut self, at: Place, cx: &mut Context<Self>) {
        let layout = laid_out("", &self.letters);
        self.selected.clear();
        self.typing = Some(Typing::new(
            None,
            [
                (at[0] - layout.around).round(),
                (at[1] - layout.around - layout.line / 2.0).round(),
            ],
            String::new(),
        ));
        cx.notify();
    }

    /// One of the edits, written before, is written anew: in its place, as it is written.
    pub(super) fn rewrite(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(edit @ Edit::Text { from, text, .. }) = self.drawing.edits().get(index) else {
            return;
        };
        let Some(how) = writing::how_of(edit) else {
            return;
        };
        self.letters = how;
        self.selected.clear();
        self.typing = Some(Typing::new(Some(index), *from, text.clone()));
        cx.notify();
    }

    /// What is being written is done: one of the edits now (or, with nothing written, none of them
    /// any more). False when it could not be done (too large for a picture, or a drawing that
    /// holds no more): it is still being written, and the user is told.
    pub(super) fn finish_typing(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(typing) = self.typing.as_ref() else {
            return true;
        };
        let blank = typing.text.trim().is_empty();
        let one = if blank {
            None
        } else {
            written(typing.at, &typing.text, &self.letters)
        };
        if !blank && one.is_none() {
            self.full = true;
            cx.notify();
            return false;
        }
        let edits = self.drawing.edits();
        let next: Vec<Edit> = match (typing.index, one) {
            (None, Some(one)) => edits.iter().cloned().chain([one]).collect(),
            (None, None) => edits.to_vec(),
            (Some(index), Some(one)) => edits
                .iter()
                .enumerate()
                .map(|(at, edit)| {
                    if at == index {
                        one.clone()
                    } else {
                        edit.clone()
                    }
                })
                .collect(),
            (Some(index), None) => edits
                .iter()
                .enumerate()
                .filter(|(at, _)| *at != index)
                .map(|(_, edit)| edit.clone())
                .collect(),
        };
        if !perora_core::drawing::fits(&next) {
            self.full = true;
            cx.notify();
            return false;
        }
        let new = typing.index.is_none();
        self.typing = None;
        self.letters_put_away();
        if next.as_slice() != self.drawing.edits() {
            self.change(next, cx);
            self.just_drawn = new;
        }
        cx.notify();
        true
    }

    /// What is being written is given up: what was written before stays as it was.
    pub(super) fn give_up_typing(&mut self, cx: &mut Context<Self>) {
        if self.typing.take().is_some() {
            self.letters_put_away();
            cx.notify();
        }
    }

    /// What opens from the box by what is written is put away with it (opened again, the box
    /// starts with none open, as the Tauri version's did).
    fn letters_put_away(&mut self) {
        if self.asked.is_some_and(Panel::of_letters) {
            self.asked = None;
        }
    }

    /// The room of what is being written, in pixels of the image (the field: at least 0.6 of a
    /// letter wide inside, as the Tauri version's field was).
    pub(super) fn typing_room(&self) -> Option<crate::view::Area> {
        let typing = self.typing.as_ref()?;
        let layout = laid_out(&typing.text, &self.letters);
        let width = layout
            .width
            .max(layout.around * 2.0 + self.letters.size * 0.6);
        Some(crate::view::Area::new(
            typing.at[0],
            typing.at[1],
            width,
            layout.height,
        ))
    }

    /// Where a byte of what is being written is: across the image, and the middle of its line.
    fn place_of(&self, at: usize) -> Option<Place> {
        let typing = self.typing.as_ref()?;
        let layout = laid_out(&typing.text, &self.letters);
        let (line, start) = typing.line_of(at);
        let units = typing.text[start..at].encode_utf16().count();
        let x = layout
            .lettered
            .lines
            .get(line)
            .and_then(|line| line.carets.get(units).copied())
            .unwrap_or(0.0);
        Some([
            typing.at[0] + layout.around + x,
            typing.at[1] + layout.middle_of(line),
        ])
    }

    /// The byte of what is being written nearest a place of the image.
    pub(super) fn index_at(&self, at: Place) -> Option<usize> {
        let typing = self.typing.as_ref()?;
        let layout = laid_out(&typing.text, &self.letters);
        let lines = typing.text.split('\n').count();
        let line = ((at[1] - typing.at[1] - layout.around) / layout.line)
            .floor()
            .clamp(0.0, (lines - 1) as f32) as usize;
        let x = at[0] - typing.at[0] - layout.around;
        let range = typing.line_range(line);
        let carets = layout
            .lettered
            .lines
            .get(line)
            .map(|line| line.carets.clone())
            .unwrap_or_default();
        // The caret nearest across, by the UTF-16 unit; back to a byte of the line.
        let units = carets
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |(units, _)| units);
        let mut counted = 0;
        for (index, letter) in typing.text[range.clone()].char_indices() {
            if counted >= units {
                return Some(range.start + index);
            }
            counted += letter.len_utf16();
        }
        Some(range.end)
    }

    /// A key while something is being written (with the keyboard in the editor). Whether it was the
    /// writing's: every key is, but those for the capture as a whole (Ctrl+S, Ctrl+C with nothing
    /// selected of the text, and those the window has).
    pub(super) fn typing_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Keyed {
        let Some(typing) = self.typing.as_mut() else {
            return Keyed::NotMine;
        };
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let alone = !modifiers.control && !modifiers.alt && !modifiers.platform;
        // (Keys the system's input is putting letters together with are its own.)
        if typing.marked.is_some() {
            return Keyed::Letter;
        }
        if key == "escape" {
            if !event.is_held {
                self.give_up_typing(cx);
            }
            return Keyed::Taken;
        }
        if key == "enter" {
            if modifiers.shift && alone {
                let range = typing.selected.clone();
                typing.replace(range, "\n");
                typing.held = false;
                cx.notify();
            } else if !event.is_held && !modifiers.alt {
                // Enter, and Ctrl+Enter: done (not leaving: Ctrl+Enter leaves nothing meanwhile).
                self.finish_typing(cx);
            }
            return Keyed::Taken;
        }
        // Held as a thing: Delete takes all of it away (what was written before and is written
        // anew too). Another key, and it is being written again.
        if typing.held {
            if matches!(key, "delete" | "backspace") && alone && !modifiers.shift {
                let index = typing.index;
                self.typing = None;
                self.letters_put_away();
                if let Some(index) = index {
                    let edits = self
                        .drawing
                        .edits()
                        .iter()
                        .enumerate()
                        .filter(|(at, _)| *at != index)
                        .map(|(_, edit)| edit.clone())
                        .collect();
                    self.change(edits, cx);
                }
                self.just_drawn = false;
                cx.notify();
                return Keyed::Taken;
            }
            if !matches!(key, "shift" | "control" | "alt" | "platform" | "capslock") {
                typing.held = false;
                cx.notify();
            }
        }
        let shift = modifiers.shift;
        let caret = typing.caret();
        // (With Ctrl: by words, and to the very beginning and end.)
        let word = modifiers.control && !modifiers.alt && !modifiers.platform;
        let moved = match key {
            "left" if word => Some(typing.word_before(caret)),
            "right" if word => Some(typing.word_after(caret)),
            "home" if word => Some(0),
            "end" if word => Some(typing.text.len()),
            "left" if alone => Some(if typing.selected.is_empty() || shift {
                typing.before(caret)
            } else {
                typing.selected.start
            }),
            "right" if alone => Some(if typing.selected.is_empty() || shift {
                typing.after(caret)
            } else {
                typing.selected.end
            }),
            "home" if alone => Some(typing.line_of(caret).1),
            "end" if alone => {
                let (line, _) = typing.line_of(caret);
                Some(typing.line_range(line).end)
            }
            "up" | "down" if alone => {
                let (line, _) = typing.line_of(caret);
                let lines = typing.text.split('\n').count();
                let to = if key == "up" {
                    line.checked_sub(1)
                } else {
                    (line + 1 < lines).then_some(line + 1)
                };
                match (to, self.place_of(caret)) {
                    (Some(to), Some(place)) => {
                        let layout = laid_out(
                            &self
                                .typing
                                .as_ref()
                                .map_or(String::new(), |t| t.text.clone()),
                            &self.letters,
                        );
                        let y =
                            self.typing.as_ref().map_or(0.0, |t| t.at[1]) + layout.middle_of(to);
                        self.index_at([place[0], y])
                    }
                    (None, _) if key == "up" => Some(0),
                    (None, _) => self.typing.as_ref().map(|t| t.text.len()),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(to) = moved {
            if let Some(typing) = self.typing.as_mut() {
                typing.move_to(to, shift);
                window.invalidate_character_coordinates();
            }
            cx.notify();
            return Keyed::Taken;
        }
        let Some(typing) = self.typing.as_mut() else {
            return Keyed::Taken;
        };
        match key {
            "backspace" | "delete" if word => {
                let range = if !typing.selected.is_empty() {
                    typing.selected.clone()
                } else if key == "backspace" {
                    typing.word_before(caret)..caret
                } else {
                    caret..typing.word_after(caret)
                };
                if !range.is_empty() {
                    typing.replace(range, "");
                    cx.notify();
                }
                Keyed::Taken
            }
            "backspace" | "delete" if alone => {
                let range = if !typing.selected.is_empty() {
                    typing.selected.clone()
                } else if key == "backspace" {
                    typing.before(caret)..caret
                } else {
                    caret..typing.after(caret)
                };
                if !range.is_empty() {
                    typing.replace(range, "");
                    cx.notify();
                }
                Keyed::Taken
            }
            "a" if modifiers.control && !modifiers.alt => {
                typing.selected = 0..typing.text.len();
                typing.reversed = false;
                cx.notify();
                Keyed::Taken
            }
            "c" | "x" if modifiers.control && !modifiers.alt => {
                if typing.selected.is_empty() {
                    // (Nothing of the text selected: the capture's, done with what is written.)
                    return Keyed::NotMine;
                }
                let part = typing.text[typing.selected.clone()].to_string();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(part));
                if key == "x" {
                    let range = typing.selected.clone();
                    typing.replace(range, "");
                }
                cx.notify();
                Keyed::Taken
            }
            "v" if modifiers.control && !modifiers.alt => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    let range = typing.selected.clone();
                    typing.replace(range, &text);
                    cx.notify();
                }
                Keyed::Taken
            }
            "z" | "y" if modifiers.control && !modifiers.alt => {
                let back = key == "z" && !modifiers.shift;
                let (from, to) = if back {
                    (&mut typing.past, &mut typing.future)
                } else {
                    (&mut typing.future, &mut typing.past)
                };
                if let Some((text, selected)) = from.pop() {
                    to.push((typing.text.clone(), typing.selected.clone()));
                    typing.text = text;
                    typing.selected = selected;
                    typing.reversed = false;
                    typing.run_end = None;
                    cx.notify();
                }
                Keyed::Taken
            }
            // (Ctrl+S and the rest of the capture's keys: done first, by the window.)
            _ if modifiers.control || modifiers.alt || modifiers.platform => Keyed::NotMine,
            // Every other key is typed (the system's input brings its letters).
            _ => Keyed::Letter,
        }
    }

    /// Draws the field of what is being written: its frame (dashed; in the accent while it is held),
    /// what is selected of it, and its caret.
    pub(super) fn paint_typing(
        &self,
        window: &mut Window,
        painter: &Painter,
        focused: bool,
        accent: Hsla,
        text_colour: Hsla,
    ) {
        let (Some(typing), Some(room)) = (self.typing.as_ref(), self.typing_room()) else {
            return;
        };
        let layout = laid_out(&typing.text, &self.letters);
        writing::paint_written(
            window,
            painter,
            typing.at,
            [1.0, 1.0],
            &typing.text,
            &self.letters,
        );
        let bounds = painter.bounds(room);
        if typing.held {
            paint::rectangle_line(window, bounds, 2.0, None, accent);
        } else {
            let mut dashes = text_colour;
            dashes.a = 0.8;
            let outside = Bounds::new(
                bounds.origin - point(px(1.0), px(1.0)),
                bounds.size + size(px(2.0), px(2.0)),
            );
            paint::rectangle_line(
                window,
                outside,
                1.0,
                None,
                Hsla {
                    a: 0.5,
                    ..gpui::black()
                },
            );
            paint::rectangle_line(window, bounds, 1.0, Some((3.0, 3.0)), dashes);
        }
        if typing.held {
            return;
        }
        let line_height = painter.length(layout.line);
        // What is selected, line by line.
        if !typing.selected.is_empty() {
            let mut highlight = accent;
            highlight.a = 0.35;
            let lines = typing.text.split('\n').count();
            for line in 0..lines {
                let range = typing.line_range(line);
                let from = typing.selected.start.max(range.start);
                let to = typing.selected.end.min(range.end);
                if from > to
                    || (from == to && !(typing.selected.contains(&range.end) && line + 1 < lines))
                {
                    continue;
                }
                let (Some(left), Some(right)) = (self.place_of(from), self.place_of(to)) else {
                    continue;
                };
                // (A line break selected shows as a little more.)
                let extra = if typing.selected.end > range.end {
                    painter.length(layout.lettered.ascent * 0.3)
                } else {
                    px(0.0)
                };
                let top = painter.at(left).y - line_height / 2.0;
                let start = painter.at(left).x;
                let end = painter.at(right).x + extra;
                window.paint_quad(fill(
                    Bounds::new(point(start, top), size(end - start, line_height)),
                    highlight,
                ));
            }
        }
        // What the system's input is still putting together: a line under it, line by line (as a
        // field of the browser had it).
        if let Some(marked) = typing.marked.clone().filter(|marked| !marked.is_empty()) {
            let colour = if self.letters.fill >= 0.4 {
                paint::colour(self.letters.color)
            } else {
                paint::colour(self.letters.outline.unwrap_or(self.letters.color))
            };
            let below = painter.length(self.letters.size * 0.6);
            let lines = typing.text.split('\n').count();
            for line in 0..lines {
                let range = typing.line_range(line);
                let from = marked.start.max(range.start);
                let to = marked.end.min(range.end);
                if from >= to {
                    continue;
                }
                let (Some(left), Some(right)) = (self.place_of(from), self.place_of(to)) else {
                    continue;
                };
                let (start, end) = (painter.at(left), painter.at(right));
                window.paint_quad(fill(
                    Bounds::new(
                        point(start.x, start.y + below),
                        size(end.x - start.x, px(1.5)),
                    ),
                    colour,
                ));
            }
        }
        if focused
            && typing.selected.is_empty()
            && let Some(place) = self.place_of(typing.caret())
        {
            let at = painter.at(place);
            let colour = if self.letters.fill >= 0.4 {
                paint::colour(self.letters.color)
            } else {
                paint::colour(self.letters.outline.unwrap_or(self.letters.color))
            };
            let tall = painter.length(self.letters.size * 1.15);
            window.paint_quad(fill(
                Bounds::new(point(at.x, at.y - tall / 2.0), size(px(1.5), tall)),
                colour,
            ));
        }
    }

    /// Where the band around the field is pressed: the field's room, and the band around it, on the
    /// screen. (`on_band`: on the band, not in the field.)
    pub(super) fn typing_hit(&self, painter: &Painter, position: Point<Pixels>) -> Option<bool> {
        let room = painter.bounds(self.typing_room()?);
        let band = Bounds::new(
            room.origin - point(px(BAND), px(BAND)),
            room.size + size(px(BAND * 2.0), px(BAND * 2.0)),
        );
        if room.contains(&position) {
            Some(false)
        } else if band.contains(&position) {
            Some(true)
        } else {
            None
        }
    }

    /// The band was pressed (`from`, on the screen).
    pub(super) fn take_band(&mut self, from: Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(typing) = self.typing.as_mut() {
            typing.carrying = Some((from, typing.at, false));
            cx.notify();
        }
    }

    /// The pointer moved with the band held: what is being written goes along, by whole pixels of the
    /// image, never out of the part that is shown. Not until it has gone a few pixels: a press.
    pub(super) fn carry(
        &mut self,
        to: Point<Pixels>,
        scale: f32,
        shown: crate::view::Area,
        cx: &mut Context<Self>,
    ) {
        let Some(typing) = self.typing.as_mut() else {
            return;
        };
        let Some((from, was, far)) = typing.carrying.as_mut() else {
            return;
        };
        let (dx, dy) = ((to.x - from.x).as_f32(), (to.y - from.y).as_f32());
        if !*far && dx.hypot(dy) < STILL {
            return;
        }
        *far = true;
        let room = 16.0 / scale;
        let within =
            |value: f32, least: f32, most: f32| value.clamp(least, most.max(least)).round();
        typing.at = [
            within(was[0] + dx / scale, shown.x, shown.right() - room),
            within(was[1] + dy / scale, shown.y, shown.bottom() - room),
        ];
        cx.notify();
    }

    /// The band is let go: where it was pressed, what is being written is held as a thing.
    pub(super) fn let_go_of_band(&mut self, cx: &mut Context<Self>) {
        if let Some(typing) = self.typing.as_mut()
            && let Some((_, _, far)) = typing.carrying.take()
        {
            if !far {
                typing.held = true;
                let end = typing.text.len();
                typing.selected = end..end;
                typing.reversed = false;
            }
            cx.notify();
        }
    }

    /// A press in the field: the caret there (with Shift, what is selected goes as far); held, the
    /// pointer selects. Pressed twice, the word there is selected; three times, all of it.
    pub(super) fn press_in_field(
        &mut self,
        at: Place,
        shift: bool,
        presses: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.index_at(at) else {
            return;
        };
        if let Some(typing) = self.typing.as_mut() {
            typing.held = false;
            match presses {
                2 if !shift => {
                    typing.selected = typing.word_at(index);
                    typing.reversed = false;
                }
                3.. if !shift => {
                    typing.selected = 0..typing.text.len();
                    typing.reversed = false;
                }
                _ => {
                    typing.move_to(index, shift);
                    typing.selecting = true;
                }
            }
            cx.notify();
        }
    }

    pub(super) fn select_in_field(&mut self, at: Place, cx: &mut Context<Self>) {
        let Some(index) = self.index_at(at) else {
            return;
        };
        if let Some(typing) = self.typing.as_mut()
            && typing.selecting
        {
            typing.move_to(index, true);
            cx.notify();
        }
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let typing = self.typing.as_ref()?;
        let range = typing.range_from_utf16(&range);
        actual.replace(typing.range_to_utf16(&range));
        Some(typing.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let typing = self.typing.as_ref()?;
        Some(UTF16Selection {
            range: typing.range_to_utf16(&typing.selected),
            reversed: typing.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let typing = self.typing.as_ref()?;
        typing
            .marked
            .as_ref()
            .map(|range| typing.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(typing) = self.typing.as_mut() {
            typing.marked = None;
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.frozen {
            return;
        }
        let Some(typing) = self.typing.as_mut() else {
            return;
        };
        let range = range
            .as_ref()
            .map(|range| typing.range_from_utf16(range))
            .or(typing.marked.clone())
            .unwrap_or(typing.selected.clone());
        // (Lines are broken by Shift+Enter: what comes in has no breaks of its own.)
        let text = text.replace(['\r', '\n'], "");
        let composed = typing.marked.is_some();
        typing.type_in(range, &text, composed);
        typing.held = false;
        window.invalidate_character_coordinates();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.frozen {
            return;
        }
        let Some(typing) = self.typing.as_mut() else {
            return;
        };
        let range = range
            .as_ref()
            .map(|range| typing.range_from_utf16(range))
            .or(typing.marked.clone())
            .unwrap_or(typing.selected.clone());
        if typing.marked.is_none() {
            typing.remember();
        }
        typing.text.replace_range(range.clone(), text);
        typing.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        let start = typing.to_utf16(range.start);
        let length = text.encode_utf16().count();
        typing.selected = match selected {
            Some(selected) => typing.range_from_utf16(
                &(start + selected.start.min(length)..start + selected.end.min(length)),
            ),
            None => range.start + text.len()..range.start + text.len(),
        };
        typing.reversed = false;
        typing.held = false;
        window.invalidate_character_coordinates();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let typing = self.typing.as_ref()?;
        let start = typing.byte_of_utf16(range.start);
        let (stage, look) = self.look(window)?;
        let painter = Painter {
            origin: stage.origin,
            offset: [look.part.x, look.part.y],
            scale: look.scale,
        };
        let place = painter.at(self.place_of(start)?);
        let tall = painter.length(self.letters.size * LINE_SHOWN);
        Some(Bounds::new(
            point(place.x, place.y - tall / 2.0),
            size(px(1.0), tall),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let (stage, look) = self.look(window)?;
        let local = super::Editor::local(stage, position);
        let at = look.in_image(local.0, local.1);
        let index = self.index_at(at)?;
        Some(self.typing.as_ref()?.to_utf16(index))
    }
}

/// How tall a line is said to be to the system's input (in sizes of the letters).
const LINE_SHOWN: f32 = 1.2;

/// How something is written to start with, for a capture with steps of `unit` pixels: red, 24 units,
/// bold (owner: 「太字はONでいいよ」), in the font last written in.
pub(super) fn first_writing(unit: f32, font: String) -> Writing {
    Writing {
        color: crate::tools::pen::RED,
        size: writing::FIRST_STEPS * writing::STEP * unit,
        font,
        fill: 1.0,
        outline: None,
        edge: 0.0,
        bold: true,
        italic: false,
        strike: false,
    }
}
