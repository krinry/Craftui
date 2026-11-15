#[cfg(feature = "cpu-vello")]
use crate::scene::brush_to_peniko;
use crate::scene::{PaintCommand, PaintScene, compute_image_transform};
use crate::texture::TextureRegistry;
#[cfg(feature = "cpu-vello")]
use craftui_core::TextureId;
#[cfg(any(feature = "cpu-vello", feature = "cpu-tiny-skia"))]
use craftui_core::{BlendMode, ColorStop, CornerRadii};
use craftui_core::{Brush, Color, Rect, Transform2D};
use craftui_text::TextEngine;

#[cfg(any(feature = "cpu-vello", feature = "cpu-tiny-skia"))]
use crate::glyph_raster::separable_box_blur_rgba;

#[cfg(feature = "cpu-vello")]
mod vello_cpu_impl {
    use super::*;
    use kurbo::{Affine, Shape, Stroke};
    use parley::PositionedLayoutItem;
    use std::collections::HashMap;
    use std::sync::Arc;
    use vello_cpu::{
        Image as VelloCpuImage, ImageSource, Level, PaintType, Pixmap, RasterizerSettings,
        RenderContext, RenderSettings, Resources, TargetInit,
    };

    struct CachedCpuPixmap {
        version: u64,
        pixmap: Arc<Pixmap>,
    }

    /// CPU software vector renderer backed by `vello_cpu`.
    pub struct VelloCpuBackend {
        ctx: RenderContext,
        resources: Resources,
        target: Pixmap,
        cached_images: HashMap<TextureId, CachedCpuPixmap>,
    }

    impl Default for VelloCpuBackend {
        fn default() -> Self {
            Self::new(1, 1)
        }
    }

    impl VelloCpuBackend {
        pub fn new(width: u32, height: u32) -> Self {
            let w = width.clamp(1, u16::MAX as u32) as u16;
            let h = height.clamp(1, u16::MAX as u32) as u16;
            let settings = RenderSettings {
                level: Level::new(),
                num_threads: 0,
            };
            Self {
                ctx: RenderContext::new_with(w, h, settings),
                resources: Resources::new(),
                target: Pixmap::new(w, h),
                cached_images: HashMap::new(),
            }
        }

        fn ensure_size(&mut self, width: u32, height: u32) -> (u16, u16) {
            let w = width.clamp(1, u16::MAX as u32) as u16;
            let h = height.clamp(1, u16::MAX as u32) as u16;
            if self.ctx.width() != w || self.ctx.height() != h {
                self.ctx.reset_and_resize(w, h);
                self.target = Pixmap::new(w, h);
            } else {
                self.ctx.reset();
            }
            (w, h)
        }

        fn get_or_upload_pixmap(
            &mut self,
            id: TextureId,
            textures: &TextureRegistry,
        ) -> Option<Arc<Pixmap>> {
            let handle = textures.get(id)?;
            let ver = handle.version();
            if let Some(entry) = self.cached_images.get(&id)
                && entry.version == ver
            {
                return Some(entry.pixmap.clone());
            }
            let (w, h, ver, rgba) = handle.snapshot();
            if w == 0 || h == 0 {
                return None;
            }
            let pw = w.min(u16::MAX as u32) as u16;
            let ph = h.min(u16::MAX as u32) as u16;
            let mut pixmap = Pixmap::new(pw, ph);
            let dst = pixmap.data_as_u8_slice_mut();
            let copy_len = dst.len().min(rgba.len());
            let (dst_chunks, _) = dst[..copy_len].as_chunks_mut::<4>();
            let (src_chunks, _) = rgba[..copy_len].as_chunks::<4>();
            for (chunk_dst, chunk_src) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
                let a = chunk_src[3] as u16;
                chunk_dst[0] = ((chunk_src[0] as u16 * a) / 255) as u8;
                chunk_dst[1] = ((chunk_src[1] as u16 * a) / 255) as u8;
                chunk_dst[2] = ((chunk_src[2] as u16 * a) / 255) as u8;
                chunk_dst[3] = chunk_src[3];
            }
            pixmap.recompute_may_have_transparency();
            let arc = Arc::new(pixmap);
            self.cached_images.insert(
                id,
                CachedCpuPixmap {
                    version: ver,
                    pixmap: arc.clone(),
                },
            );
            Some(arc)
        }

        fn flush_batch(&mut self, rendered_any: &mut bool) {
            self.ctx.flush();
            let settings = RasterizerSettings {
                target_init: if *rendered_any {
                    TargetInit::SrcOver
                } else {
                    TargetInit::Clear(vello_cpu::peniko::Color::TRANSPARENT)
                },
                ..Default::default()
            };
            self.ctx
                .render_with(&mut self.target, &mut self.resources, settings);
            self.ctx.reset();
            *rendered_any = true;
        }

        /// Rasterize `scene` into premultiplied RGBA8 pixels.
        #[allow(clippy::too_many_arguments)]
        pub fn render_premul(
            &mut self,
            scene: &PaintScene,
            width: u32,
            height: u32,
            scale_factor: f32,
            background: Color,
            textures: &TextureRegistry,
            text_engine: &mut TextEngine,
        ) -> &[u8] {
            let (w, h) = self.ensure_size(width, height);
            let scale = scale_factor.max(1.0);
            let base_scale = Affine::scale(scale as f64);

            self.ctx.set_transform(Affine::IDENTITY);
            self.ctx.set_paint(background.to_peniko());
            self.ctx
                .fill_rect(&kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));

            let mut xform_stack = vec![Transform2D::IDENTITY];
            let mut clip_stack_depth = 0usize;
            let mut layer_stack_depth = 0usize;
            let mut rendered_any = false;

