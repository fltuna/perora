//! Choosing what to keep of a capture: the rules, without the screen they are shown on.
//!
//! A session starts from what was on screen when the shortcut was pressed (the monitors and the windows,
//! front to back) and is fed where the pointer went down, moved and came up. It answers two questions:
//! what to highlight right now, and, once the user has decided, what was chosen. Everything is in the
//! coordinates of the virtual screen, in physical pixels.

use crate::geometry::{Point, Rect};
use crate::platform::{ScreenLayout, Window};

/// A press that moves less than this far, in either direction, is a click, not a drag.
const DRAG_THRESHOLD: i32 = 4;

/// A window of which less than this is on the screen, in either direction, is not offered: bars docked
/// just outside an edge, of which a pixel or two reach in, would otherwise be "the window in front"
/// along that edge.
const LEAST_VISIBLE: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// A click takes the window under the pointer, a drag takes the rectangle dragged.
    Smart,
    /// A window picked from the list of windows: its picture, whatever lies in front of it. Nothing
    /// is chosen by pointing at the screen in this mode (the smart mode does that).
    Window,
    /// The rectangle dragged, across monitors if need be.
    Rectangle,
    /// The monitor the pointer is on.
    Monitor,
    /// Every monitor in one image. Chosen by choosing the mode: there is nothing left to point at.
    AllMonitors,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Smart,
        Mode::Window,
        Mode::Rectangle,
        Mode::Monitor,
        Mode::AllMonitors,
    ];

    /// The name used in settings, language files and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            Mode::Smart => "smart",
            Mode::Window => "window",
            Mode::Rectangle => "rectangle",
            Mode::Monitor => "monitor",
            Mode::AllMonitors => "allMonitors",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.name() == name)
    }
}

/// What the user decided on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// A part of the screen as it was: a rectangle, a monitor, or all of them.
    Area(Rect),
    /// One window: its index among the session's windows.
    Window(usize),
}

/// What to show as "this is what you would get".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub area: Rect,
    /// Being dragged out, as opposed to pointed at: shown with its size.
    pub dragging: bool,
}

#[derive(Debug, Clone)]
pub struct Session {
    layout: ScreenLayout,
    /// Front to back, as they were stacked.
    windows: Vec<Window>,
    mode: Mode,
    pointer: Point,
    /// Where the button went down, while it is down.
    pressed: Option<Point>,
}

