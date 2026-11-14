use craftui::core::Rect;
use craftui::render::kurbo::BezPath;

use crate::model::{ElementKind, WhiteboardElement};
use crate::sketch::{
    sketch_arrow_path, sketch_diamond_path, sketch_ellipse_path, sketch_freehand_path,
    sketch_line_path, sketch_rect_path,
};

/// Export elements to pretty-printed JSON string.
pub fn elements_to_json(elements: &[WhiteboardElement]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(elements)
}

/// Import elements from JSON string.
pub fn elements_from_json(json_str: &str) -> Result<Vec<WhiteboardElement>, serde_json::Error> {
    serde_json::from_str(json_str)
}

/// Helper converting a kurbo::BezPath into standard SVG path data 'd' string.
fn bezpath_to_svg_d(path: &BezPath) -> String {
    use craftui::render::kurbo::PathEl;
    let mut d = String::new();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                d.push_str(&format!("M {:.2} {:.2} ", p.x, p.y));
            }
            PathEl::LineTo(p) => {
                d.push_str(&format!("L {:.2} {:.2} ", p.x, p.y));
            }
            PathEl::QuadTo(p1, p2) => {
                d.push_str(&format!(
                    "Q {:.2} {:.2} {:.2} {:.2} ",
                    p1.x, p1.y, p2.x, p2.y
                ));
            }
            PathEl::CurveTo(p1, p2, p3) => {
                d.push_str(&format!(
                    "C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
                    p1.x, p1.y, p2.x, p2.y, p3.x, p3.y
                ));
            }
            PathEl::ClosePath => {
                d.push_str("Z ");
            }
        }
    }
    d
}

/// Convert whiteboard elements into standalone SVG document string.
pub fn elements_to_svg(elements: &[WhiteboardElement], bounds: Rect) -> String {
    let mut svg = String::new();
    let width = bounds.width.max(100.0);
    let height = bounds.height.max(100.0);

    svg.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">
  <rect width="100%" height="100%" fill="#ffffff" />
  <g stroke-linecap="round" stroke-linejoin="round">
"##
    ));

    for elem in elements {
        let hex_stroke = format!(
            "#{:02x}{:02x}{:02x}",
            elem.stroke_color.r, elem.stroke_color.g, elem.stroke_color.b
        );
        let fill_str = if let Some(fill) = elem.fill_color {
            format!(
                "#{format_hex:02x}{format_hex2:02x}{format_hex3:02x}",
                format_hex = fill.r,
                format_hex2 = fill.g,
                format_hex3 = fill.b
            )
        } else {
            "none".to_string()
        };
        let seed = elem.id.wrapping_mul(31);

        match &elem.kind {
            ElementKind::Rectangle {
                width: w,
                height: h,
                ..
            } => {
                let path = sketch_rect_path(elem.origin, *w, *h, elem.roughness, seed);
                let d = bezpath_to_svg_d(&path);
                svg.push_str(&format!(
                    r#"    <path d="{d}" stroke="{hex_stroke}" stroke-width="{}" fill="{fill_str}" />{sep}"#,
                    elem.stroke_width,
                    sep = "\n"
                ));
            }
            ElementKind::Diamond {
                width: w,
                height: h,
            } => {
                let path = sketch_diamond_path(elem.origin, *w, *h, elem.roughness, seed);
                let d = bezpath_to_svg_d(&path);
                svg.push_str(&format!(
                    r#"    <path d="{d}" stroke="{hex_stroke}" stroke-width="{}" fill="{fill_str}" />{sep}"#,
                    elem.stroke_width,
                    sep = "\n"
                ));
            }
            ElementKind::Ellipse { radius_x, radius_y } => {
                let path =
                    sketch_ellipse_path(elem.origin, *radius_x, *radius_y, elem.roughness, seed);
                let d = bezpath_to_svg_d(&path);
                svg.push_str(&format!(
                    r#"    <path d="{d}" stroke="{hex_stroke}" stroke-width="{}" fill="{fill_str}" />{sep}"#,
                    elem.stroke_width,
                    sep = "\n"
                ));
            }
            ElementKind::Arrow { end } => {
                let path = sketch_arrow_path(elem.origin, *end, elem.roughness, seed);
                let d = bezpath_to_svg_d(&path);
                svg.push_str(&format!(
                    r#"    <path d="{d}" stroke="{hex_stroke}" stroke-width="{}" fill="none" />{sep}"#,
                    elem.stroke_width,
                    sep = "\n"
                ));
            }
            ElementKind::Line { end } => {
                let path = sketch_line_path(elem.origin, *end, elem.roughness, seed);
                let d = bezpath_to_svg_d(&path);
                svg.push_str(&format!(
                    r#"    <path d="{d}" stroke="{hex_stroke}" stroke-width="{}" fill="none" />{sep}"#,
                    elem.stroke_width,
                    sep = "\n"
                ));
            }
            ElementKind::Freehand { points } => {
                let mut all_pts = Vec::with_capacity(points.len() + 1);
                all_pts.push(elem.origin);
                all_pts.extend_from_slice(points);
                let path = sketch_freehand_path(&all_pts, elem.roughness, seed);
                let d = bezpath_to_svg_d(&path);
                svg.push_str(&format!(
                    r#"    <path d="{d}" stroke="{hex_stroke}" stroke-width="{}" fill="none" />{sep}"#,
                    elem.stroke_width,
                    sep = "\n"
                ));
            }
            ElementKind::Text { content, font_size } => {
                svg.push_str(&format!(
                    r#"    <text x="{}" y="{}" font-size="{}" fill="{hex_stroke}" font-family="sans-serif">{}</text>{sep}"#,
                    elem.origin.x,
                    elem.origin.y + font_size,
                    font_size,
                    content,
                    sep = "\n"
                ));
            }
        }
    }

    svg.push_str("  </g>\n</svg>\n");
    svg
}
