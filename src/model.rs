#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn between(a: Point, b: Point) -> Self {
        Self {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            width: (self.x + self.width).max(other.x + other.width) - x,
            height: (self.y + self.height).max(other.y + other.height) - y,
        }
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let width = (self.x + self.width).min(other.x + other.width) - x;
        let height = (self.y + self.height).min(other.y + other.height) - y;
        (width > 0.0 && height > 0.0).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }
}

/// Crop corners use logical desktop coordinates, so hit areas stay consistent on HiDPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl CropHandle {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    pub fn position(self, r: Rect) -> Point {
        Point {
            x: r.x
                + if matches!(self, Self::TopRight | Self::BottomRight) {
                    r.width
                } else {
                    0.
                },
            y: r.y
                + if matches!(self, Self::BottomLeft | Self::BottomRight) {
                    r.height
                } else {
                    0.
                },
        }
    }

    pub fn hit(r: Rect, p: Point) -> Option<Self> {
        Self::ALL
            .into_iter()
            .filter_map(|handle| {
                let c = handle.position(r);
                let dx = (p.x - c.x).abs();
                let dy = (p.y - c.y).abs();
                (dx <= 10. && dy <= 10.).then_some((handle, dx * dx + dy * dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(handle, _)| handle)
    }

    pub fn cursor(self) -> &'static str {
        match self {
            Self::TopLeft | Self::BottomRight => "nwse-resize",
            Self::TopRight | Self::BottomLeft => "nesw-resize",
        }
    }

    pub fn resize(self, r: Rect, p: Point, bounds: Rect) -> Rect {
        let left = matches!(self, Self::TopLeft | Self::BottomLeft);
        let top = matches!(self, Self::TopLeft | Self::TopRight);
        let fixed = Point {
            x: if left { r.x + r.width } else { r.x },
            y: if top { r.y + r.height } else { r.y },
        };
        // Keep the opposite corner fixed and prevent collapsing or flipping the crop.
        let w = r.width.min(1.);
        let h = r.height.min(1.);
        let moving = Point {
            x: if left {
                p.x.clamp(bounds.x, fixed.x - w)
            } else {
                p.x.clamp(fixed.x + w, bounds.x + bounds.width)
            },
            y: if top {
                p.y.clamp(bounds.y, fixed.y - h)
            } else {
                p.y.clamp(fixed.y + h, bounds.y + bounds.height)
            },
        };
        Rect::between(fixed, moving)
    }
}

/// All editing coordinates are desktop logical coordinates, including negative origins.
#[derive(Clone, Debug)]
pub struct Desktop {
    pub bounds: Rect,
    pub monitors: Vec<Rect>,
    pub scale: f64,
}

impl Desktop {
    pub fn new(monitors: Vec<Rect>, pixels: (i32, i32)) -> Result<Self, String> {
        if monitors.is_empty() || monitors.iter().any(|m| m.width <= 0.0 || m.height <= 0.0) {
            return Err("No monitors with valid geometry were found.".into());
        }
        let bounds = monitors.iter().copied().reduce(Rect::union).unwrap();
        let scale = pixels.0 as f64 / bounds.width;
        // The portal must return a complete, uniformly scaled desktop, not a region/window.
        // Allow at most two pixels of compositor rounding, never silently stretch an image.
        if pixels.0 <= 0 || pixels.1 <= 0 || (bounds.height * scale - pixels.1 as f64).abs() > 2.0 {
            return Err(format!(
                "The portal image ({}×{}) does not match the full desktop ({}×{}). Select all screens in the portal, rather than a window or region.",
                pixels.0, pixels.1, bounds.width, bounds.height
            ));
        }
        Ok(Self {
            bounds,
            monitors,
            scale,
        })
    }

    pub fn pixel_crop(&self, selection: Rect) -> Option<(i32, i32, i32, i32)> {
        let r = selection.intersect(self.bounds)?;
        let x = ((r.x - self.bounds.x) * self.scale).floor() as i32;
        let y = ((r.y - self.bounds.y) * self.scale).floor() as i32;
        let right = ((r.x + r.width - self.bounds.x) * self.scale).ceil() as i32;
        let bottom = ((r.y + r.height - self.bounds.y) * self.scale).ceil() as i32;
        Some((x, y, right - x, bottom - y))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Line,
    Rectangle,
    Text,
}

#[derive(Clone, Debug)]
pub struct Style {
    pub color: (f64, f64, f64, f64),
    pub width: f64,
    pub font: String,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: (1.0, 0.25, 0.25, 1.0),
            width: 3.0,
            font: "Sans Bold 20".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Shape {
    Line(Point, Point),
    Rectangle(Rect),
    Text(Point, String),
}

#[derive(Clone, Debug)]
pub struct Annotation {
    pub shape: Shape,
    pub style: Style,
}

#[derive(Default)]
pub struct Document {
    pub selection: Option<Rect>,
    pub annotations: Vec<Annotation>,
    pub(crate) redo: Vec<Annotation>,
}

impl Document {
    pub fn add(&mut self, annotation: Annotation) {
        self.annotations.push(annotation);
        self.redo.clear();
    }
    pub fn undo(&mut self) {
        if let Some(a) = self.annotations.pop() {
            self.redo.push(a);
        }
    }
    pub fn redo(&mut self) {
        if let Some(a) = self.redo.pop() {
            self.annotations.push(a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }
    #[test]
    fn crop_handles_resize_each_corner_with_negative_coordinates() {
        let r = rect(-100., -50., 200., 100.);
        let bounds = rect(-1920., -1080., 5760., 2160.);
        for (handle, expected) in [
            (CropHandle::TopLeft, rect(-80., -30., 180., 80.)),
            (CropHandle::TopRight, rect(-100., -30., 220., 80.)),
            (CropHandle::BottomLeft, rect(-80., -50., 180., 120.)),
            (CropHandle::BottomRight, rect(-100., -50., 220., 120.)),
        ] {
            let p = handle.position(r);
            assert_eq!(CropHandle::hit(r, p), Some(handle));
            assert_eq!(
                handle.resize(
                    r,
                    Point {
                        x: p.x + 20.,
                        y: p.y + 20.
                    },
                    bounds
                ),
                expected
            );
        }
        assert_eq!(CropHandle::hit(r, Point::default()), None);
        let tiny = rect(0., 0., 3., 3.);
        assert_eq!(
            CropHandle::hit(tiny, Point { x: 3., y: 3. }),
            Some(CropHandle::BottomRight)
        );
    }

    #[test]
    fn crop_handles_clamp_and_do_not_collapse() {
        let bounds = rect(-100., -100., 300., 300.);
        let r = rect(0., 0., 100., 100.);
        assert_eq!(
            CropHandle::TopLeft.resize(r, Point { x: -999., y: -999. }, bounds),
            rect(-100., -100., 200., 200.)
        );
        assert_eq!(
            CropHandle::BottomRight.resize(r, Point { x: 999., y: 999. }, bounds),
            rect(0., 0., 200., 200.)
        );
        assert_eq!(
            CropHandle::TopLeft.resize(r, Point { x: 999., y: 999. }, bounds),
            rect(99., 99., 1., 1.)
        );
        assert_eq!(
            CropHandle::BottomRight.resize(r, Point { x: -999., y: -999. }, bounds),
            rect(0., 0., 1., 1.)
        );
    }

    #[test]
    fn three_monitors_negative_and_stacked() {
        let d = Desktop::new(
            vec![
                rect(-1920., 0., 1920., 1080.),
                rect(0., 0., 2560., 1440.),
                rect(0., -1200., 1920., 1200.),
            ],
            (4480, 2640),
        )
        .unwrap();
        assert_eq!(d.bounds, rect(-1920., -1200., 4480., 2640.));
        assert_eq!(
            d.pixel_crop(rect(-100., -100., 200., 200.)),
            Some((1820, 1100, 200, 200))
        );
    }
    #[test]
    fn fractional_scale_and_crop_clamping() {
        let d = Desktop::new(
            vec![
                rect(-100., 0., 100., 80.),
                rect(0., 0., 100., 80.),
                rect(100., 0., 100., 80.),
            ],
            (450, 120),
        )
        .unwrap();
        assert_eq!(
            d.pixel_crop(rect(-101., 0., 11., 10.)),
            Some((0, 0, 15, 15))
        );
        assert_eq!(d.pixel_crop(rect(201., 0., 10., 10.)), None);
    }
    #[test]
    fn rejects_region_or_nonuniform_portal_image() {
        assert!(Desktop::new(vec![rect(0., 0., 5760., 1080.)], (1920, 1080)).is_err());
        assert!(Desktop::new(vec![], (1920, 1080)).is_err());
    }
    #[test]
    fn reverse_drag_and_cross_monitor_selection() {
        assert_eq!(
            Rect::between(Point { x: 100., y: 50. }, Point { x: -100., y: -50. }),
            rect(-100., -50., 200., 100.)
        );
    }
    #[test]
    fn new_annotation_invalidates_redo() {
        let mut doc = Document::default();
        let a = Annotation {
            shape: Shape::Line(Point::default(), Point { x: 10., y: 10. }),
            style: Style::default(),
        };
        doc.add(a.clone());
        doc.undo();
        assert!(doc.annotations.is_empty());
        doc.redo();
        assert_eq!(doc.annotations.len(), 1);
        doc.undo();
        doc.add(a);
        doc.redo();
        assert_eq!(doc.annotations.len(), 1);
    }
}