impl Session {
    pub fn new(layout: ScreenLayout, windows: Vec<Window>, mode: Mode, pointer: Point) -> Self {
        let windows = windows
            .into_iter()
            .filter(|window| {
                window
                    .bounds
                    .intersection(&layout.bounds)
                    .is_some_and(|seen| seen.width >= LEAST_VISIBLE && seen.height >= LEAST_VISIBLE)
            })
            .collect();
        Self {
            layout,
            windows,
            mode,
            pointer,
            pressed: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn layout(&self) -> &ScreenLayout {
        &self.layout
    }

    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Switches the mode. A press under way is dropped; choosing "all monitors" is the choice itself.
    pub fn set_mode(&mut self, mode: Mode) -> Option<Choice> {
        self.mode = mode;
        self.pressed = None;
        (mode == Mode::AllMonitors).then_some(Choice::Area(self.layout.bounds))
    }

    pub fn pointer_down(&mut self, at: Point) {
        self.pointer = at;
        self.pressed = Some(at);
    }

    pub fn pointer_moved(&mut self, at: Point) {
        self.pointer = at;
    }

    /// The button came up: what was chosen, if that completes a choice. A click on nothing that can be
    /// chosen, or a drag where a click was called for, chooses nothing and the session goes on.
    pub fn pointer_up(&mut self, at: Point) -> Option<Choice> {
        self.pointer = at;
        let pressed = self.pressed.take()?;
        let dragged = self.dragged(pressed);

        match self.mode {
            Mode::Smart => match dragged {
                // A drag is a drag even when it encloses nothing (along one line): not a click.
                Some(area) => area.map(Choice::Area),
                None => self.window_at(at).map(Choice::Window),
            },
            Mode::Window => None,
            Mode::Rectangle => dragged.flatten().map(Choice::Area),
            Mode::Monitor => self
                .layout
                .monitor_at(at)
                .map(|monitor| Choice::Area(monitor.bounds)),
            Mode::AllMonitors => Some(Choice::Area(self.layout.bounds)),
        }
    }

    /// Gives up the press under way, if any (the pointer left, the window lost the keyboard).
    pub fn release(&mut self) {
        self.pressed = None;
    }

    /// What would be chosen if the button came up (or, with nothing pressed, went down and up) here.
    pub fn highlight(&self) -> Option<Highlight> {
        let dragged = self.pressed.and_then(|pressed| self.dragged(pressed));
        let dragging = dragged.is_some();
        let dragged = dragged.flatten();
        let pointed = |area: Rect| Highlight {
            area,
            dragging: false,
        };

        match self.mode {
            Mode::Smart | Mode::Rectangle if dragging => dragged.map(|area| Highlight {
                area,
                dragging: true,
            }),
            Mode::Smart => self
                .window_at(self.pointer)
                .and_then(|index| self.visible(self.windows[index].bounds))
                .map(pointed),
            Mode::Window | Mode::Rectangle => None,
            Mode::Monitor => self
                .layout
                .monitor_at(self.pointer)
                .map(|monitor| pointed(monitor.bounds)),
            Mode::AllMonitors => Some(pointed(self.layout.bounds)),
        }
    }

    /// `None` while the pointer has not gone far enough from where the button went down for a drag to
    /// be meant. Then the rectangle between the two, cut to the screen: `Some(None)` when that is
    /// nothing (a drag along one line, or all of it off the screen).
    fn dragged(&self, pressed: Point) -> Option<Option<Rect>> {
        let moved = (self.pointer.x - pressed.x)
            .abs()
            .max((self.pointer.y - pressed.y).abs());
        if moved < DRAG_THRESHOLD {
            return None;
        }
        Some(self.visible(Rect::from_corners(pressed, self.pointer)))
    }

    /// The window in front at a point: the first, front to back, that covers it.
    fn window_at(&self, point: Point) -> Option<usize> {
        self.windows
            .iter()
            .position(|window| window.bounds.contains(point))
    }

    /// The part of an area that is on the virtual screen at all.
    fn visible(&self, area: Rect) -> Option<Rect> {
        area.intersection(&self.layout.bounds)
    }
}

/// How many pixels of a window's edge are left out when the window is captured. Its outermost pixels
/// are not quite its own: the border is drawn over what lies behind it, and the desktop showed along
/// every side of a captured window (owner, 2026-10-14: "1px か 2px ぐらい見えちゃってる").
pub const WINDOW_EDGE: u32 = 1;

/// The part of a window that is captured: without its edge (`WINDOW_EDGE` on every side), unless the
/// window fills a monitor, or the part of one windows are meant to use (full screen, maximized): its
/// edge is the monitor's then, and nothing behind it shows.
pub fn without_edge(bounds: Rect, layout: &ScreenLayout) -> Rect {
    let fills = |area: &Rect| bounds.intersection(area) == Some(*area);
    let edge = WINDOW_EDGE;
    if layout
        .monitors
        .iter()
        .any(|monitor| fills(&monitor.bounds) || fills(&monitor.work_area))
        || bounds.width <= edge * 2
        || bounds.height <= edge * 2
    {
        return bounds;
    }
    Rect::new(
        bounds.x + edge as i32,
        bounds.y + edge as i32,
        bounds.width - edge * 2,
        bounds.height - edge * 2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Monitor, WindowId};

    fn monitor(x: i32, primary: bool) -> Monitor {
        let bounds = Rect::new(x, 0, 1000, 800);
        Monitor {
            name: format!("monitor at {x}"),
            bounds,
            work_area: bounds,
            scale: 1.0,
            primary,
        }
    }

    fn window(id: isize, bounds: Rect) -> Window {
        Window {
            id: WindowId(id),
            title: format!("window {id}"),
            bounds,
        }
    }

    #[test]
    fn a_window_is_captured_without_its_edge_unless_it_fills_a_monitor() {
        let mut first = monitor(0, true);
        first.work_area = Rect::new(0, 0, 1000, 760);
        let layout = ScreenLayout {
            bounds: Rect::new(0, 0, 2000, 800),
            monitors: vec![first, monitor(1000, false)],
        };
        // A window among others: a pixel in from every side.
        assert_eq!(
            without_edge(Rect::new(100, 50, 400, 300), &layout),
            Rect::new(101, 51, 398, 298)
        );
        // Full screen, maximized (the work area), and larger than the monitor: as it is.
        for filling in [
            Rect::new(0, 0, 1000, 800),
            Rect::new(0, 0, 1000, 760),
            Rect::new(1000, 0, 1000, 800),
            Rect::new(-8, -8, 1016, 776),
        ] {
            assert_eq!(without_edge(filling, &layout), filling);
        }
        // Too thin to have an edge to leave out.
        assert_eq!(
            without_edge(Rect::new(10, 10, 2, 50), &layout),
            Rect::new(10, 10, 2, 50)
        );
    }

    /// Two monitors side by side; a small window in front of a large one on the left monitor, and one
    /// that reaches from the left monitor onto the right.
    fn session(mode: Mode) -> Session {
        let layout = ScreenLayout::new(vec![monitor(0, true), monitor(1000, false)]).unwrap();
        let windows = vec![
            window(1, Rect::new(100, 100, 200, 200)),
            window(2, Rect::new(50, 50, 600, 600)),
            window(3, Rect::new(900, 300, 400, 300)),
        ];
        Session::new(layout, windows, mode, Point::new(0, 0))
    }

    fn click(session: &mut Session, at: Point) -> Option<Choice> {
        session.pointer_down(at);
        session.pointer_up(at)
    }

    fn drag(session: &mut Session, from: Point, to: Point) -> Option<Choice> {
        session.pointer_down(from);
        session.pointer_moved(to);
        session.pointer_up(to)
    }

    #[test]
    fn smart_takes_the_window_in_front_on_a_click() {
        let mut session = session(Mode::Smart);

        session.pointer_moved(Point::new(150, 150));
        assert_eq!(
            session.highlight().unwrap().area,
            Rect::new(100, 100, 200, 200)
        );
        assert_eq!(
            click(&mut session, Point::new(150, 150)),
            Some(Choice::Window(0))
        );

        // Beside the small one, the large one behind it.
        session.pointer_moved(Point::new(400, 400));
        assert_eq!(
            session.highlight().unwrap().area,
            Rect::new(50, 50, 600, 600)
        );
        assert_eq!(
            click(&mut session, Point::new(400, 400)),
            Some(Choice::Window(1))
        );
    }

    #[test]
    fn smart_takes_the_rectangle_on_a_drag_even_across_monitors_and_windows() {
        let mut session = session(Mode::Smart);

        session.pointer_down(Point::new(150, 150));
        session.pointer_moved(Point::new(1500, 700));
        assert_eq!(
            session.highlight(),
            Some(Highlight {
                area: Rect::new(150, 150, 1350, 550),
                dragging: true
            })
        );
        assert_eq!(
            session.pointer_up(Point::new(1500, 700)),
            Some(Choice::Area(Rect::new(150, 150, 1350, 550)))
        );

        // In any direction.
        assert_eq!(
            drag(&mut session, Point::new(1500, 700), Point::new(150, 150)),
            Some(Choice::Area(Rect::new(150, 150, 1350, 550)))
        );
    }

    #[test]
    fn a_press_that_hardly_moves_is_a_click() {
        let mut session = session(Mode::Smart);

        assert_eq!(
            drag(&mut session, Point::new(150, 150), Point::new(153, 148)),
            Some(Choice::Window(0))
        );
        // Far enough to be a drag, but along one line: no area, and not a click on the window either.
        session.pointer_down(Point::new(150, 150));
        session.pointer_moved(Point::new(400, 150));
        assert_eq!(session.highlight(), None);
        assert_eq!(session.pointer_up(Point::new(400, 150)), None);
    }

    #[test]
    fn where_nothing_can_be_chosen_a_click_chooses_nothing_and_the_session_goes_on() {
        let mut session = session(Mode::Smart);

        // The desktop: no window there.
        session.pointer_moved(Point::new(800, 20));
        assert_eq!(session.highlight(), None);
        assert_eq!(click(&mut session, Point::new(800, 20)), None);
        assert_eq!(
            click(&mut session, Point::new(150, 150)),
            Some(Choice::Window(0))
        );
    }

    #[test]
    fn the_window_mode_chooses_nothing_by_pointing_and_rectangle_mode_does_not_click() {
        // Windows are picked from the list there; the screen behind it is only a backdrop.
        let mut windows = session(Mode::Window);
        windows.pointer_moved(Point::new(150, 150));
        assert_eq!(windows.highlight(), None);
        assert_eq!(click(&mut windows, Point::new(150, 150)), None);
        assert_eq!(
            drag(&mut windows, Point::new(150, 150), Point::new(400, 400)),
            None
        );

        let mut rectangle = session(Mode::Rectangle);
        rectangle.pointer_moved(Point::new(150, 150));
        assert_eq!(rectangle.highlight(), None, "nothing to point at");
        assert_eq!(click(&mut rectangle, Point::new(150, 150)), None);
        assert_eq!(
            drag(&mut rectangle, Point::new(10, 10), Point::new(60, 40)),
            Some(Choice::Area(Rect::new(10, 10, 50, 30)))
        );
    }

    #[test]
    fn monitor_mode_takes_the_monitor_under_the_pointer() {
        let mut session = session(Mode::Monitor);

        session.pointer_moved(Point::new(1700, 10));
        assert_eq!(
            session.highlight().unwrap().area,
            Rect::new(1000, 0, 1000, 800)
        );
        assert_eq!(
            click(&mut session, Point::new(1700, 10)),
            Some(Choice::Area(Rect::new(1000, 0, 1000, 800)))
        );
        assert_eq!(
            click(&mut session, Point::new(5, 5)),
            Some(Choice::Area(Rect::new(0, 0, 1000, 800)))
        );
    }

    #[test]
    fn choosing_all_monitors_is_the_choice() {
        let mut session = session(Mode::Smart);

        assert_eq!(session.set_mode(Mode::Rectangle), None);
        assert_eq!(
            session.set_mode(Mode::AllMonitors),
            Some(Choice::Area(Rect::new(0, 0, 2000, 800)))
        );
    }

    #[test]
    fn a_highlight_and_a_drag_stop_at_the_edge_of_the_screen() {
        // A window hanging off the right edge and the bottom.
        let layout = ScreenLayout::new(vec![monitor(0, true)]).unwrap();
        let windows = vec![window(1, Rect::new(800, 600, 500, 500))];
        let mut session = Session::new(layout, windows, Mode::Smart, Point::new(900, 700));

        assert_eq!(
            session.highlight().unwrap().area,
            Rect::new(800, 600, 200, 200)
        );
        assert_eq!(
            drag(&mut session, Point::new(900, 700), Point::new(5000, -300)),
            Some(Choice::Area(Rect::new(900, 0, 100, 700)))
        );
    }

    #[test]
    fn a_window_of_which_hardly_anything_is_on_screen_is_not_offered() {
        // A bar docked above the top edge, two pixels of it reaching in, in front of a real window.
        let layout = ScreenLayout::new(vec![monitor(0, true)]).unwrap();
        let windows = vec![
            window(1, Rect::new(0, -28, 1000, 30)),
            window(2, Rect::new(0, 0, 1000, 800)),
        ];
        let mut session = Session::new(layout, windows, Mode::Smart, Point::new(500, 1));

        assert_eq!(session.windows().len(), 1);
        assert_eq!(
            session.highlight().unwrap().area,
            Rect::new(0, 0, 1000, 800)
        );
        assert_eq!(
            click(&mut session, Point::new(500, 1)),
            Some(Choice::Window(0))
        );
    }

    #[test]
    fn switching_the_mode_drops_a_press_under_way() {
        let mut session = session(Mode::Smart);

        session.pointer_down(Point::new(10, 10));
        session.pointer_moved(Point::new(500, 500));
        session.set_mode(Mode::Rectangle);

        assert_eq!(session.highlight(), None);
        assert_eq!(session.pointer_up(Point::new(500, 500)), None);
    }

    #[test]
    fn modes_are_known_by_their_names() {
        for mode in Mode::ALL {
            assert_eq!(Mode::from_name(mode.name()), Some(mode));
        }
        assert_eq!(Mode::from_name("nonsense"), None);
    }
}