            for cmd in &scene.commands {
                let cur_xform = *xform_stack.last().unwrap();
                let eff_xform = base_scale * cur_xform.to_kurbo();

                match cmd {
                    PaintCommand::FillRect {
                        rect,
                        brush,
                        radius,
                    } => {
                        self.ctx.set_transform(eff_xform);
                        self.ctx.reset_paint_transform();
                        apply_paint(&mut self.ctx, brush);
                        if radius.is_zero() {
                            self.ctx.fill_rect(&rect.to_kurbo());
                        } else {
                            let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.fill_path(&path);
                        }
                    }
                    PaintCommand::StrokeRect {
                        rect,
                        color,
                        width,
                        radius,
                    } => {
                        self.ctx.set_transform(eff_xform);
                        self.ctx.reset_paint_transform();
                        self.ctx.set_paint(color.to_peniko());
                        self.ctx.set_stroke(Stroke::new(*width as f64));
                        if radius.is_zero() {
                            self.ctx.stroke_rect(&rect.to_kurbo());
                        } else {
                            let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.stroke_path(&path);
                        }
                    }
                    PaintCommand::DropShadow {
                        rect,
                        radius,
                        std_dev,
                        spread,
                        inset,
                        offset_x,
                        offset_y,
                        color,
                    } => {
                        self.ctx.set_transform(eff_xform);
                        self.ctx.reset_paint_transform();
                        self.ctx.set_paint(color.to_peniko());
                        let avg_r = (radius.top_left
                            + radius.top_right
                            + radius.bottom_right
                            + radius.bottom_left)
                            * 0.25;
                        if *inset {
                            let clip_path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.push_clip_layer(&clip_path);
                            let cut = rect
                                .translate(*offset_x, *offset_y)
                                .inflate(-*spread, -*spread)
                                .to_kurbo();
                            let adj_r = (avg_r - *spread).max(0.0);
                            self.ctx
                                .fill_blurred_rounded_rect(&cut, adj_r, *std_dev, true);
                            self.ctx.pop_layer();
                        } else {
                            let expanded = rect
                                .translate(*offset_x, *offset_y)
                                .inflate(*spread, *spread)
                                .to_kurbo();
                            let adj_r = if avg_r > 0.0 || *spread > 0.0 {
                                (avg_r + *spread).max(0.0)
                            } else {
                                0.0
                            };
                            self.ctx
                                .fill_blurred_rounded_rect(&expanded, adj_r, *std_dev, false);
                        }
                    }
                    PaintCommand::BackdropBlur {
                        rect,
                        radius,
                        blur_radius,
                        tint,
                    } => {
                        if layer_stack_depth == 0 && clip_stack_depth == 0 {
                            self.flush_batch(&mut rendered_any);
                            let bounds = cur_xform.transform_rect_bounds(*rect);
                            let pad = (*blur_radius * scale * 2.0).ceil() as i32;
                            let sx0 =
                                ((bounds.x * scale).floor() as i32 - pad).clamp(0, w as i32 - 1);
                            let sy0 =
                                ((bounds.y * scale).floor() as i32 - pad).clamp(0, h as i32 - 1);
                            let sx1 = ((bounds.max_x() * scale).ceil() as i32 + pad)
                                .clamp(sx0 + 1, w as i32);
                            let sy1 = ((bounds.max_y() * scale).ceil() as i32 + pad)
                                .clamp(sy0 + 1, h as i32);
                            let bw = (sx1 - sx0) as usize;
                            let bh = (sy1 - sy0) as usize;
                            let mut patch = vec![0u8; bw * bh * 4];
                            let target_bytes = self.target.data_as_u8_slice();
                            let row_stride = w as usize * 4;
                            for row in 0..bh {
                                let src_off = (sy0 as usize + row) * row_stride + sx0 as usize * 4;
                                patch[row * bw * 4..(row + 1) * bw * 4]
                                    .copy_from_slice(&target_bytes[src_off..src_off + bw * 4]);
                            }
                            let br = (*blur_radius * scale * 0.5).round().max(1.0) as usize;
                            separable_box_blur_rgba(&mut patch, bw, bh, br);

                            let mut blur_pix = Pixmap::new(bw as u16, bh as u16);
                            blur_pix.data_as_u8_slice_mut().copy_from_slice(&patch);
                            blur_pix.recompute_may_have_transparency();

                            let clip_path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.set_transform(eff_xform);
                            self.ctx.push_clip_layer(&clip_path);
                            self.ctx.set_transform(Affine::IDENTITY);
                            self.ctx
                                .set_paint_transform(Affine::translate((sx0 as f64, sy0 as f64)));
                            self.ctx.set_paint(VelloCpuImage {
                                image: ImageSource::Pixmap(Arc::new(blur_pix)),
                                sampler: peniko::ImageSampler::default(),
                            });
                            self.ctx.fill_rect(&kurbo::Rect::new(
                                sx0 as f64, sy0 as f64, sx1 as f64, sy1 as f64,
                            ));
                            self.ctx.reset_paint_transform();
                            self.ctx.set_transform(eff_xform);
                            self.ctx.set_paint(tint.to_peniko());
                            self.ctx.fill_path(&clip_path);
                            self.ctx.pop_layer();
                        } else {
                            // Inside an active clip/layer: apply frosted tint fallback cleanly
                            self.ctx.set_transform(eff_xform);
                            self.ctx.reset_paint_transform();
                            self.ctx.set_paint(tint.to_peniko());
                            let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.fill_path(&path);
                        }
                    }
                    PaintCommand::FillPath { path, brush } => {
                        self.ctx.set_transform(eff_xform);
                        self.ctx.reset_paint_transform();
                        apply_paint(&mut self.ctx, brush);
                        self.ctx.fill_path(path);
                    }
                    PaintCommand::StrokePath { path, color, width } => {
                        self.ctx.set_transform(eff_xform);
                        self.ctx.reset_paint_transform();
                        self.ctx.set_paint(color.to_peniko());
                        self.ctx.set_stroke(Stroke::new(*width as f64));
                        self.ctx.stroke_path(path);
                    }
                    PaintCommand::DrawImage {
                        rect,
                        texture,
                        fit,
                        radius,
                        opacity,
                    } => {
                        if let Some(pixmap) = self.get_or_upload_pixmap(*texture, textures) {
                            let img_w = pixmap.width() as u32;
                            let img_h = pixmap.height() as u32;
                            let (_, img_affine) =
                                compute_image_transform(*rect, img_w, img_h, *fit);
                            let clip_path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.set_transform(eff_xform);
                            self.ctx.push_clip_layer(&clip_path);
                            self.ctx.set_paint_transform(img_affine);
                            let img_paint = VelloCpuImage {
                                image: ImageSource::Pixmap(pixmap),
                                sampler: peniko::ImageSampler::default().with_alpha(*opacity),
                            };
                            self.ctx.set_paint(img_paint);
                            self.ctx.fill_rect(&rect.to_kurbo());
                            self.ctx.reset_paint_transform();
                            self.ctx.pop_layer();
                        }
                    }
                    PaintCommand::DrawText { origin, .. }
                    | PaintCommand::DrawRichText { origin, .. } => {
                        let Some((layout, color_override)) = cmd.shaped_text(text_engine) else {
                            continue;
                        };
                        let text_xform =
                            eff_xform * Affine::translate((origin.x as f64, origin.y as f64));
                        self.ctx.set_transform(text_xform);
                        self.ctx.reset_paint_transform();
                        let mut coords_buf: Vec<i16> = Vec::new();
                        for line in layout.lines() {
                            for item in line.items() {
                                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                                    self.ctx.set_paint(
                                        color_override
                                            .unwrap_or(glyph_run.style().brush)
                                            .to_peniko(),
                                    );
                                    let run = glyph_run.run();
                                    let font = run.font();
                                    let font_size = run.font_size();
                                    let norm = run.normalized_coords();
                                    let coords_slice: &[i16] = if norm.is_empty() {
                                        &[]
                                    } else {
                                        coords_buf.clear();
                                        coords_buf.extend(norm.iter().map(|c| c.to_bits()));
                                        &coords_buf
                                    };
                                    let glyphs =
                                        glyph_run.positioned_glyphs().map(|g| vello_cpu::Glyph {
                                            id: g.id,
                                            x: g.x,
                                            y: g.y,
                                        });
                                    let _ = self
                                        .ctx
                                        .glyph_run(&mut self.resources, font)
                                        .font_size(font_size)
                                        .hint(true)
                                        .normalized_coords(coords_slice)
                                        .fill_glyphs(glyphs);
                                    if let Some((underline, color)) =
                                        crate::scene::underline_rect(&glyph_run, color_override)
                                    {
                                        self.ctx.set_paint(color.to_peniko());
                                        self.ctx.fill_rect(&underline.to_kurbo());
                                    }
                                }
                            }
                        }
                        self.ctx.set_transform(eff_xform);
                    }
                    PaintCommand::PushClip { rect, radius } => {
                        self.ctx.set_transform(eff_xform);
                        if radius.is_zero() && cur_xform.is_identity() {
                            self.ctx.push_clip_rect(&rect.to_kurbo());
                            clip_stack_depth += 1;
                        } else {
                            let path = rect.to_rounded_kurbo(*radius).to_path(0.1);
                            self.ctx.push_clip_layer(&path);
                            layer_stack_depth += 1;
                        }
                    }
                    PaintCommand::PopClip => {
                        if clip_stack_depth > 0 {
                            self.ctx.pop_clip();
                            clip_stack_depth -= 1;
                        } else if layer_stack_depth > 0 {
                            self.ctx.pop_layer();
                            layer_stack_depth -= 1;
                        }
                    }
                    PaintCommand::PushLayer {
                        rect,
                        opacity,
                        blend_mode,
                    } => {
                        self.ctx.set_transform(eff_xform);
                        let p_blend = to_peniko_blend(*blend_mode);
                        let pad = 64.0;
                        let padded = rect.inflate(pad, pad).to_kurbo().to_path(0.1);
                        let clip = if rect.width > 0.0 && rect.height > 0.0 {
                            Some(&padded)
                        } else {
                            None
                        };
                        self.ctx
                            .push_layer(clip, Some(p_blend), Some(*opacity), None, None);
                        layer_stack_depth += 1;
                    }
                    PaintCommand::PopLayer => {
                        if layer_stack_depth > 0 {
                            self.ctx.pop_layer();
                            layer_stack_depth -= 1;
                        }
                    }
                    PaintCommand::PushTransform { transform } => {
                        xform_stack.push(cur_xform * *transform);
                    }
                    PaintCommand::PopTransform => {
                        if xform_stack.len() > 1 {
                            xform_stack.pop();
                        }
                    }
                    PaintCommand::PlatformViewHole { rect, .. } => {
                        let r = rect.to_kurbo();
                        let clip_path = r.to_path(0.1);
                        self.ctx.set_transform(eff_xform);
                        self.ctx.push_layer(
                            Some(&clip_path),
                            Some(vello_cpu::peniko::BlendMode::from(
                                vello_cpu::peniko::Compose::Clear,
                            )),
                            None,
                            None,
                            None,
                        );
                        self.ctx.set_paint(Color::BLACK.to_peniko());
                        self.ctx.fill_rect(&r);
                        self.ctx.pop_layer();
                    }
                }
            }

            while clip_stack_depth > 0 {
                self.ctx.pop_clip();
                clip_stack_depth -= 1;
            }
            while layer_stack_depth > 0 {
                self.ctx.pop_layer();
                layer_stack_depth -= 1;
            }

            self.flush_batch(&mut rendered_any);
            self.target.data_as_u8_slice()
        }

        /// Render scene and unpremultiply into straight RGBA8 bytes.
        #[allow(clippy::too_many_arguments)]
        pub fn render_straight_rgba(
            &mut self,
            scene: &PaintScene,
            width: u32,
            height: u32,
            scale_factor: f32,
            background: Color,
            textures: &TextureRegistry,
            text_engine: &mut TextEngine,
        ) -> Vec<u8> {
            let premul = self.render_premul(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            );
            unpremultiply_rgba(premul)
        }
    }

    fn to_peniko_blend(mode: BlendMode) -> vello_cpu::peniko::BlendMode {
        use vello_cpu::peniko::{BlendMode as PBlend, Compose, Mix};
        match mode {
            BlendMode::Normal => PBlend::default(),
            BlendMode::Multiply => Mix::Multiply.into(),
            BlendMode::Screen => Mix::Screen.into(),
            BlendMode::Overlay => Mix::Overlay.into(),
            BlendMode::Darken => Mix::Darken.into(),
            BlendMode::Lighten => Mix::Lighten.into(),
            BlendMode::Plus => Compose::Plus.into(),
        }
    }

    fn apply_paint(ctx: &mut RenderContext, brush: &Brush) {
        match brush_to_peniko(brush) {
            peniko::Brush::Solid(c) => ctx.set_paint(PaintType::Solid(c)),
            peniko::Brush::Gradient(g) => ctx.set_paint(PaintType::Gradient(g)),
            peniko::Brush::Image(_) => {}
        }
    }
}

