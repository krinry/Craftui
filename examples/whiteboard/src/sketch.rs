use craftui::core::Point;
use craftui::render::kurbo::{self, BezPath};

/// Fast deterministic pseudo-random offset based on integer hashing.
/// Ensures hand-drawn roughness remains rock-solid stable across frames.
#[inline]
fn pseudo_rand(seed: u64) -> f32 {
    let mut x = seed.wrapping_mul(0x517cc1b727220a95);
    x ^= x >> 32;
    x = x.wrapping_mul(0x6c62272e07bb0142);
    x ^= x >> 32;
    let norm = (x & 0x00ff_ffff) as f32 / 16777215.0; // 0.0 to 1.0
    norm * 2.0 - 1.0 // -1.0 to 1.0
}

#[inline]
fn jitter_pt(pt: Point, seed: u64, amount: f32) -> Point {
    let jx = pseudo_rand(seed) * amount;
    let jy = pseudo_rand(seed.wrapping_add(1013904223)) * amount;
    Point::new(pt.x + jx, pt.y + jy)
}

/// Generate hand-drawn sketchy segment connecting two points.
/// Emulates Excalidraw / Rough.js double-stroke aesthetic.
pub fn sketch_line_path(start: Point, end: Point, roughness: f32, seed: u64) -> BezPath {
    let mut path = BezPath::new();
    let length = start.distance_to(end);
    if length < 0.5 {
        return path;
    }

    let r = (roughness * 1.5).clamp(0.2, 5.0);

    // Pass 1: Primary line with slight midpoint deflection and overshoot
    let p1_start = jitter_pt(start, seed.wrapping_add(1), r * 0.6);
    let p1_end = jitter_pt(end, seed.wrapping_add(2), r * 0.6);
    let mid1 = Point::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
    let ctrl1 = jitter_pt(mid1, seed.wrapping_add(3), r * 1.2);

    path.move_to(p1_start.to_kurbo());
    path.quad_to(ctrl1.to_kurbo(), p1_end.to_kurbo());

    // Pass 2: Secondary subtle stroke giving authentic pencil/ink feel
    let p2_start = jitter_pt(start, seed.wrapping_add(11), r * 0.8);
    let p2_end = jitter_pt(end, seed.wrapping_add(12), r * 0.8);
    let ctrl2 = jitter_pt(mid1, seed.wrapping_add(13), r * 1.0);

    path.move_to(p2_start.to_kurbo());
    path.quad_to(ctrl2.to_kurbo(), p2_end.to_kurbo());

    path
}

/// Generate hand-drawn arrow matching Excalidraw's style.
/// Shaft line + two angled barbed wings at the destination point.
pub fn sketch_arrow_path(start: Point, end: Point, roughness: f32, seed: u64) -> BezPath {
    let mut path = sketch_line_path(start, end, roughness, seed);
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 4.0 {
        return path;
    }

    // Direction angle
    let angle = dy.atan2(dx);
    let head_len = (18.0 + roughness * 2.0).min(length * 0.45);
    let barb_angle: f32 = 0.45; // ~26 degrees

    // Wing 1
    let wing1_angle = angle + std::f32::consts::PI - barb_angle;
    let wing1_end = Point::new(
        end.x + head_len * wing1_angle.cos(),
        end.y + head_len * wing1_angle.sin(),
    );
    let w1_path = sketch_line_path(end, wing1_end, roughness * 0.9, seed.wrapping_add(100));
    for el in w1_path.elements() {
        path.push(*el);
    }

    // Wing 2
    let wing2_angle = angle + std::f32::consts::PI + barb_angle;
    let wing2_end = Point::new(
        end.x + head_len * wing2_angle.cos(),
        end.y + head_len * wing2_angle.sin(),
    );
    let w2_path = sketch_line_path(end, wing2_end, roughness * 0.9, seed.wrapping_add(200));
    for el in w2_path.elements() {
        path.push(*el);
    }

    path
}

/// Generate hand-drawn rectangle with characteristic sketchy corner overshoots.
pub fn sketch_rect_path(
    origin: Point,
    width: f32,
    height: f32,
    roughness: f32,
    seed: u64,
) -> BezPath {
    let mut path = BezPath::new();
    let r = (roughness * 1.4).clamp(0.2, 4.0);

    let (x, y) = (origin.x, origin.y);
    let (w, h) = (width, height);

    let tl = Point::new(x, y);
    let tr = Point::new(x + w, y);
    let br = Point::new(x + w, y + h);
    let bl = Point::new(x, y + h);

    // Corner overshoot amount (2 - 4 px)
    let os = r * 1.5;

    // Top: Left to Right
    let top_a = Point::new(tl.x - os, tl.y);
    let top_b = Point::new(tr.x + os, tr.y);
    for el in sketch_line_path(top_a, top_b, roughness, seed.wrapping_add(10)).elements() {
        path.push(*el);
    }

    // Right: Top to Bottom
    let right_a = Point::new(tr.x, tr.y - os);
    let right_b = Point::new(br.x, br.y + os);
    for el in sketch_line_path(right_a, right_b, roughness, seed.wrapping_add(20)).elements() {
        path.push(*el);
    }

    // Bottom: Right to Left
    let bot_a = Point::new(br.x + os, br.y);
    let bot_b = Point::new(bl.x - os, bl.y);
    for el in sketch_line_path(bot_a, bot_b, roughness, seed.wrapping_add(30)).elements() {
        path.push(*el);
    }

    // Left: Bottom to Top
    let left_a = Point::new(bl.x, bl.y + os);
    let left_b = Point::new(tl.x, tl.y - os);
    for el in sketch_line_path(left_a, left_b, roughness, seed.wrapping_add(40)).elements() {
        path.push(*el);
    }

    path
}

