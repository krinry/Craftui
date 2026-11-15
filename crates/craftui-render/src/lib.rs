//! Hybrid GPU (`gpu-sdf` / `vello`) and CPU (`vello_cpu` / `tiny-skia`) 2D vector renderer.

pub mod cpu_backend;
#[cfg(any(feature = "gpu-sdf", feature = "cpu-vello", feature = "cpu-tiny-skia"))]
pub mod glyph_raster;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
pub mod gpu_backend;
#[cfg(feature = "gpu-sdf")]
pub mod gpu_sdf_backend;
pub mod scene;
pub mod svg;
pub mod texture;

pub use cpu_backend::SoftwareRenderer;
#[cfg(feature = "cpu-tiny-skia")]
pub use cpu_backend::TinySkiaBackend;
#[cfg(feature = "cpu-vello")]
pub use cpu_backend::VelloCpuBackend;
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
pub use gpu_backend::GpuSurfacePipeline;
#[cfg(feature = "gpu-sdf")]
pub use gpu_sdf_backend::GpuSdfBackend;
pub use kurbo;
pub use scene::{PaintCommand, PaintScene};
pub use svg::{ParsedSvgIcon, SvgShape, builtin_icon_svg, paint_svg_icon};
pub use texture::{PixelBufferHandle, TextureHandle, TextureRegistry};

use craftui_core::{Color, Result};
use craftui_text::TextEngine;
use std::path::Path;

/// Renderer backend selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendKind {
    /// Automatically pick the active compiled backend (`vello_cpu`, `tiny-skia`, or `gpu-sdf`).
    #[default]
    Auto,
    /// Prefer pure-WGSL instanced SDF shader (`gpu-sdf`) with deterministic software fallback if no adapter is available.
    GpuSdf,
    /// Force `vello_cpu` software rasterization (when compiled with `cpu-vello`).
    VelloCpu,
    /// Force `tiny-skia` software rasterization (when compiled with `cpu-tiny-skia`).
    TinySkia,
}