#[cfg(feature = "cpu-vello")]
pub use vello_cpu_impl::VelloCpuBackend;

#[cfg(feature = "cpu-tiny-skia")]
mod tiny_skia_impl {
    use super::*;
    use crate::glyph_raster::rasterize_glyph;
    use kurbo::{BezPath, Shape};
    use parley::PositionedLayoutItem;

    struct TinyLayer {
        pixmap: tiny_skia::Pixmap,
        opacity: f32,
        blend_mode: tiny_skia::BlendMode,
    }

    /// Complete `tiny-skia` software vector renderer supporting shapes, multi-stop gradients,
    /// `skrifa` text, rounded/nested clips, opacity/blend layers, transforms, shadows, and blur.
    pub struct TinySkiaBackend;

    impl TinySkiaBackend {
        pub fn render_straight_rgba(
            scene: &PaintScene,
            width: u32,
            height: u32,
            scale_factor: f32,
            background: Color,
            textures: &TextureRegistry,
            text_engine: &mut TextEngine,
        ) -> Vec<u8> {
            let w = width.max(1);
            let h = height.max(1);
            let sf = scale_factor.max(1.0);
            let base_scale = tiny_skia::Transform::from_scale(sf, sf);

            let mut root_pixmap = tiny_skia::Pixmap::new(w, h).expect("tiny-skia pixmap");
            root_pixmap.fill(tiny_skia::Color::from_rgba8(
                background.r,
                background.g,
                background.b,
                background.a,
            ));

            let mut layer_stack: Vec<TinyLayer> = Vec::new();
            let mut clip_stack: Vec<tiny_skia::Mask> = Vec::new();
            let mut xform_stack: Vec<Transform2D> = vec![Transform2D::IDENTITY];

            for cmd in &scene.commands {
                let cur_xform = *xform_stack.last().unwrap();
                let ts_local = tiny_skia::Transform::from_row(
                    cur_xform.matrix[0],
                    cur_xform.matrix[1],
                    cur_xform.matrix[2],
                    cur_xform.matrix[3],
                    cur_xform.matrix[4],
                    cur_xform.matrix[5],
                );
                let transform = base_scale.pre_concat(ts_local);

                match cmd {
                    PaintCommand::FillRect {
                        rect,
                        brush,
                        radius,
                    } => {
                        if let Some(path) = rounded_rect_to_tiny_path(*rect, *radius) {
                            let paint = brush_to_tiny_paint(brush);
                            let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                            dst.fill_path(
                                &path,
                                &paint,
                                tiny_skia::FillRule::Winding,
                                transform,
                                clip_stack.last(),
                            );
                        }
                    }
                    PaintCommand::StrokeRect {
                        rect,
                        color,
                        width: stroke_w,
                        radius,
                    } => {
                        if let Some(path) = rounded_rect_to_tiny_path(*rect, *radius) {
                            let mut paint = tiny_skia::Paint::default();
                            paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                            paint.anti_alias = true;
                            let stroke = tiny_skia::Stroke {
                                width: *stroke_w,
                                ..Default::default()
                            };
                            let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                            dst.stroke_path(&path, &paint, &stroke, transform, clip_stack.last());
                        }
                    }
                    PaintCommand::DropShadow {
                        rect,
                        radius,
                        std_dev,
                        spread,
                        inset,
                        offset_x,
                        offset_y,
                        color,
                    } => {
                        let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                        draw_tiny_shadow(
                            dst,
                            transform,
                            sf,
                            cur_xform,
                            clip_stack.last(),
                            *rect,
                            *radius,
                            *std_dev,
                            *spread,
                            *inset,
                            *offset_x,
                            *offset_y,
                            *color,
                        );
                    }
                    PaintCommand::BackdropBlur {
                        rect,
                        radius,
                        blur_radius,
                        tint,
                    } => {
                        let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                        let bounds = cur_xform.transform_rect_bounds(*rect);
                        let pad = (*blur_radius * sf * 2.0).ceil() as i32;
                        let sx0 = ((bounds.x * sf).floor() as i32 - pad).clamp(0, w as i32 - 1);
                        let sy0 = ((bounds.y * sf).floor() as i32 - pad).clamp(0, h as i32 - 1);
                        let sx1 =
                            ((bounds.max_x() * sf).ceil() as i32 + pad).clamp(sx0 + 1, w as i32);
                        let sy1 =
                            ((bounds.max_y() * sf).ceil() as i32 + pad).clamp(sy0 + 1, h as i32);
                        let bw = (sx1 - sx0) as usize;
                        let bh = (sy1 - sy0) as usize;
                        let mut patch = vec![0u8; bw * bh * 4];
                        let row_stride = w as usize * 4;
                        let src_bytes = dst.data();
                        for row in 0..bh {
                            let src_off = (sy0 as usize + row) * row_stride + sx0 as usize * 4;
                            patch[row * bw * 4..(row + 1) * bw * 4]
                                .copy_from_slice(&src_bytes[src_off..src_off + bw * 4]);
                        }
                        let br = (*blur_radius * sf * 0.5).round().max(1.0) as usize;
                        separable_box_blur_rgba(&mut patch, bw, bh, br);

                        if let Some(shape_path) = rounded_rect_to_tiny_path(*rect, *radius)
                            && let Some(mut mask) = tiny_skia::Mask::new(w, h)
                        {
                            mask.fill_path(
                                &shape_path,
                                tiny_skia::FillRule::Winding,
                                true,
                                transform,
                            );
                            if let Some(parent_clip) = clip_stack.last() {
                                for (a, b) in
                                    mask.data_mut().iter_mut().zip(parent_clip.data().iter())
                                {
                                    *a = ((*a as u16 * *b as u16) / 255) as u8;
                                }
                            }
                            if let Some(sz) = tiny_skia::IntSize::from_wh(bw as u32, bh as u32)
                                && let Some(blur_pix) = tiny_skia::Pixmap::from_vec(patch, sz)
                            {
                                let pp = tiny_skia::PixmapPaint {
                                    blend_mode: tiny_skia::BlendMode::SourceOver,
                                    ..Default::default()
                                };
                                dst.draw_pixmap(
                                    sx0,
                                    sy0,
                                    blur_pix.as_ref(),
                                    &pp,
                                    tiny_skia::Transform::identity(),
                                    Some(&mask),
                                );
                            }
                            let mut tp = tiny_skia::Paint::default();
                            tp.set_color_rgba8(tint.r, tint.g, tint.b, tint.a);
                            tp.anti_alias = true;
                            dst.fill_path(
                                &shape_path,
                                &tp,
                                tiny_skia::FillRule::Winding,
                                transform,
                                clip_stack.last(),
                            );
                        }
                    }
                    PaintCommand::FillPath { path, brush } => {
                        if let Some(ts_path) = kurbo_to_tiny_path(path) {
                            let paint = brush_to_tiny_paint(brush);
                            let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                            dst.fill_path(
                                &ts_path,
                                &paint,
                                tiny_skia::FillRule::Winding,
                                transform,
                                clip_stack.last(),
                            );
                        }
                    }
                    PaintCommand::StrokePath {
                        path,
                        color,
                        width: stroke_w,
                    } => {
                        if let Some(ts_path) = kurbo_to_tiny_path(path) {
                            let mut paint = tiny_skia::Paint::default();
                            paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                            paint.anti_alias = true;
                            let stroke = tiny_skia::Stroke {
                                width: *stroke_w,
                                ..Default::default()
                            };
                            let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                            dst.stroke_path(
                                &ts_path,
                                &paint,
                                &stroke,
                                transform,
                                clip_stack.last(),
                            );
                        }
                    }
                    PaintCommand::DrawImage {
                        rect,
                        texture,
                        fit,
                        radius,
                        opacity,
                    } => {
                        if let Some(handle) = textures.get(*texture) {
                            let (iw, ih, _, rgba) = handle.snapshot();
                            if iw > 0 && ih > 0 {
                                let mut premul = rgba;
                                for px in premul.as_chunks_mut::<4>().0 {
                                    let a = px[3] as u16;
                                    px[0] = ((px[0] as u16 * a) / 255) as u8;
                                    px[1] = ((px[1] as u16 * a) / 255) as u8;
                                    px[2] = ((px[2] as u16 * a) / 255) as u8;
                                }
                                if let Some(size) = tiny_skia::IntSize::from_wh(iw, ih)
                                    && let Some(src) = tiny_skia::Pixmap::from_vec(premul, size)
                                {
                                    let (_, aff) = compute_image_transform(*rect, iw, ih, *fit);
                                    let c = aff.as_coeffs();
                                    let img_local = tiny_skia::Transform::from_row(
                                        c[0] as f32,
                                        c[1] as f32,
                                        c[2] as f32,
                                        c[3] as f32,
                                        c[4] as f32,
                                        c[5] as f32,
                                    );
                                    let img_xform = transform.pre_concat(img_local);
                                    let pp = tiny_skia::PixmapPaint {
                                        opacity: *opacity,
                                        quality: tiny_skia::FilterQuality::Bilinear,
                                        ..Default::default()
                                    };
                                    let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                                    let img_mask = if !radius.is_zero()
                                        || cur_xform != Transform2D::IDENTITY
                                    {
                                        rounded_rect_to_tiny_path(*rect, *radius).and_then(|p| {
                                            let mut m = tiny_skia::Mask::new(w, h)?;
                                            m.fill_path(
                                                &p,
                                                tiny_skia::FillRule::Winding,
                                                true,
                                                transform,
                                            );
                                            if let Some(outer) = clip_stack.last() {
                                                for (a, b) in
                                                    m.data_mut().iter_mut().zip(outer.data().iter())
                                                {
                                                    *a = ((*a as u16 * *b as u16) / 255) as u8;
                                                }
                                            }
                                            Some(m)
                                        })
                                    } else {
                                        None
                                    };
                                    let mask_ref = img_mask.as_ref().or_else(|| clip_stack.last());
                                    dst.draw_pixmap(0, 0, src.as_ref(), &pp, img_xform, mask_ref);
                                }
                            }
                        }
                    }
                    PaintCommand::DrawText { origin, .. }
                    | PaintCommand::DrawRichText { origin, .. } => {
                        let Some((layout, color_override)) = cmd.shaped_text(text_engine) else {
                            continue;
                        };
                        let mut coords_buf: Vec<i16> = Vec::new();
                        let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                        for line in layout.lines() {
                            for item in line.items() {
                                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                                    let col = color_override.unwrap_or(glyph_run.style().brush);
                                    let run = glyph_run.run();
                                    let font = run.font();
                                    let font_size_phys = run.font_size() * sf;
                                    let norm = run.normalized_coords();
                                    coords_buf.clear();
                                    coords_buf.extend(norm.iter().map(|c| c.to_bits()));
                                    for g in glyph_run.positioned_glyphs() {
                                        let p_local =
                                            Point_xf(cur_xform, origin.x + g.x, origin.y + g.y);
                                        let gx_phys = p_local.0 * sf;
                                        let gy_phys = p_local.1 * sf;
                                        let gx_floor = gx_phys.floor();
                                        let gy_round = gy_phys.round();
                                        let sub_x = (gx_phys - gx_floor).clamp(0.0, 0.99);
                                        if let Some(mask) = rasterize_glyph(
                                            font.data.as_ref(),
                                            font.index,
                                            g.id,
                                            font_size_phys,
                                            sub_x,
                                            0.0,
                                            &coords_buf,
                                        ) && let Some(sz) =
                                            tiny_skia::IntSize::from_wh(mask.width, mask.height)
                                        {
                                            let mut premul =
                                                vec![0u8; (mask.width * mask.height * 4) as usize];
                                            for (i, &cov) in mask.coverage.iter().enumerate() {
                                                if cov == 0 {
                                                    continue;
                                                }
                                                let a = ((col.a as u16 * cov as u16) / 255) as u8;
                                                let idx = i * 4;
                                                premul[idx] =
                                                    ((col.r as u16 * a as u16) / 255) as u8;
                                                premul[idx + 1] =
                                                    ((col.g as u16 * a as u16) / 255) as u8;
                                                premul[idx + 2] =
                                                    ((col.b as u16 * a as u16) / 255) as u8;
                                                premul[idx + 3] = a;
                                            }
                                            if let Some(glyph_pix) =
                                                tiny_skia::Pixmap::from_vec(premul, sz)
                                            {
                                                dst.draw_pixmap(
                                                    gx_floor as i32 + mask.offset_x,
                                                    gy_round as i32 + mask.offset_y,
                                                    glyph_pix.as_ref(),
                                                    &tiny_skia::PixmapPaint::default(),
                                                    tiny_skia::Transform::identity(),
                                                    clip_stack.last(),
                                                );
                                            }
                                        }
                                    }
                                    if let Some((underline, color)) =
                                        crate::scene::underline_rect(&glyph_run, color_override)
                                        && let Some(rect) = tiny_skia::Rect::from_xywh(
                                            origin.x + underline.x,
                                            origin.y + underline.y,
                                            underline.width,
                                            underline.height,
                                        )
                                    {
                                        let mut paint = tiny_skia::Paint::default();
                                        paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                                        dst.fill_rect(rect, &paint, transform, clip_stack.last());
                                    }
                                }
                            }
                        }
                    }
                    PaintCommand::PushClip { rect, radius } => {
                        if let Some(path) = rounded_rect_to_tiny_path(*rect, *radius) {
                            let mut mask = if let Some(prev) = clip_stack.last() {
                                let mut cloned = prev.clone();
                                cloned.intersect_path(
                                    &path,
                                    tiny_skia::FillRule::Winding,
                                    true,
                                    transform,
                                );
                                cloned
                            } else {
                                let mut m = tiny_skia::Mask::new(w, h).expect("tiny-skia mask");
                                m.fill_path(&path, tiny_skia::FillRule::Winding, true, transform);
                                m
                            };
                            let _ = &mut mask;
                            clip_stack.push(mask);
                        }
                    }
                    PaintCommand::PopClip => {
                        clip_stack.pop();
                    }
                    PaintCommand::PushLayer {
                        opacity,
                        blend_mode,
                        ..
                    } => {
                        let pixmap = tiny_skia::Pixmap::new(w, h).expect("tiny-skia layer");
                        layer_stack.push(TinyLayer {
                            pixmap,
                            opacity: *opacity,
                            blend_mode: to_tiny_blend(*blend_mode),
                        });
                    }
                    PaintCommand::PopLayer => {
                        if let Some(layer) = layer_stack.pop() {
                            let parent = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                            let paint = tiny_skia::PixmapPaint {
                                opacity: layer.opacity,
                                blend_mode: layer.blend_mode,
                                quality: tiny_skia::FilterQuality::Nearest,
                            };
                            parent.draw_pixmap(
                                0,
                                0,
                                layer.pixmap.as_ref(),
                                &paint,
                                tiny_skia::Transform::identity(),
                                clip_stack.last(),
                            );
                        }
                    }
                    PaintCommand::PushTransform { transform } => {
                        xform_stack.push(cur_xform * *transform);
                    }
                    PaintCommand::PopTransform => {
                        if xform_stack.len() > 1 {
                            xform_stack.pop();
                        }
                    }
                    PaintCommand::PlatformViewHole { rect, .. } => {
                        if let Some(path) = rounded_rect_to_tiny_path(*rect, CornerRadii::ZERO) {
                            let mut paint = tiny_skia::Paint::default();
                            paint.set_color_rgba8(0, 0, 0, 255);
                            paint.blend_mode = tiny_skia::BlendMode::Clear;
                            let dst = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                            dst.fill_path(
                                &path,
                                &paint,
                                tiny_skia::FillRule::Winding,
                                transform,
                                clip_stack.last(),
                            );
                        }
                    }
                }
            }