/// Generate hand-drawn diamond with corner overshoots.
pub fn sketch_diamond_path(
    origin: Point,
    width: f32,
    height: f32,
    roughness: f32,
    seed: u64,
) -> BezPath {
    let mut path = BezPath::new();
    let (x, y) = (origin.x, origin.y);
    let (w, h) = (width, height);

    let top = Point::new(x + w * 0.5, y);
    let right = Point::new(x + w, y + h * 0.5);
    let bottom = Point::new(x + w * 0.5, y + h);
    let left = Point::new(x, y + h * 0.5);

    for el in sketch_line_path(top, right, roughness, seed.wrapping_add(10)).elements() {
        path.push(*el);
    }
    for el in sketch_line_path(right, bottom, roughness, seed.wrapping_add(20)).elements() {
        path.push(*el);
    }
    for el in sketch_line_path(bottom, left, roughness, seed.wrapping_add(30)).elements() {
        path.push(*el);
    }
    for el in sketch_line_path(left, top, roughness, seed.wrapping_add(40)).elements() {
        path.push(*el);
    }

    path
}

/// Generate hand-drawn ellipse using 4 cubic bezier quadrants with organic jitter.
pub fn sketch_ellipse_path(center: Point, rx: f32, ry: f32, roughness: f32, seed: u64) -> BezPath {
    let mut path = BezPath::new();
    let rx = rx.abs().max(1.0);
    let ry = ry.abs().max(1.0);
    let k = 0.552_284_8; // Standard cubic bezier circle approximation constant

    let r = (roughness * 1.2).clamp(0.2, 4.0);

    for pass in 0..2 {
        let p_seed = seed.wrapping_add((pass * 50) as u64);
        let j_rx = rx + pseudo_rand(p_seed.wrapping_add(1)) * r;
        let j_ry = ry + pseudo_rand(p_seed.wrapping_add(2)) * r;
        let c = jitter_pt(center, p_seed.wrapping_add(3), r * 0.5);

        let p_top = Point::new(c.x, c.y - j_ry);
        let p_right = Point::new(c.x + j_rx, c.y);
        let p_bot = Point::new(c.x, c.y + j_ry);
        let p_left = Point::new(c.x - j_rx, c.y);

        let kx = j_rx * k;
        let ky = j_ry * k;

        path.move_to(p_top.to_kurbo());
        // Quadrant 1: Top to Right
        path.curve_to(
            kurbo::Point::new((c.x + kx) as f64, (c.y - j_ry) as f64),
            kurbo::Point::new((c.x + j_rx) as f64, (c.y - ky) as f64),
            p_right.to_kurbo(),
        );
        // Quadrant 2: Right to Bottom
        path.curve_to(
            kurbo::Point::new((c.x + j_rx) as f64, (c.y + ky) as f64),
            kurbo::Point::new((c.x + kx) as f64, (c.y + j_ry) as f64),
            p_bot.to_kurbo(),
        );
        // Quadrant 3: Bottom to Left
        path.curve_to(
            kurbo::Point::new((c.x - kx) as f64, (c.y + j_ry) as f64),
            kurbo::Point::new((c.x - j_rx) as f64, (c.y + ky) as f64),
            p_left.to_kurbo(),
        );
        // Quadrant 4: Left to Top (with slight overshoot loop)
        path.curve_to(
            kurbo::Point::new((c.x - j_rx) as f64, (c.y - ky) as f64),
            kurbo::Point::new((c.x - kx) as f64, (c.y - j_ry) as f64),
            Point::new(p_top.x + pseudo_rand(p_seed.wrapping_add(9)) * r, p_top.y).to_kurbo(),
        );
    }

    path
}

/// Generate smoothed freehand curve from recorded user stroke points.
pub fn sketch_freehand_path(points: &[Point], _roughness: f32, _seed: u64) -> BezPath {
    let mut path = BezPath::new();
    if points.is_empty() {
        return path;
    }
    if points.len() == 1 {
        let p = points[0];
        path.move_to(p.to_kurbo());
        path.line_to(kurbo::Point::new(p.x as f64 + 0.1, p.y as f64 + 0.1));
        return path;
    }

    path.move_to(points[0].to_kurbo());

    if points.len() == 2 {
        path.line_to(points[1].to_kurbo());
        return path;
    }

    // Midpoint quadratic Bezier smoothing for silky smooth stylus / mouse lines
    for i in 1..points.len() - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];
        let mid = Point::new((p0.x + p1.x) * 0.5, (p0.y + p1.y) * 0.5);
        path.quad_to(p0.to_kurbo(), mid.to_kurbo());
    }

    if let Some(&last) = points.last() {
        path.line_to(last.to_kurbo());
    }

    path
}
