//! The small preview, as Perora holds it while it is on screen: the capture, whether it is on the
//! clipboard, what was chosen for it this once, and whether anything was done about it (one that
//! nothing was done about is given up when its time is over). From the Tauri version's `preview.rs`,
//! without its windows: the GPUI application shows it (`app/src/preview.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use crate::edit::Edit;
use crate::geometry::Rect;
use crate::output::{Format, Output};
use crate::platform::Frame;
use crate::settings::{Leaves, Settings};

/// The small window, in logical pixels. It has the shape of its capture: its picture is as wide as
/// there is room for and as tall as the capture then is, and where that would be taller than a
/// window in a corner should be, as tall as that and narrower instead. Within bounds, both ways:
/// the buttons need their width, and a strip of a capture still needs a picture to click on. Past
/// the bounds the picture is filled with the middle of the capture rather than left partly empty.
///
/// How wide the picture can be, and how tall; and what the window has around it (across, and
/// down: the status and the buttons).
pub const PICTURE_WIDTH: (f64, f64) = (324.0, 336.0);
pub const PICTURE_HEIGHT: (f64, f64) = (48.0, 420.0);
pub const AROUND_PICTURE: (f64, f64) = (24.0, 89.0);

/// How large the picture is for a capture of that many pixels, in logical pixels.
pub fn picture_size(width: u32, height: u32) -> (f64, f64) {
    let shape = f64::from(width.max(1)) / f64::from(height.max(1));
    let (mut across, mut down) = (PICTURE_WIDTH.1, (PICTURE_WIDTH.1 / shape).round());
    if down > PICTURE_HEIGHT.1 {
        down = PICTURE_HEIGHT.1;
        across = (down * shape).round().max(PICTURE_WIDTH.0);
    }
    (across, down.max(PICTURE_HEIGHT.0))
}

/// How large the small window is for a capture of that many pixels, in logical pixels.
pub fn small_size(width: u32, height: u32) -> (f64, f64) {
    let (across, down) = picture_size(width, height);
    (AROUND_PICTURE.0 + across, AROUND_PICTURE.1 + down)
}

/// The part of a capture of `width` by `height` that the picture shows: all of it when it has the
/// picture's shape (`across` by `down`), otherwise its middle, cut to that shape.
pub fn middle(width: u32, height: u32, across: f64, down: f64) -> Rect {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let shape = across / down.max(1.0);
    if w / h > shape {
        let kept = (h * shape).round().clamp(1.0, w) as u32;
        Rect::new(((width - kept) / 2) as i32, 0, kept, height)
    } else {
        let kept = (w / shape).round().clamp(1.0, h) as u32;
        Rect::new(0, ((height - kept) / 2) as i32, width, kept)
    }
}

/// What the user chose for one capture alone, of one way it leaves, in place of what the settings
/// say: the kind of file, the square it is to fit (`Some(None)` for none), or neither.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Once {
    pub format: Option<Format>,
    /// Several kinds at once, for saving: one file of each.
    pub formats: Option<Vec<Format>>,
    pub fit: Option<Option<u32>>,
}

impl Once {
    /// What a capture is made into as it leaves that way, this time.
    pub fn output(&self, settings: &Settings, leaves: Leaves) -> Output {
        let usual = settings.output(leaves);
        Output {
            format: self.format.unwrap_or(usual.format),
            fit: self.fit.unwrap_or(usual.fit),
            ..usual
        }
    }

    /// The kinds of file a capture is saved as, this time: those asked for, or what the settings
    /// say. Never none.
    pub fn saved_as(&self, settings: &Settings) -> Vec<Format> {
        let kinds = match (&self.formats, self.format) {
            (Some(kinds), _) if !kinds.is_empty() => kinds.clone(),
            (_, Some(kind)) => vec![kind],
            _ => settings.save_formats.clone(),
        };
        if kinds.is_empty() {
            vec![Format::default()]
        } else {
            kinds
        }
    }
}

/// What was chosen this once, of each way a capture leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chosen([Once; 3]);

impl Chosen {
    pub fn of(&self, leaves: Leaves) -> &Once {
        &self.0[leaves as usize]
    }

    pub fn of_mut(&mut self, leaves: Leaves) -> &mut Once {
        &mut self.0[leaves as usize]
    }
}

