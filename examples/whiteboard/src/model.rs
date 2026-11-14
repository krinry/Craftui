use craftui::core::{Color, Point, Rect};
use serde::{Deserialize, Serialize};

/// Whiteboard drawing tools matching Excalidraw's primary tool ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tool {
    #[default]
    Select,
    Hand,
    Rectangle,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Pen,
    Text,
    Eraser,
}

impl Tool {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Select => "↖",
            Self::Hand => "✋",
            Self::Rectangle => "▭",
            Self::Diamond => "◇",
            Self::Ellipse => "◯",
            Self::Arrow => "➔",
            Self::Line => "—",
            Self::Pen => "✏",
            Self::Text => "T",
            Self::Eraser => "⌫",
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            Self::Select => "V",
            Self::Hand => "H",
            Self::Rectangle => "R",
            Self::Diamond => "D",
            Self::Ellipse => "O",
            Self::Arrow => "A",
            Self::Line => "L",
            Self::Pen => "P",
            Self::Text => "T",
            Self::Eraser => "E",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Select => "Selection",
            Self::Hand => "Hand (Pan)",
            Self::Rectangle => "Rectangle",
            Self::Diamond => "Diamond",
            Self::Ellipse => "Ellipse",
            Self::Arrow => "Arrow",
            Self::Line => "Line",
            Self::Pen => "Draw",
            Self::Text => "Text",
            Self::Eraser => "Eraser",
        }
    }
}

/// Geometric shape kinds with local dimensional properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElementKind {
    Rectangle {
        width: f32,
        height: f32,
        roundness: f32,
    },
    Diamond {
        width: f32,
        height: f32,
    },
    Ellipse {
        radius_x: f32,
        radius_y: f32,
    },
    Arrow {
        end: Point,
    },
    Line {
        end: Point,
    },
    Freehand {
        points: Vec<Point>,
    },
    Text {
        content: String,
        font_size: f32,
    },
}

/// A single discrete entity placed upon the infinite whiteboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhiteboardElement {
    pub id: u64,
    pub origin: Point,
    pub kind: ElementKind,
    pub stroke_color: Color,
    pub fill_color: Option<Color>,
    pub stroke_width: f32,
    pub roughness: f32,
    #[serde(skip)]
    pub selected: bool,
}

impl WhiteboardElement {
    pub fn new(
        id: u64,
        origin: Point,
        kind: ElementKind,
        stroke_color: Color,
        fill_color: Option<Color>,
        stroke_width: f32,
    ) -> Self {
        Self {
            id,
            origin,
            kind,
            stroke_color,
            fill_color,
            stroke_width,
            roughness: 1.0,
            selected: false,
        }
    }

    /// Compute world-space axis-aligned bounding box.
    pub fn bounds(&self) -> Rect {
        let pad = (self.stroke_width * 0.5 + 4.0).max(4.0);
        match &self.kind {
            ElementKind::Rectangle { width, height, .. } => {
                let min_x = self.origin.x.min(self.origin.x + width);
                let min_y = self.origin.y.min(self.origin.y + height);
                Rect::new(
                    min_x - pad,
                    min_y - pad,
                    width.abs() + pad * 2.0,
                    height.abs() + pad * 2.0,
                )
            }
            ElementKind::Diamond { width, height } => {
                let min_x = self.origin.x.min(self.origin.x + width);
                let min_y = self.origin.y.min(self.origin.y + height);
                Rect::new(
                    min_x - pad,
                    min_y - pad,
                    width.abs() + pad * 2.0,
                    height.abs() + pad * 2.0,
                )
            }
            ElementKind::Ellipse { radius_x, radius_y } => {
                let rx = radius_x.abs();
                let ry = radius_y.abs();
                Rect::new(
                    self.origin.x - rx - pad,
                    self.origin.y - ry - pad,
                    rx * 2.0 + pad * 2.0,
                    ry * 2.0 + pad * 2.0,
                )
            }
            ElementKind::Arrow { end } | ElementKind::Line { end } => {
                let min_x = self.origin.x.min(end.x);
                let max_x = self.origin.x.max(end.x);
                let min_y = self.origin.y.min(end.y);
                let max_y = self.origin.y.max(end.y);
                Rect::new(
                    min_x - pad,
                    min_y - pad,
                    (max_x - min_x).max(1.0) + pad * 2.0,
                    (max_y - min_y).max(1.0) + pad * 2.0,
                )
            }
            ElementKind::Freehand { points } => {
                if points.is_empty() {
                    return Rect::new(
                        self.origin.x - pad,
                        self.origin.y - pad,
                        pad * 2.0,
                        pad * 2.0,
                    );
                }
                let mut min_x = self.origin.x;
                let mut max_x = self.origin.x;
                let mut min_y = self.origin.y;
                let mut max_y = self.origin.y;
                for p in points {
                    min_x = min_x.min(p.x);
                    max_x = max_x.max(p.x);
                    min_y = min_y.min(p.y);
                    max_y = max_y.max(p.y);
                }
                Rect::new(
                    min_x - pad,
                    min_y - pad,
                    (max_x - min_x).max(1.0) + pad * 2.0,
                    (max_y - min_y).max(1.0) + pad * 2.0,
                )
            }
            ElementKind::Text { content, font_size } => {
                let approx_w = content.chars().count() as f32 * font_size * 0.6;
                let approx_h = *font_size * 1.3;
                Rect::new(
                    self.origin.x - pad,
                    self.origin.y - pad,
                    approx_w + pad * 2.0,
                    approx_h + pad * 2.0,
                )
            }
        }
    }

