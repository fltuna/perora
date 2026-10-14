//! Positions and areas on the virtual screen: the one coordinate space that holds every monitor, in
//! physical pixels. Its origin is the primary monitor's top-left corner, so a monitor to the left of it
//! or above it has negative coordinates.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// From two opposite corners in any order, as a drag gives them. `right` and `bottom` are exclusive.
    pub fn from_corners(a: Point, b: Point) -> Self {
        let (left, right) = (a.x.min(b.x), a.x.max(b.x));
        let (top, bottom) = (a.y.min(b.y), a.y.max(b.y));
        Self::new(left, top, (right - left) as u32, (bottom - top) as u32)
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }

    /// The part both cover; `None` when they only touch or are apart.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > left && bottom > top)
            .then(|| Rect::new(left, top, (right - left) as u32, (bottom - top) as u32))
    }

    /// The smallest rectangle that holds both.
    pub fn union(&self, other: &Rect) -> Rect {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(left, top, (right - left) as u32, (bottom - top) as u32)
    }

    /// The smallest rectangle that holds all of them; `None` for none.
    pub fn bounding(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
        rects.into_iter().reduce(|all, rect| all.union(&rect))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_from_two_corners_does_not_care_about_their_order() {
        let expected = Rect::new(-100, 50, 300, 150);
        assert_eq!(
            Rect::from_corners(Point::new(-100, 50), Point::new(200, 200)),
            expected
        );
        assert_eq!(
            Rect::from_corners(Point::new(200, 200), Point::new(-100, 50)),
            expected
        );
        assert_eq!(
            Rect::from_corners(Point::new(200, 50), Point::new(-100, 200)),
            expected
        );
        assert!(Rect::from_corners(Point::new(5, 5), Point::new(5, 90)).is_empty());
    }

    #[test]
    fn right_and_bottom_are_outside() {
        let rect = Rect::new(-1920, 0, 1920, 1080);
        assert_eq!((rect.right(), rect.bottom()), (0, 1080));
        assert!(rect.contains(Point::new(-1920, 0)));
        assert!(rect.contains(Point::new(-1, 1079)));
        assert!(!rect.contains(Point::new(0, 0)));
        assert!(!rect.contains(Point::new(-1, 1080)));
    }

    #[test]
    fn an_intersection_spans_what_both_cover() {
        let left = Rect::new(-1920, 0, 1920, 1080);
        let right = Rect::new(0, 0, 2560, 1440);
        let drag = Rect::new(-200, 900, 500, 400);

        assert_eq!(
            left.intersection(&drag),
            Some(Rect::new(-200, 900, 200, 180))
        );
        assert_eq!(right.intersection(&drag), Some(Rect::new(0, 900, 300, 400)));
        assert_eq!(left.intersection(&right), None); // side by side: they only touch
        assert_eq!(drag.intersection(&drag), Some(drag));
    }

    #[test]
    fn the_bounding_rect_of_monitors_is_the_virtual_screen() {
        let monitors = [
            Rect::new(0, 0, 2560, 1440),
            Rect::new(-1920, 180, 1920, 1080),
            Rect::new(2560, -400, 1080, 1920),
        ];
        assert_eq!(
            Rect::bounding(monitors),
            Some(Rect::new(-1920, -400, 5560, 1920))
        );
        assert_eq!(Rect::bounding([]), None);
    }
}