/// The capture being previewed.
pub struct Pending {
    pub id: u64,
    pub frame: Arc<Frame>,
    /// The capture as a PNG, when one was made of it as it was (it went to the clipboard that way):
    /// saved as that, not made again.
    pub png: Option<Arc<Vec<u8>>>,
    /// Whether the image went to the clipboard when the preview appeared: `None` when the user has
    /// that turned off, otherwise the clipboard's count of changes right after it did (`None` when
    /// the system did not say, or a later capture went there first), or why it did not.
    pub copied: Option<Result<Option<u32>, String>>,
    /// What it was made into as it went there: the kind of file, how large at most.
    pub copied_as: Output,
    /// What was drawn on it when it last went there (nothing: it went as it was taken).
    pub copied_edits: Vec<Edit>,
    /// What the user chose for this capture alone.
    pub once: Chosen,
    /// Whether anything was done about the capture: shown large, asked to leave (whatever came of
    /// that), a choice opened. It is not given up on its own after that.
    pub touched: bool,
    /// The files it was saved as, as it was taken, when the user has every capture saved.
    pub saved: Vec<PathBuf>,
    /// Which time it is that the small preview was left to: one that began later counts, not the
    /// ones before it.
    pub timer: u64,
}

impl Pending {
    pub fn new(
        id: u64,
        frame: Arc<Frame>,
        png: Option<Arc<Vec<u8>>>,
        copied: Option<Result<Option<u32>, String>>,
        copied_as: Output,
    ) -> Self {
        Self {
            id,
            frame,
            png,
            copied,
            copied_as,
            copied_edits: Vec::new(),
            once: Chosen::default(),
            touched: false,
            saved: Vec::new(),
            timer: 0,
        }
    }

    /// Whether nothing was done about the capture yet.
    pub fn is_untouched(&self) -> bool {
        !self.touched
    }

    /// Whether the image this preview put on the clipboard is still what is there (`now` is the
    /// clipboard's count of changes): nothing was put there since, by the user or by Perora.
    pub fn on_clipboard(&self, now: Option<u32>) -> bool {
        matches!(&self.copied, Some(Ok(Some(written))) if now == Some(*written))
    }

    /// Whether what is on the clipboard of it is what copying would put there now: the same kind
    /// of file, as large. (With another chosen this once, copying is on offer again.)
    pub fn copied_as_asked(&self, settings: &Settings) -> bool {
        let asked = self.once.of(Leaves::Copy).output(settings, Leaves::Copy);
        self.copied_as.format == asked.format && self.copied_as.fit == asked.fit
    }

    /// Whether the clipboard holds the capture as copying it would put it there now (`now` is the
    /// clipboard's count of changes): as large, the same kind of file, and with what is drawn on it
    /// now (`edits`).
    pub fn copied_now(&self, now: Option<u32>, settings: &Settings, edits: &[Edit]) -> bool {
        self.on_clipboard(now) && self.copied_as_asked(settings) && self.copied_edits == edits
    }

    /// Why it could not be put on the clipboard as the preview appeared, if it could not.
    pub fn copy_failure(&self) -> Option<&str> {
        match &self.copied {
            Some(Err(detail)) => Some(detail),
            _ => None,
        }
    }

    /// The file it leaves as, made as `how` says: the PNG at hand, when that is what is asked for.
    pub fn file(&self, how: Output) -> Result<Arc<Vec<u8>>, String> {
        if let Some(png) = &self.png
            && how.keeps(self.frame.width(), self.frame.height())
        {
            return Ok(png.clone());
        }
        self.frame.to_file(how).map(Arc::new)
    }
}

/// The captures the user last discarded, the latest last: what is saved of one after that (it was
/// saved as it was taken, and that took a moment) is taken away again, as it would have been had it
/// been there. (A few: another capture may have been taken and discarded meanwhile.)
#[derive(Debug, Default)]
pub struct Discarded(Vec<u64>);

/// How many of the captures discarded last are kept in mind.
const DISCARDED_KEPT: usize = 8;

impl Discarded {
    pub fn add(&mut self, id: u64) {
        self.0.push(id);
        let over = self.0.len().saturating_sub(DISCARDED_KEPT);
        self.0.drain(..over);
    }