            while let Some(layer) = layer_stack.pop() {
                let parent = active_pixmap_mut(&mut root_pixmap, &mut layer_stack);
                let paint = tiny_skia::PixmapPaint {
                    opacity: layer.opacity,
                    blend_mode: layer.blend_mode,
                    quality: tiny_skia::FilterQuality::Nearest,
                };
                parent.draw_pixmap(
                    0,
                    0,
                    layer.pixmap.as_ref(),
                    &paint,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }

            unpremultiply_rgba(&root_pixmap.take())
        }
    }

    #[allow(non_snake_case)]
    #[inline]
    fn Point_xf(xf: Transform2D, x: f32, y: f32) -> (f32, f32) {
        let p = xf.transform_point(craftui_core::Point::new(x, y));
        (p.x, p.y)
    }

    fn active_pixmap_mut<'a>(
        root: &'a mut tiny_skia::Pixmap,
        stack: &'a mut [TinyLayer],
    ) -> &'a mut tiny_skia::Pixmap {
        stack.last_mut().map_or(root, |l| &mut l.pixmap)
    }

    fn to_tiny_blend(mode: BlendMode) -> tiny_skia::BlendMode {
        match mode {
            BlendMode::Normal => tiny_skia::BlendMode::SourceOver,
            BlendMode::Multiply => tiny_skia::BlendMode::Multiply,
            BlendMode::Screen => tiny_skia::BlendMode::Screen,
            BlendMode::Overlay => tiny_skia::BlendMode::Overlay,
            BlendMode::Darken => tiny_skia::BlendMode::Darken,
            BlendMode::Lighten => tiny_skia::BlendMode::Lighten,
            BlendMode::Plus => tiny_skia::BlendMode::Plus,
        }
    }

    fn brush_to_tiny_paint(brush: &Brush) -> tiny_skia::Paint<'static> {
        let mut paint = tiny_skia::Paint {
            anti_alias: true,
            ..Default::default()
        };
        match brush {
            Brush::Solid(c) => paint.set_color_rgba8(c.r, c.g, c.b, c.a),
            Brush::LinearGradient { start, end, stops } => {
                let t_stops = to_tiny_stops(stops);
                if let Some(shader) = tiny_skia::LinearGradient::new(
                    tiny_skia::Point::from_xy(start.x, start.y),
                    tiny_skia::Point::from_xy(end.x, end.y),
                    t_stops,
                    tiny_skia::SpreadMode::Pad,
                    tiny_skia::Transform::identity(),
                ) {
                    paint.shader = shader;
                }
            }
            Brush::RadialGradient {
                center,
                radius,
                stops,
            } => {
                let t_stops = to_tiny_stops(stops);
                let pt = tiny_skia::Point::from_xy(center.x, center.y);
                if let Some(shader) = tiny_skia::RadialGradient::new(
                    pt,
                    0.0,
                    pt,
                    (*radius).max(0.01),
                    t_stops,
                    tiny_skia::SpreadMode::Pad,
                    tiny_skia::Transform::identity(),
                ) {
                    paint.shader = shader;
                }
            }
        }
        paint
    }

    fn to_tiny_stops(stops: &[ColorStop]) -> Vec<tiny_skia::GradientStop> {
        if stops.is_empty() {
            return vec![
                tiny_skia::GradientStop::new(0.0, tiny_skia::Color::TRANSPARENT),
                tiny_skia::GradientStop::new(1.0, tiny_skia::Color::TRANSPARENT),
            ];
        }
        let mut sorted = stops.to_vec();
        sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        sorted
            .into_iter()
            .map(|s| {
                tiny_skia::GradientStop::new(
                    s.offset.clamp(0.0, 1.0),
                    tiny_skia::Color::from_rgba8(s.color.r, s.color.g, s.color.b, s.color.a),
                )
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tiny_shadow(
        dst: &mut tiny_skia::Pixmap,
        transform: tiny_skia::Transform,
        sf: f32,
        cur_xform: Transform2D,
        clip: Option<&tiny_skia::Mask>,
        rect: Rect,
        radius: CornerRadii,
        std_dev: f32,
        spread: f32,
        inset: bool,
        offset_x: f32,
        offset_y: f32,
        color: Color,
    ) {
        let w = dst.width();
        let h = dst.height();
        let pad_px = (std_dev * sf * 2.5).ceil() as i32 + 2;
        if inset {
            let bounds = cur_xform.transform_rect_bounds(rect);
            let x0 = ((bounds.x * sf).floor() as i32 - 2).clamp(0, w as i32 - 1);
            let y0 = ((bounds.y * sf).floor() as i32 - 2).clamp(0, h as i32 - 1);
            let x1 = ((bounds.max_x() * sf).ceil() as i32 + 2).clamp(x0 + 1, w as i32);
            let y1 = ((bounds.max_y() * sf).ceil() as i32 + 2).clamp(y0 + 1, h as i32);
            let sw = (x1 - x0) as u32;
            let sh = (y1 - y0) as u32;
            if let Some(mut shadow_pix) = tiny_skia::Pixmap::new(sw, sh) {
                shadow_pix.fill(tiny_skia::Color::from_rgba8(
                    color.r, color.g, color.b, color.a,
                ));
                let cut_rect = rect.translate(offset_x, offset_y).inflate(-spread, -spread);
                let cut_radii = CornerRadii::new(
                    (radius.top_left - spread).max(0.0),
                    (radius.top_right - spread).max(0.0),
                    (radius.bottom_right - spread).max(0.0),
                    (radius.bottom_left - spread).max(0.0),
                );
                if let Some(cut_path) = rounded_rect_to_tiny_path(cut_rect, cut_radii) {
                    let mut clr = tiny_skia::Paint::default();
                    clr.set_color_rgba8(0, 0, 0, 255);
                    clr.blend_mode = tiny_skia::BlendMode::Clear;
                    clr.anti_alias = true;
                    let local_ts = tiny_skia::Transform::from_translate(-x0 as f32, -y0 as f32)
                        .pre_concat(transform);
                    shadow_pix.fill_path(
                        &cut_path,
                        &clr,
                        tiny_skia::FillRule::Winding,
                        local_ts,
                        None,
                    );
                }
                let br = (std_dev * sf * 0.5).round() as usize;
                if br > 0 {
                    separable_box_blur_rgba(shadow_pix.data_mut(), sw as usize, sh as usize, br);
                }
                if let Some(outer_path) = rounded_rect_to_tiny_path(rect, radius)
                    && let Some(mut mask) = tiny_skia::Mask::new(w, h)
                {
                    mask.fill_path(&outer_path, tiny_skia::FillRule::Winding, true, transform);
                    if let Some(parent_clip) = clip {
                        for (a, b) in mask.data_mut().iter_mut().zip(parent_clip.data().iter()) {
                            *a = ((*a as u16 * *b as u16) / 255) as u8;
                        }
                    }
                    dst.draw_pixmap(
                        x0,
                        y0,
                        shadow_pix.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        Some(&mask),
                    );
                }
            }
        } else {
            let srect = rect.translate(offset_x, offset_y).inflate(spread, spread);
            let adj = |r: f32| {
                if r > 0.0 || spread > 0.0 {
                    (r + spread).max(0.0)
                } else {
                    0.0
                }
            };
            let sradii = CornerRadii::new(
                adj(radius.top_left),
                adj(radius.top_right),
                adj(radius.bottom_right),
                adj(radius.bottom_left),
            );
            let bounds = cur_xform.transform_rect_bounds(srect);
            let x0 = ((bounds.x * sf).floor() as i32 - pad_px).clamp(0, w as i32 - 1);
            let y0 = ((bounds.y * sf).floor() as i32 - pad_px).clamp(0, h as i32 - 1);
            let x1 = ((bounds.max_x() * sf).ceil() as i32 + pad_px).clamp(x0 + 1, w as i32);
            let y1 = ((bounds.max_y() * sf).ceil() as i32 + pad_px).clamp(y0 + 1, h as i32);
            let sw = (x1 - x0) as u32;
            let sh = (y1 - y0) as u32;
            if let Some(path) = rounded_rect_to_tiny_path(srect, sradii)
                && let Some(mut shadow_pix) = tiny_skia::Pixmap::new(sw, sh)
            {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                paint.anti_alias = true;
                let local_ts = tiny_skia::Transform::from_translate(-x0 as f32, -y0 as f32)
                    .pre_concat(transform);
                shadow_pix.fill_path(&path, &paint, tiny_skia::FillRule::Winding, local_ts, None);
                let br = (std_dev * sf * 0.5).round() as usize;
                if br > 0 {
                    separable_box_blur_rgba(shadow_pix.data_mut(), sw as usize, sh as usize, br);
                }
                dst.draw_pixmap(
                    x0,
                    y0,
                    shadow_pix.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    clip,
                );
            }
        }
    }

    fn rounded_rect_to_tiny_path(rect: Rect, radius: CornerRadii) -> Option<tiny_skia::Path> {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        if radius.is_zero() {
            let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height)?;
            return Some(tiny_skia::PathBuilder::from_rect(r));
        }
        let bez = rect.to_rounded_kurbo(radius).to_path(0.25);
        kurbo_to_tiny_path(&bez)
    }

    fn kurbo_to_tiny_path(path: &BezPath) -> Option<tiny_skia::Path> {
        let mut pb = tiny_skia::PathBuilder::new();
        for el in path.elements() {
            match *el {
                kurbo::PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
                kurbo::PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
                kurbo::PathEl::QuadTo(p1, p2) => {
                    pb.quad_to(p1.x as f32, p1.y as f32, p2.x as f32, p2.y as f32)
                }
                kurbo::PathEl::CurveTo(p1, p2, p3) => pb.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p3.x as f32,
                    p3.y as f32,
                ),
                kurbo::PathEl::ClosePath => pb.close(),
            }
        }
        pb.finish()
    }
}

