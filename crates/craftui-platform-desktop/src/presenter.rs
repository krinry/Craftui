use craftui_core::{Color, CraftError, Result};
#[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
use craftui_render::GpuSurfacePipeline;
use craftui_render::{PaintScene, SoftwareRenderer, TextureRegistry};
use craftui_text::TextEngine;
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

pub(crate) struct SoftbufferPresenter {
    surface: softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
    cpu: SoftwareRenderer,
    width: u32,
    height: u32,
}

impl SoftbufferPresenter {
    pub(crate) fn new(
        context: &softbuffer::Context<OwnedDisplayHandle>,
        window: Arc<Window>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        let mut surface = softbuffer::Surface::new(context, window)
            .map_err(|e| CraftError::Render(format!("softbuffer surface creation failed: {e}")))?;
        let w = width.max(1);
        let h = height.max(1);
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            surface
                .resize(nw, nh)
                .map_err(|e| CraftError::Render(format!("softbuffer resize failed: {e}")))?;
        }
        Ok(Self {
            surface,
            cpu: SoftwareRenderer::new(w, h),
            width: w,
            height: h,
        })
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return;
        }
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h))
            && self.surface.resize(nw, nh).is_ok()
        {
            self.width = w;
            self.height = h;
        }
    }

    pub(crate) fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        let w = self.width.max(1);
        let h = self.height.max(1);
        let rgba = self.cpu.render_straight_rgba(
            paint_scene,
            w,
            h,
            scale_factor,
            background,
            textures,
            text_engine,
        );
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|e| CraftError::Render(format!("softbuffer buffer_mut failed: {e}")))?;
        pack_rgba8_to_softbuffer_0rgb(&rgba, &mut buffer);
        buffer
            .present()
            .map_err(|e| CraftError::Render(format!("softbuffer present failed: {e}")))?;
        Ok(())
    }
}

/// Convert straight RGBA8 pixels (`[r, g, b, a]`) into `softbuffer`'s `0x00RRGGBB` layout.
pub fn pack_rgba8_to_softbuffer_0rgb(rgba: &[u8], dst: &mut [u32]) {
    let (chunks, _) = rgba.as_chunks::<4>();
    for (pixel_out, chunk) in dst.iter_mut().zip(chunks.iter()) {
        *pixel_out = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
    }
}

pub(crate) enum DesktopPresenter {
    #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
    Wgpu(Box<GpuSurfacePipeline>),
    Softbuffer(Box<SoftbufferPresenter>),
}

impl DesktopPresenter {
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => gpu.resize(width, height),
            Self::Softbuffer(sb) => sb.resize(width, height),
        }
    }

    pub(crate) fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => {
                gpu.render_frame(paint_scene, scale_factor, background, textures, text_engine)
            }
            Self::Softbuffer(sb) => {
                sb.render_frame(paint_scene, scale_factor, background, textures, text_engine)
            }
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            #[cfg(any(feature = "gpu-sdf", feature = "gpu-vello"))]
            Self::Wgpu(gpu) => gpu.active_backend_name(),
            Self::Softbuffer(_) => "softbuffer (CPU)",
        }
    }
}
