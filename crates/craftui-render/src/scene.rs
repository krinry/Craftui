use craftui_core::{
    BlendMode, BoxShadow, Brush, Color, CornerRadii, Fit, PlatformViewId, Point, Rect, TextureId,
    Transform2D,
};
use craftui_text::{RichText, TextStyle};
use kurbo::{Affine, BezPath};

/// Backend-agnostic 2D paint command recorded from the retained widget tree.
#[derive(Debug, Clone)]
pub enum PaintCommand {
    FillRect {
        rect: Rect,
        brush: Brush,
        radius: CornerRadii,
    },
    StrokeRect {
        rect: Rect,
        color: Color,
        width: f32,
        radius: CornerRadii,
    },
    DropShadow {
        rect: Rect,
        radius: CornerRadii,
        std_dev: f32,
        spread: f32,
        inset: bool,
        offset_x: f32,
        offset_y: f32,
        color: Color,
    },
    BackdropBlur {
        rect: Rect,
        radius: CornerRadii,
        blur_radius: f32,
        tint: Color,
    },
    FillPath {
        path: BezPath,
        brush: Brush,
    },
    StrokePath {
        path: BezPath,
        color: Color,
        width: f32,
    },
    DrawImage {
        rect: Rect,
        texture: TextureId,
        fit: Fit,
        radius: CornerRadii,
        opacity: f32,
    },
    DrawText {
        origin: Point,
        text: String,
        style: TextStyle,
        max_width: Option<f32>,
    },
    /// One shaped paragraph, with styling applied across UTF-8 span ranges.
    DrawRichText {
        origin: Point,
        text: RichText,
        max_width: Option<f32>,
    },
    PushClip {
        rect: Rect,
        radius: CornerRadii,
    },
    PopClip,
    PushLayer {
        rect: Rect,
        opacity: f32,
        blend_mode: BlendMode,
    },
    PopLayer,
    PushTransform {
        transform: Transform2D,
    },
    PopTransform,
    PlatformViewHole {
        id: PlatformViewId,
        rect: Rect,
    },
}

#[cfg(any(
    feature = "cpu-vello",
    feature = "cpu-tiny-skia",
    feature = "gpu-vello",
    feature = "gpu-sdf"
))]
impl PaintCommand {
    /// Plain text uses its runtime color; rich text uses each glyph run's brush.
    pub(crate) fn shaped_text(
        &self,
        engine: &mut craftui_text::TextEngine,
    ) -> Option<(std::sync::Arc<parley::Layout<Color>>, Option<Color>)> {
        match self {
            Self::DrawText {
                text,
                style,
                max_width,
                ..
            } if !text.is_empty() => Some((
                engine.build_layout_cached(text, style, *max_width),
                Some(style.color),
            )),
            Self::DrawRichText {
                text, max_width, ..
            } if !text.is_empty() => Some((
                std::sync::Arc::new(engine.build_rich_layout(text, *max_width)),
                None,
            )),
            _ => None,
        }
    }
}

/// Resolve an underline in paragraph-local coordinates using the shaped font metrics.
#[cfg(any(
    feature = "cpu-vello",
    feature = "cpu-tiny-skia",
    feature = "gpu-vello",
    feature = "gpu-sdf"
))]
pub(crate) fn underline_rect(
    glyph_run: &parley::GlyphRun<'_, Color>,
    color_override: Option<Color>,
) -> Option<(Rect, Color)> {
    let decoration = glyph_run.style().underline.as_ref()?;
    let metrics = glyph_run.run().font_metrics();
    let offset = decoration.offset.unwrap_or(metrics.underline_offset);
    let thickness = decoration.size.unwrap_or(metrics.underline_size).max(0.5);
    Some((
        Rect::new(
            glyph_run.offset(),
            glyph_run.baseline() - offset,
            glyph_run.advance(),
            thickness,
        ),
        color_override.unwrap_or(decoration.brush),
    ))
}

/// Recorded vector scene for a single frame.
#[derive(Debug, Clone, Default)]
pub struct PaintScene {
    pub commands: Vec<PaintCommand>,
}