    /// Hit-test predicate determining if a world-space point touches this element.
    pub fn contains_point(&self, pt: Point, tolerance: f32) -> bool {
        let b = self.bounds().inflate(tolerance, tolerance);
        if !b.contains(pt) {
            return false;
        }

        match &self.kind {
            ElementKind::Rectangle { .. }
            | ElementKind::Diamond { .. }
            | ElementKind::Text { .. } => b.contains(pt),
            ElementKind::Ellipse { radius_x, radius_y } => {
                let dx = (pt.x - self.origin.x) / radius_x.max(1.0);
                let dy = (pt.y - self.origin.y) / radius_y.max(1.0);
                (dx * dx + dy * dy) <= 1.25
            }
            ElementKind::Arrow { end } | ElementKind::Line { end } => {
                Self::dist_to_line_segment(pt, self.origin, *end)
                    <= (self.stroke_width * 0.5 + tolerance)
            }
            ElementKind::Freehand { points } => {
                if points.is_empty() {
                    return pt.distance_to(self.origin) <= tolerance;
                }
                for window in points.windows(2) {
                    if Self::dist_to_line_segment(pt, window[0], window[1])
                        <= (self.stroke_width * 0.5 + tolerance)
                    {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Translate element in world coordinates.
    pub fn translate(&mut self, delta: Point) {
        self.origin.x += delta.x;
        self.origin.y += delta.y;
        match &mut self.kind {
            ElementKind::Arrow { end } | ElementKind::Line { end } => {
                end.x += delta.x;
                end.y += delta.y;
            }
            ElementKind::Freehand { points } => {
                for p in points {
                    p.x += delta.x;
                    p.y += delta.y;
                }
            }
            _ => {}
        }
    }

    fn dist_to_line_segment(p: Point, a: Point, b: Point) -> f32 {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let l2 = dx * dx + dy * dy;
        if l2 == 0.0 {
            return p.distance_to(a);
        }
        let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0);
        let proj = Point::new(a.x + t * dx, a.y + t * dy);
        p.distance_to(proj)
    }
}

/// Canvas 2D view transformation (pan and zoom).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    pub pan: Point,
    pub zoom: f32,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            pan: Point::new(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl ViewTransform {
    pub fn screen_to_world(&self, pt: Point) -> Point {
        Point::new(
            (pt.x - self.pan.x) / self.zoom.max(0.01),
            (pt.y - self.pan.y) / self.zoom.max(0.01),
        )
    }

    pub fn world_to_screen(&self, pt: Point) -> Point {
        Point::new(pt.x * self.zoom + self.pan.x, pt.y * self.zoom + self.pan.y)
    }

    pub fn zoom_at(&mut self, factor: f32, focal_screen: Point) {
        let old_zoom = self.zoom;
        let new_zoom = (old_zoom * factor).clamp(0.15, 6.0);
        let world_focus = self.screen_to_world(focal_screen);
        self.zoom = new_zoom;
        self.pan = Point::new(
            focal_screen.x - world_focus.x * new_zoom,
            focal_screen.y - world_focus.y * new_zoom,
        );
    }
}

/// Undo / Redo history manager.
#[derive(Debug, Default, Clone)]
pub struct HistoryManager {
    undo_stack: Vec<Vec<WhiteboardElement>>,
    redo_stack: Vec<Vec<WhiteboardElement>>,
}

impl HistoryManager {
    pub fn push(&mut self, snapshot: Vec<WhiteboardElement>) {
        self.undo_stack.push(snapshot);
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, current: Vec<WhiteboardElement>) -> Option<Vec<WhiteboardElement>> {
        let prev = self.undo_stack.pop()?;
        self.redo_stack.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: Vec<WhiteboardElement>) -> Option<Vec<WhiteboardElement>> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

/// Standard Excalidraw Color Swatches.
pub const EXCALI_PALETTE: &[(&str, Color)] = &[
    ("Black", Color::rgb(30, 30, 30)),
    ("Red", Color::rgb(224, 49, 49)),
    ("Green", Color::rgb(47, 158, 68)),
    ("Blue", Color::rgb(25, 113, 194)),
    ("Orange", Color::rgb(240, 140, 0)),
    ("Violet", Color::rgb(156, 54, 181)),
    ("Teal", Color::rgb(18, 184, 134)),
    ("Gray", Color::rgb(134, 142, 150)),
];