#[cfg(feature = "cpu-tiny-skia")]
pub use tiny_skia_impl::TinySkiaBackend;

/// Lightweight feature-adaptive software renderer wrapper that dispatches cleanly to
/// `VelloCpuBackend` (`cpu-vello`), `TinySkiaBackend` (`cpu-tiny-skia`), or a zero-dep fallback.
#[derive(Default)]
pub struct SoftwareRenderer {
    #[cfg(feature = "cpu-vello")]
    vello: VelloCpuBackend,
}

impl SoftwareRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        #[cfg(feature = "cpu-vello")]
        {
            Self {
                vello: VelloCpuBackend::new(width, height),
            }
        }
        #[cfg(not(feature = "cpu-vello"))]
        {
            let _ = (width, height);
            Self {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_straight_rgba(
        &mut self,
        scene: &PaintScene,
        width: u32,
        height: u32,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Vec<u8> {
        #[cfg(feature = "cpu-vello")]
        {
            self.vello.render_straight_rgba(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            )
        }
        #[cfg(all(not(feature = "cpu-vello"), feature = "cpu-tiny-skia"))]
        {
            TinySkiaBackend::render_straight_rgba(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            )
        }
        #[cfg(not(any(feature = "cpu-vello", feature = "cpu-tiny-skia")))]
        {
            render_minimal_software_rgba(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            )
        }
    }
}

#[cfg(any(feature = "cpu-vello", feature = "cpu-tiny-skia"))]
fn unpremultiply_rgba(premul: &[u8]) -> Vec<u8> {
    let mut out = premul.to_vec();
    for px in out.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a > 0 && a < 255 {
            let af = 255.0 / a as f32;
            px[0] = ((px[0] as f32 * af).round() as u16).min(255) as u8;
            px[1] = ((px[1] as f32 * af).round() as u16).min(255) as u8;
            px[2] = ((px[2] as f32 * af).round() as u16).min(255) as u8;
        }
    }
    out
}

/// Zero-bloat fallback software rasterizer used only when neither `cpu-vello` nor `cpu-tiny-skia`
/// is enabled AND GPU initialization also failed.
#[cfg(not(any(feature = "cpu-vello", feature = "cpu-tiny-skia")))]
pub(crate) fn render_minimal_software_rgba(
    scene: &PaintScene,
    width: u32,
    height: u32,
    scale_factor: f32,
    background: Color,
    textures: &TextureRegistry,
    text_engine: &mut TextEngine,
) -> Vec<u8> {
    let w = width.max(1) as usize;
    let h = height.max(1) as usize;
    let sf = scale_factor.max(1.0);
    let mut out = vec![0u8; w * h * 4];
    for px in out.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&[background.r, background.g, background.b, background.a]);
    }
    let _ = text_engine;
    let mut xform_stack = vec![Transform2D::IDENTITY];
    for cmd in &scene.commands {
        let xf = *xform_stack.last().unwrap();
        match cmd {
            PaintCommand::FillRect { rect, brush, .. } => {
                let c = match brush {
                    Brush::Solid(c) => *c,
                    Brush::LinearGradient { stops, .. } | Brush::RadialGradient { stops, .. } => {
                        stops.first().map(|s| s.color).unwrap_or(Color::WHITE)
                    }
                };
                let b = xf.transform_rect_bounds(*rect);
                fill_span_rgba(&mut out, w, h, b, sf, c);
            }
            PaintCommand::StrokeRect {
                rect, color, width, ..
            } => {
                let b = xf.transform_rect_bounds(*rect);
                let sw = (*width).max(1.0);
                fill_span_rgba(&mut out, w, h, Rect::new(b.x, b.y, b.width, sw), sf, *color);
                fill_span_rgba(
                    &mut out,
                    w,
                    h,
                    Rect::new(b.x, (b.max_y() - sw).max(b.y), b.width, sw),
                    sf,
                    *color,
                );
            }
            PaintCommand::DrawImage {
                rect,
                texture,
                fit,
                opacity,
                ..
            } => {
                if let Some(handle) = textures.get(*texture) {
                    let (iw, ih, _, src) = handle.snapshot();
                    if iw > 0 && ih > 0 {
                        let (dr, _) =
                            compute_image_transform(xf.transform_rect_bounds(*rect), iw, ih, *fit);
                        let dx0 = (dr.x * sf).floor().max(0.0) as usize;
                        let dy0 = (dr.y * sf).floor().max(0.0) as usize;
                        let dx1 = (dr.max_x() * sf).ceil().min(w as f32) as usize;
                        let dy1 = (dr.max_y() * sf).ceil().min(h as f32) as usize;
                        for py in dy0..dy1 {
                            let v = (((py as f32 - dr.y * sf) / (dr.height * sf).max(1.0))
                                * ih as f32)
                                .clamp(0.0, (ih - 1) as f32)
                                as usize;
                            for px in dx0..dx1 {
                                let u = (((px as f32 - dr.x * sf) / (dr.width * sf).max(1.0))
                                    * iw as f32)
                                    .clamp(0.0, (iw - 1) as f32)
                                    as usize;
                                let si = (v * iw as usize + u) * 4;
                                let di = (py * w + px) * 4;
                                let a = (src[si + 3] as f32 * *opacity / 255.0).clamp(0.0, 1.0);
                                blend_straight(
                                    &mut out[di..di + 4],
                                    [src[si], src[si + 1], src[si + 2], (a * 255.0) as u8],
                                );
                            }
                        }
                    }
                }
            }
            PaintCommand::PlatformViewHole { rect, .. } => {
                let b = xf.transform_rect_bounds(*rect);
                let x0 = (b.x * sf).floor().clamp(0.0, w as f32) as usize;
                let y0 = (b.y * sf).floor().clamp(0.0, h as f32) as usize;
                let x1 = (b.max_x() * sf).ceil().clamp(0.0, w as f32) as usize;
                let y1 = (b.max_y() * sf).ceil().clamp(0.0, h as f32) as usize;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = (y * w + x) * 4;
                        out[i..i + 4].fill(0);
                    }
                }
            }
            PaintCommand::PushTransform { transform } => xform_stack.push(xf * *transform),
            PaintCommand::PopTransform if xform_stack.len() > 1 => {
                xform_stack.pop();
            }
            _ => {}
        }
    }
    out
}