impl PaintScene {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }

    pub fn push(&mut self, cmd: PaintCommand) {
        self.commands.push(cmd);
    }

    pub fn fill_rect(&mut self, rect: Rect, brush: impl Into<Brush>, radius: CornerRadii) {
        self.commands.push(PaintCommand::FillRect {
            rect,
            brush: brush.into(),
            radius,
        });
    }

    pub fn stroke_rect(&mut self, rect: Rect, color: Color, width: f32, radius: CornerRadii) {
        self.commands.push(PaintCommand::StrokeRect {
            rect,
            color,
            width,
            radius,
        });
    }

    pub fn drop_shadow(&mut self, rect: Rect, radius: CornerRadii, shadow: BoxShadow) {
        self.commands.push(PaintCommand::DropShadow {
            rect,
            radius,
            std_dev: shadow.blur_radius,
            spread: shadow.spread,
            inset: shadow.inset,
            offset_x: shadow.offset_x,
            offset_y: shadow.offset_y,
            color: shadow.color,
        });
    }

    pub fn backdrop_blur(
        &mut self,
        rect: Rect,
        radius: CornerRadii,
        blur_radius: f32,
        tint: Color,
    ) {
        self.commands.push(PaintCommand::BackdropBlur {
            rect,
            radius,
            blur_radius,
            tint,
        });
    }

    pub fn push_transform(&mut self, transform: Transform2D) {
        self.commands
            .push(PaintCommand::PushTransform { transform });
    }

    pub fn pop_transform(&mut self) {
        self.commands.push(PaintCommand::PopTransform);
    }

    pub fn push_clip(&mut self, rect: Rect, radius: CornerRadii) {
        self.commands.push(PaintCommand::PushClip { rect, radius });
    }

    pub fn pop_clip(&mut self) {
        self.commands.push(PaintCommand::PopClip);
    }

    pub fn push_layer(&mut self, rect: Rect, opacity: f32, blend_mode: BlendMode) {
        self.commands.push(PaintCommand::PushLayer {
            rect,
            opacity,
            blend_mode,
        });
    }

    pub fn pop_layer(&mut self) {
        self.commands.push(PaintCommand::PopLayer);
    }

    pub fn fill_path(&mut self, path: BezPath, brush: impl Into<Brush>) {
        self.commands.push(PaintCommand::FillPath {
            path,
            brush: brush.into(),
        });
    }

    pub fn stroke_path(&mut self, path: BezPath, color: Color, width: f32) {
        self.commands
            .push(PaintCommand::StrokePath { path, color, width });
    }

    /// Draw a single shaped paragraph with per-span font and color styles.
    pub fn draw_rich_text(&mut self, origin: Point, text: RichText, max_width: Option<f32>) {
        self.commands.push(PaintCommand::DrawRichText {
            origin,
            text,
            max_width,
        });
    }

    pub fn draw_text(
        &mut self,
        origin: Point,
        text: impl Into<String>,
        style: TextStyle,
        max_width: Option<f32>,
    ) {
        self.commands.push(PaintCommand::DrawText {
            origin,
            text: text.into(),
            style,
            max_width,
        });
    }
}

/// Compute destination rectangle & affine transform for fitting an image `(img_w, img_h)` into `bounds`.
pub fn compute_image_transform(bounds: Rect, img_w: u32, img_h: u32, fit: Fit) -> (Rect, Affine) {
    let iw = img_w.max(1) as f64;
    let ih = img_h.max(1) as f64;
    let bw = bounds.width.max(1.0) as f64;
    let bh = bounds.height.max(1.0) as f64;

    let (sx, sy) = match fit {
        Fit::Fill => (bw / iw, bh / ih),
        Fit::Contain => {
            let s = (bw / iw).min(bh / ih);
            (s, s)
        }
        Fit::Cover => {
            let s = (bw / iw).max(bh / ih);
            (s, s)
        }
        Fit::None => (1.0, 1.0),
    };

    let drawn_w = iw * sx;
    let drawn_h = ih * sy;
    let ox = bounds.x as f64 + (bw - drawn_w) * 0.5;
    let oy = bounds.y as f64 + (bh - drawn_h) * 0.5;

    let draw_rect = Rect::new(ox as f32, oy as f32, drawn_w as f32, drawn_h as f32);
    let affine = Affine::translate((ox, oy)) * Affine::scale_non_uniform(sx, sy);
    (draw_rect, affine)
}

/// Convert a [`craftui_core::Brush`] into a [`peniko::Brush`].
pub fn brush_to_peniko(brush: &Brush) -> peniko::Brush {
    match brush {
        Brush::Solid(c) => peniko::Brush::Solid(c.to_peniko()),
        Brush::LinearGradient { start, end, stops } => {
            let mut color_stops = peniko::ColorStops::new();
            for s in stops {
                color_stops.push(peniko::ColorStop::from((s.offset, s.color.to_peniko())));
            }
            peniko::Brush::Gradient(peniko::Gradient {
                kind: peniko::LinearGradientPosition {
                    start: start.to_kurbo(),
                    end: end.to_kurbo(),
                }
                .into(),
                stops: color_stops,
                extend: peniko::Extend::Pad,
                ..Default::default()
            })
        }
        Brush::RadialGradient {
            center,
            radius,
            stops,
        } => {
            let mut color_stops = peniko::ColorStops::new();
            for s in stops {
                color_stops.push(peniko::ColorStop::from((s.offset, s.color.to_peniko())));
            }
            peniko::Brush::Gradient(peniko::Gradient {
                kind: peniko::RadialGradientPosition {
                    start_center: center.to_kurbo(),
                    start_radius: 0.0,
                    end_center: center.to_kurbo(),
                    end_radius: *radius,
                }
                .into(),
                stops: color_stops,
                extend: peniko::Extend::Pad,
                ..Default::default()
            })
        }
    }
}