/// Convenience helper for headless rendering and snapshot testing without opening a window.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_to_rgba(
    backend: BackendKind,
    scene: &PaintScene,
    width: u32,
    height: u32,
    scale_factor: f32,
    background: Color,
    textures: &TextureRegistry,
    text_engine: &mut TextEngine,
) -> Vec<u8> {
    match backend {
        BackendKind::GpuSdf => {
            #[cfg(all(feature = "gpu-sdf", not(target_arch = "wasm32")))]
            if let Some(px) = gpu_backend::try_render_gpu_sdf_headless(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            ) {
                return px;
            }
            let mut sw = SoftwareRenderer::new(width, height);
            sw.render_straight_rgba(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            )
        }
        BackendKind::Auto => {
            let mut sw = SoftwareRenderer::new(width, height);
            sw.render_straight_rgba(
                scene,
                width,
                height,
                scale_factor,
                background,
                textures,
                text_engine,
            )
        }
        BackendKind::VelloCpu => {
            #[cfg(feature = "cpu-vello")]
            {
                let mut cpu = VelloCpuBackend::new(width, height);
                cpu.render_straight_rgba(
                    scene,
                    width,
                    height,
                    scale_factor,
                    background,
                    textures,
                    text_engine,
                )
            }
            #[cfg(not(feature = "cpu-vello"))]
            {
                let mut sw = SoftwareRenderer::new(width, height);
                sw.render_straight_rgba(
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
        BackendKind::TinySkia => {
            #[cfg(feature = "cpu-tiny-skia")]
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
            #[cfg(not(feature = "cpu-tiny-skia"))]
            {
                let mut sw = SoftwareRenderer::new(width, height);
                sw.render_straight_rgba(
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
}

/// Encode straight RGBA8 pixels into an in-memory PNG byte buffer.
pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut encoder = png::Encoder::new(&mut buf, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| craftui_core::CraftError::Render(e.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| craftui_core::CraftError::Render(e.to_string()))?;
    drop(writer);
    Ok(buf)
}

/// Write straight RGBA8 pixels to a PNG file on disk.
pub fn write_rgba_png(path: impl AsRef<Path>, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let bytes = encode_rgba_png(width, height, rgba)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_core::{
        BlendMode, BoxShadow, Brush, ColorStop, CornerRadii, Fit, Point, Rect, Transform2D,
    };
    use craftui_text::TextStyle;

    #[test]
    #[cfg(any(feature = "cpu-vello", feature = "cpu-tiny-skia"))]
    fn rich_text_renders_distinct_span_colors_on_cpu_backends() {
        use craftui_text::{RichText, TextSpan};

        let red = Color::rgb(240, 20, 20);
        let blue = Color::rgb(20, 20, 240);
        let rich = RichText::new()
            .with_span(TextSpan::new(
                "RED ",
                TextStyle::default().with_size(28.0).with_color(red),
            ))
            .with_span(TextSpan::new(
                "BLUE",
                TextStyle::default().with_size(28.0).with_color(blue),
            ));
        let mut scene = PaintScene::new();
        scene.draw_rich_text(Point::new(4.0, 4.0), rich, Some(220.0));
        let textures = TextureRegistry::new();
        let mut backends = Vec::new();
        #[cfg(feature = "cpu-vello")]
        backends.push(BackendKind::VelloCpu);
        #[cfg(feature = "cpu-tiny-skia")]
        backends.push(BackendKind::TinySkia);
        for backend in backends {
            let mut engine = TextEngine::new_without_system_fonts();
            let rgba = render_scene_to_rgba(
                backend,
                &scene,
                240,
                60,
                1.0,
                Color::WHITE,
                &textures,
                &mut engine,
            );
            let red_pixels = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] > p[2].saturating_add(30))
                .count();
            let blue_pixels = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[2] > p[0].saturating_add(30))
                .count();
            assert!(red_pixels > 5, "{backend:?} did not paint red span");
            assert!(blue_pixels > 5, "{backend:?} did not paint blue span");
        }
    }

    #[test]
    #[cfg(any(feature = "cpu-vello", feature = "cpu-tiny-skia"))]
    fn underlined_rich_text_adds_decoration_pixels() {
        use craftui_text::{RichText, TextSpan};

        let mut style = TextStyle::default()
            .with_size(28.0)
            .with_color(Color::BLACK);
        let mut scene = PaintScene::new();
        scene.draw_rich_text(
            Point::new(4.0, 4.0),
            RichText::new().with_span(TextSpan::new("MMMM MMMM", style.clone())),
            Some(220.0),
        );
        let mut underlined = PaintScene::new();
        style.underline = true;
        underlined.draw_rich_text(
            Point::new(4.0, 4.0),
            RichText::new().with_span(TextSpan::new("MMMM MMMM", style)),
            Some(220.0),
        );
        let textures = TextureRegistry::new();
        let mut backends = Vec::new();
        #[cfg(feature = "cpu-vello")]
        backends.push(BackendKind::VelloCpu);
        #[cfg(feature = "cpu-tiny-skia")]
        backends.push(BackendKind::TinySkia);
        for backend in backends {
            let mut engine = TextEngine::new_without_system_fonts();
            let plain = render_scene_to_rgba(
                backend,
                &scene,
                240,
                60,
                1.0,
                Color::WHITE,
                &textures,
                &mut engine,
            );
            let decorated = render_scene_to_rgba(
                backend,
                &underlined,
                240,
                60,
                1.0,
                Color::WHITE,
                &textures,
                &mut engine,
            );
            let extra_pixels = plain
                .as_chunks::<4>()
                .0
                .iter()
                .zip(decorated.as_chunks::<4>().0.iter())
                .filter(|(before, after)| after[0].saturating_add(10) < before[0])
                .count();
            assert!(extra_pixels > 5, "{backend:?} did not paint an underline");
        }
    }

    #[test]
    fn cpu_and_tinyskia_render_rounded_rect_text_and_pixelbuffer() {
        let mut textures = TextureRegistry::new();
        let tex = textures.create_from_rgba(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, // row 0
                0, 0, 255, 255, 255, 255, 0, 255, // row 1
            ],
        );

        let mut scene = PaintScene::new();
        scene.fill_rect(
            Rect::new(10.0, 10.0, 80.0, 40.0),
            Brush::LinearGradient {
                start: Point::new(10.0, 10.0),
                end: Point::new(90.0, 50.0),
                stops: vec![
                    ColorStop {
                        offset: 0.0,
                        color: Color::rgb(59, 130, 246),
                    },
                    ColorStop {
                        offset: 0.5,
                        color: Color::rgb(139, 92, 246),
                    },
                    ColorStop {
                        offset: 1.0,
                        color: Color::rgb(16, 120, 220),
                    },
                ],
            },
            CornerRadii::all(8.0),
        );
        scene.push(PaintCommand::DrawImage {
            rect: Rect::new(100.0, 10.0, 32.0, 32.0),
            texture: tex.id,
            fit: Fit::Fill,
            radius: CornerRadii::all(4.0),
            opacity: 1.0,
        });
        scene.draw_text(
            Point::new(16.0, 22.0),
            "CraftUI",
            TextStyle::default()
                .with_size(14.0)
                .with_color(Color::WHITE),
            Some(120.0),
        );

        let mut text = TextEngine::new();
        let rgba = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            160,
            80,
            1.0,
            Color::rgb(17, 20, 26),
            &textures,
            &mut text,
        );
        assert_eq!(rgba.len(), 160 * 80 * 4);
        assert_eq!(&rgba[0..4], &[17, 20, 26, 255]);
        let idx = (12 * 160 + 102) * 4;
        assert!(rgba[idx] > 200 && rgba[idx + 1] < 50);

        tex.buffer.with_pixels_mut(|_, _, px| {
            px[0] = 0;
            px[1] = 255;
        });
        tex.buffer.commit_damage(Rect::new(0.0, 0.0, 1.0, 1.0));
        let rgba2 = render_scene_to_rgba(
            BackendKind::VelloCpu,
            &scene,
            160,
            80,
            1.0,
            Color::rgb(17, 20, 26),
            &textures,
            &mut text,
        );
        assert!(rgba2[idx + 1] > 200);

        // Verify TinySkia now renders multi-stop gradients AND text glyphs (BLOAT-02)!
        let tiny = render_scene_to_rgba(
            BackendKind::TinySkia,
            &scene,
            160,
            80,
            1.0,
            Color::rgb(17, 20, 26),
            &textures,
            &mut text,
        );
        assert_eq!(tiny.len(), 160 * 80 * 4);
        // Gradient middle stop at ~(50, 30) should reflect purple stop (#8b5cf6)
        let mid_idx = (30 * 160 + 50) * 4;
        assert!(tiny[mid_idx] > 80 && tiny[mid_idx + 2] > 180);
        // Text region (16..70, 22..36) must contain bright pixels near white
        let has_glyph_pixels = (22..38).any(|y| {
            (16..70).any(|x| {
                let p = &tiny[(y * 160 + x) * 4..][..4];
                p[0] > 220 && p[1] > 220 && p[2] > 220
            })
        });
        assert!(
            has_glyph_pixels,
            "TinySkiaBackend must rasterize DrawText glyphs"
        );
    }

    #[test]
    fn platform_view_hole_punches_transparent_cutout() {
        let mut scene = PaintScene::new();
        scene.push(PaintCommand::PlatformViewHole {
            id: craftui_core::PlatformViewId::default(),
            rect: Rect::new(20.0, 20.0, 40.0, 30.0),
        });
        let textures = TextureRegistry::new();
        let mut text = TextEngine::new();
        for backend in [
            BackendKind::VelloCpu,
            BackendKind::TinySkia,
            BackendKind::GpuSdf,
        ] {
            let rgba = render_scene_to_rgba(
                backend,
                &scene,
                80,
                80,
                1.0,
                Color::rgb(30, 41, 59),
                &textures,
                &mut text,
            );
            assert_eq!(rgba[3], 255);
            let hole_idx = (35 * 80 + 40) * 4;
            assert_eq!(rgba[hole_idx + 3], 0);
        }
    }

    #[test]
    fn transforms_backdrop_blur_shadows_and_blend_layers_render_cleanly() {
        let mut scene = PaintScene::new();
        // High-contrast vertical stripe background
        scene.fill_rect(
            Rect::new(0.0, 0.0, 60.0, 120.0),
            Color::rgb(240, 40, 40),
            CornerRadii::ZERO,
        );
        scene.fill_rect(
            Rect::new(60.0, 0.0, 60.0, 120.0),
            Color::rgb(40, 40, 240),
            CornerRadii::ZERO,
        );

        // BackdropBlur frosted panel spanning x=30..90 over red/blue boundary
        scene.backdrop_blur(
            Rect::new(30.0, 30.0, 60.0, 60.0),
            CornerRadii::all(10.0),
            8.0,
            Color::rgba(255, 255, 255, 40),
        );

        // Transformed rotated + scaled pill inside bounded opacity blend layer
        scene.push_layer(Rect::new(10.0, 10.0, 100.0, 100.0), 0.75, BlendMode::Screen);
        scene.drop_shadow(
            Rect::new(40.0, 40.0, 40.0, 40.0),
            CornerRadii::new(8.0, 12.0, 8.0, 4.0),
            BoxShadow::drop(0.0, 3.0, 6.0, 2.0, Color::rgba(0, 0, 0, 120)),
        );
        scene.push_transform(Transform2D::around_pivot(
            Point::new(60.0, 60.0),
            Transform2D::rotation(0.4),
        ));
        scene.fill_rect(
            Rect::new(45.0, 45.0, 30.0, 30.0),
            Color::rgb(34, 197, 94),
            CornerRadii::all(6.0),
        );
        scene.pop_transform();
        scene.pop_layer();

        // Inset shadow pill
        scene.drop_shadow(
            Rect::new(10.0, 95.0, 40.0, 16.0),
            CornerRadii::all(8.0),
            BoxShadow::inner(0.0, 2.0, 4.0, 0.0, Color::rgba(0, 0, 0, 160)),
        );

        let textures = TextureRegistry::new();
        let mut text = TextEngine::new();
        for backend in [
            BackendKind::VelloCpu,
            BackendKind::TinySkia,
            BackendKind::GpuSdf,
        ] {
            let rgba = render_scene_to_rgba(
                backend,
                &scene,
                120,
                120,
                1.0,
                Color::BLACK,
                &textures,
                &mut text,
            );
            assert_eq!(rgba.len(), 120 * 120 * 4);
            // Outside frosted region at (10, 10) remains red-dominant
            let p_left = &rgba[(10 * 120 + 10) * 4..][..4];
            assert!(p_left[0] > 180 && p_left[2] < 80);
            // Inside frosted stripe boundary at (59, 36) should contain mixed red & blue from blur
            let p_seam = &rgba[(36 * 120 + 59) * 4..][..4];
            assert!(
                p_seam[0] > 60 && p_seam[2] > 60,
                "expected blurred seam pixel on {backend:?}, got {p_seam:?}"
            );
        }
    }
}