#[cfg(not(any(feature = "cpu-vello", feature = "cpu-tiny-skia")))]
fn fill_span_rgba(out: &mut [u8], w: usize, h: usize, rect: Rect, sf: f32, c: Color) {
    let x0 = (rect.x * sf).floor().clamp(0.0, w as f32) as usize;
    let y0 = (rect.y * sf).floor().clamp(0.0, h as f32) as usize;
    let x1 = (rect.max_x() * sf).ceil().clamp(0.0, w as f32) as usize;
    let y1 = (rect.max_y() * sf).ceil().clamp(0.0, h as f32) as usize;
    let src = [c.r, c.g, c.b, c.a];
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 4;
            blend_straight(&mut out[i..i + 4], src);
        }
    }
}

#[cfg(not(any(feature = "cpu-vello", feature = "cpu-tiny-skia")))]
fn blend_straight(dst: &mut [u8], src: [u8; 4]) {
    let sa = src[3] as f32 / 255.0;
    let inv = 1.0 - sa;
    dst[0] = (src[0] as f32 * sa + dst[0] as f32 * inv)
        .round()
        .min(255.0) as u8;
    dst[1] = (src[1] as f32 * sa + dst[1] as f32 * inv)
        .round()
        .min(255.0) as u8;
    dst[2] = (src[2] as f32 * sa + dst[2] as f32 * inv)
        .round()
        .min(255.0) as u8;
    dst[3] = ((sa + (dst[3] as f32 / 255.0) * inv) * 255.0)
        .round()
        .min(255.0) as u8;
}
