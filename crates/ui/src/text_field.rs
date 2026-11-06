//! A box to write a line in: the browser's `<input type="text">` in the Tauri version. It takes what
//! is typed through the system's input (so that Japanese is put together in it as anywhere), keeps a
//! selection, and copies, cuts and pastes.
//!
//! Made from GPUI's own example of a text input (`crates/gpui/examples/input.rs` in Zed, Apache-2.0),
//! with Perora's look and what Perora's boxes need: at most so many letters, small letters only
//! where asked, and telling the one it belongs to of each change.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InteractiveElement, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement, Pixels, Point, Render, ShapedLine,
    SharedString, Style, Styled, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div,
    fill, point, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::Theme;

actions!(
    text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
    ]
);

/// The key context the boxes' keys are bound in.
const CONTEXT: &str = "TextField";

/// The keys of the boxes: once, when the application starts.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("ctrl-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("ctrl-v", Paste, Some(CONTEXT)),
        KeyBinding::new("ctrl-c", Copy, Some(CONTEXT)),
        KeyBinding::new("ctrl-x", Cut, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
    ]);
}

/// What a box tells the one it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFieldEvent {
    /// What is written in it changed (by the user): all of it now.
    Changed(String),
}

pub struct TextField {
    focus: FocusHandle,
    content: String,
    /// At most this many letters (`maxlength`).
    longest: usize,
    /// What is typed is written in small letters.
    lowercase: bool,
    /// The letters are put at the right of the box (numbers).
    to_the_right: bool,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    selecting: bool,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            content: String::new(),
            longest: usize::MAX,
            lowercase: false,
            to_the_right: false,
            selected: 0..0,
            reversed: false,
            marked: None,
            last_layout: None,
            last_bounds: None,
            selecting: false,
        }
    }

    pub fn longest(mut self, letters: usize) -> Self {
        self.longest = letters;
        self
    }

    pub fn lowercase(mut self) -> Self {
        self.lowercase = true;
        self
    }

    pub fn to_the_right(mut self) -> Self {
        self.to_the_right = true;
        self
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Writes this in it, as the one it belongs to has it (not the user: no `Changed`). The caret
    /// goes to its end.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.content == text {
            return;
        }
        self.content = text.to_string();
        self.selected = self.content.len()..self.content.len();
        self.reversed = false;
        self.marked = None;
        cx.notify();
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.previous_boundary(self.cursor()), window, cx);
        } else {
            self.move_to(self.selected.start, window, cx)
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.next_boundary(self.selected.end), window, cx);
        } else {
            self.move_to(self.selected.end, window, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor()), window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx)
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            let before = self.previous_boundary(self.cursor());
            if self.cursor() == before {
                return;
            }
            self.select_to(before, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            let after = self.next_boundary(self.cursor());
            if self.cursor() == after {
                return;
            }
            self.select_to(after, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.selecting = true;
        window.focus(&self.focus, cx);
        if event.modifiers.shift {
            self.select_to(self.index_at(event.position), window, cx);
        } else {
            self.move_to(self.index_at(event.position), window, cx)
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_at(event.position), window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace(['\r', '\n'], " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected = offset..offset;
        self.reversed = false;
        window.notify_text_selection_changed(&self.focus);
        cx.notify()
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        // (Laid out for what was there before: the end, which is surely a letter's edge.)
        if line.text != self.content {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - self.line_left(bounds, line))
    }

    /// Where the line begins in the box (at the right of it, for numbers).
    fn line_left(&self, bounds: &Bounds<Pixels>, line: &ShapedLine) -> Pixels {
        if self.to_the_right {
            bounds.right() - line.width
        } else {
            bounds.left()
        }
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset
        } else {
            self.selected.end = offset
        };
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        window.notify_text_selection_changed(&self.focus);
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for letter in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += letter.len_utf16();
            utf8 += letter.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for letter in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += letter.len_utf8();
            utf16 += letter.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    /// What is to be put in place of `range`, kept within the letters the box takes.
    fn fitted(&self, range: &Range<usize>, text: &str) -> String {
        let text = if self.lowercase {
            text.to_lowercase()
        } else {
            text.to_string()
        };
        let staying =
            self.content[..range.start].chars().count() + self.content[range.end..].chars().count();
        let room = self.longest.saturating_sub(staying);
        text.chars().take(room).collect()
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextFieldEvent::Changed(self.content.clone()));
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = self.range_from_utf16(&range_utf16);
        self.reversed = false;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        let new_text = self.fitted(&range, new_text);
        self.content =
            self.content[..range.start].to_owned() + &new_text + &self.content[range.end..];
        self.selected = range.start + new_text.len()..range.start + new_text.len();
        self.reversed = false;
        self.marked.take();
        window.notify_text_input_changed(&self.focus);
        self.changed(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        // (What is being put together is not cut short: it is not written yet.)
        self.content =
            self.content[..range.start].to_owned() + new_text + &self.content[range.end..];
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        let start_utf16 = self.offset_to_utf16(range.start);
        let length_utf16 = new_text.encode_utf16().count();
        self.selected = new_selected_range_utf16
            .map(|selected| {
                self.range_from_utf16(
                    &(start_utf16 + selected.start.min(length_utf16)
                        ..start_utf16 + selected.end.min(length_utf16)),
                )
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.reversed = false;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        if line.text != self.content {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let left = self.line_left(&bounds, line);
        Some(Bounds::from_corners(
            point(left + line.x_for_index(range.start), bounds.top()),
            point(left + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line = self.last_layout.as_ref()?;
        if line.text != self.content || !bounds.contains(&point) {
            return None;
        }
        let index = line.closest_index_for_x(point.x - self.line_left(&bounds, line));
        Some(self.offset_to_utf16(index))
    }
}

/// The line of a box, with its caret or selection.
struct Line {
    field: Entity<TextField>,
}

struct Shaped {
    line: Option<ShapedLine>,
    caret: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for Line {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Line {
    type RequestLayoutState = ();
    type PrepaintState = Shaped;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.field.read(cx);
        let theme = Theme::get(cx);
        let content: SharedString = field.content.clone().into();
        let style = window.text_style();
        let run = TextRun {
            len: content.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match field.marked.as_ref() {
            Some(marked) => [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: content.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(content, font_size, &runs, None);
        let left = field.line_left(&bounds, &line);
        let (selection, caret) = if field.selected.is_empty() {
            let at = line.x_for_index(field.cursor());
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(left + at, bounds.top()),
                        size(px(1.0), bounds.bottom() - bounds.top()),
                    ),
                    theme.text,
                )),
            )
        } else {
            let mut selection = theme.accent;
            selection.a = 0.35;
            (
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(field.selected.start), bounds.top()),
                        point(left + line.x_for_index(field.selected.end), bounds.bottom()),
                    ),
                    selection,
                )),
                None,
            )
        };
        Shaped {
            line: Some(line),
            caret,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        shaped: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.field.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        if let Some(selection) = shaped.selection.take() {
            window.paint_quad(selection)
        }
        let Some(line) = shaped.line.take() else {
            return;
        };
        let left = self.field.read(cx).line_left(&bounds, &line);
        let _ = line.paint(
            point(left, bounds.top()),
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        if focus.is_focused(window)
            && let Some(caret) = shaped.caret.take()
        {
            window.paint_quad(caret);
        }
        self.field.update(cx, |field, _| {
            field.last_layout = Some(line);
            field.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let focused = self.focus.is_focused(window);
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .h(px(28.0))
            .w_full()
            .flex()
            .items_center()
            .px(px(8.0))
            .overflow_hidden()
            .rounded(theme.radius_small)
            .border_1()
            .border_color(if focused { theme.accent } else { theme.border })
            .bg(theme.background)
            .text_color(theme.text)
            .line_height(px(18.0))
            .child(Line { field: cx.entity() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext;

    #[gpui::test]
    fn what_is_typed_is_kept_within_its_letters(cx: &mut gpui::TestAppContext) {
        let field = cx.new(|cx| TextField::new(cx).longest(7).lowercase());
        let window = cx.add_empty_window();
        window.update(|window, cx| {
            field.update(cx, |field, cx| {
                field.replace_text_in_range(None, "#E61B1B99", window, cx);
                assert_eq!(field.text(), "#e61b1b");
                field.replace_text_in_range(None, "0", window, cx);
                assert_eq!(field.text(), "#e61b1b");
                field.set_text("abc", cx);
                assert_eq!(field.text(), "abc");
            });
        });
    }
}