    pub fn has(&self, id: u64) -> bool {
        self.0.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Quality;

    #[test]
    fn the_window_has_the_shape_of_its_capture_within_bounds() {
        // 16:9: as wide as there is room for.
        assert_eq!(small_size(1920, 1080), (360.0, 89.0 + 189.0));
        // Taller than a window in a corner should be: as tall as that, narrower, but not narrower
        // than the buttons need.
        assert_eq!(picture_size(1080, 1920), (324.0, 420.0));
        assert_eq!(picture_size(1000, 1266), (332.0, 420.0));
        // A strip still has a picture to click on.
        assert_eq!(picture_size(5120, 40), (336.0, 48.0));
    }

    #[test]
    fn a_shape_the_window_cannot_follow_shows_its_middle() {
        // As the picture is: all of it.
        assert_eq!(
            middle(1920, 1080, 336.0, 189.0),
            Rect::new(0, 0, 1920, 1080)
        );
        // Much wider than the picture: the middle of it, as wide as the picture's shape allows.
        assert_eq!(middle(5120, 40, 336.0, 48.0), Rect::new(2420, 0, 280, 40));
        // Much taller: the middle, top to bottom.
        let tall = middle(100, 2000, 324.0, 420.0);
        assert_eq!((tall.x, tall.width), (0, 100));
        assert_eq!(tall.height, 130);
        assert_eq!(tall.y, (2000 - 130) / 2);
    }

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn what_is_chosen_once_takes_the_place_of_the_settings() {
        let settings = settings();
        let usual = settings.output(Leaves::Copy);
        let mut once = Once::default();
        assert_eq!(once.output(&settings, Leaves::Copy), usual);
        once.format = Some(Format::WebP);
        once.fit = Some(None);
        let asked = once.output(&settings, Leaves::Copy);
        assert_eq!(asked.format, Format::WebP);
        assert_eq!(asked.fit, None);
        assert_eq!(asked.quality, usual.quality);
        once.fit = Some(Some(1280));
        assert_eq!(once.output(&settings, Leaves::Copy).fit, Some(1280));
    }

    #[test]
    fn a_capture_is_saved_as_some_kind_always() {
        let mut settings = settings();
        let mut once = Once::default();
        assert_eq!(once.saved_as(&settings), settings.save_formats);
        once.formats = Some(Vec::new());
        assert_eq!(once.saved_as(&settings), settings.save_formats);
        once.format = Some(Format::Jpeg);
        assert_eq!(once.saved_as(&settings), vec![Format::Jpeg]);
        once.formats = Some(vec![Format::Png, Format::WebP]);
        assert_eq!(once.saved_as(&settings), vec![Format::Png, Format::WebP]);
        settings.save_formats.clear();
        assert_eq!(Once::default().saved_as(&settings), vec![Format::default()]);
    }

    fn pending(copied: Option<Result<Option<u32>, String>>) -> Pending {
        let frame = Frame {
            area: Rect::new(0, 0, 2, 2),
            rgba: vec![0; 16],
        };
        Pending::new(
            1,
            Arc::new(frame),
            None,
            copied,
            Output {
                format: Format::Png,
                quality: Quality::default(),
                fit: None,
            },
        )
    }

    #[test]
    fn on_the_clipboard_until_something_else_is_put_there() {
        let there = pending(Some(Ok(Some(7))));
        assert!(there.on_clipboard(Some(7)));
        assert!(!there.on_clipboard(Some(8)));
        assert!(!there.on_clipboard(None));
        // Not put there (the user would rather not, or it failed, or a later one went first).
        assert!(!pending(None).on_clipboard(Some(7)));
        assert!(!pending(Some(Err("no".into()))).on_clipboard(Some(7)));
        assert!(!pending(Some(Ok(None))).on_clipboard(None));
        assert_eq!(pending(Some(Err("no".into()))).copy_failure(), Some("no"));
    }

    #[test]
    fn copied_as_asked_follows_what_is_chosen_once() {
        let settings = settings();
        let mut there = pending(Some(Ok(Some(7))));
        there.copied_as = settings.output(Leaves::Copy);
        assert!(there.copied_as_asked(&settings));
        there.once.of_mut(Leaves::Copy).fit = Some(Some(1024));
        assert!(!there.copied_as_asked(&settings));
        // (Chosen once for saving: nothing to do with what is on the clipboard.)
        there.once = Chosen::default();
        there.once.of_mut(Leaves::Save).format = Some(Format::WebP);
        assert!(there.copied_as_asked(&settings));
    }

    #[test]
    fn untouched_until_something_is_done_about_it() {
        let mut one = pending(None);
        assert!(one.is_untouched());
        one.touched = true;
        assert!(!one.is_untouched());
    }

    #[test]
    fn the_png_at_hand_is_what_leaves_when_that_is_asked_for() {
        let mut one = pending(None);
        one.png = Some(Arc::new(b"png".to_vec()));
        let as_png = Output {
            format: Format::Png,
            quality: Quality::default(),
            fit: None,
        };
        assert_eq!(one.file(as_png).unwrap().as_slice(), b"png");
        // Smaller than it is asks for nothing: it fits already.
        let within = Output {
            fit: Some(1024),
            ..as_png
        };
        assert_eq!(one.file(within).unwrap().as_slice(), b"png");
        // Another kind is made.
        let jpeg = Output {
            format: Format::Jpeg,
            ..as_png
        };
        assert_ne!(one.file(jpeg).unwrap().as_slice(), b"png");
    }

    #[test]
    fn a_few_discarded_captures_are_kept_in_mind() {
        let mut discarded = Discarded::default();
        for id in 1..=10 {
            discarded.add(id);
        }
        assert!(!discarded.has(2));
        assert!(discarded.has(3));
        assert!(discarded.has(10));
    }
}
