use crate::cpu_backend::SoftwareRenderer;
#[cfg(feature = "gpu-sdf")]
use crate::gpu_sdf_backend::GpuSdfBackend;
use crate::scene::PaintScene;
#[cfg(feature = "gpu-vello")]
use crate::scene::{PaintCommand, brush_to_peniko, compute_image_transform};
use crate::texture::TextureRegistry;
#[cfg(feature = "gpu-vello")]
use craftui_core::{BlendMode, TextureId, Transform2D};
use craftui_core::{Color, CraftError, Result};
use craftui_text::TextEngine;
#[cfg(feature = "gpu-vello")]
use kurbo::{Affine, Stroke};
#[cfg(feature = "gpu-vello")]
use parley::PositionedLayoutItem;
#[cfg(feature = "gpu-vello")]
use std::collections::HashMap;
#[cfg(feature = "gpu-vello")]
use std::num::NonZeroUsize;
#[cfg(feature = "gpu-vello")]
use vello::AaConfig;
#[cfg(feature = "gpu-vello")]
use vello::peniko;
#[cfg(feature = "gpu-vello")]
use vello::{RenderParams, Renderer, RendererOptions, Scene};
use wgpu::util::TextureBlitter;

#[cfg(feature = "gpu-vello")]
struct CachedGpuImage {
    version: u64,
    brush: peniko::ImageBrush,
}

#[cfg(feature = "gpu-vello")]
struct VelloGpuPipeline {
    renderer: Box<Renderer>,
    target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,
    scene: Box<Scene>,
    image_cache: HashMap<TextureId, CachedGpuImage>,
    blitter: TextureBlitter,
}

struct CpuUploadPipeline {
    cpu: Box<SoftwareRenderer>,
    upload_texture: wgpu::Texture,
    upload_view: wgpu::TextureView,
    blitter: TextureBlitter,
}

enum ActivePipelineKind {
    #[cfg(feature = "gpu-sdf")]
    GpuSdf(Box<GpuSdfBackend>),
    #[cfg(feature = "gpu-vello")]
    VelloGpu(Box<VelloGpuPipeline>),
    CpuUpload(Box<CpuUploadPipeline>),
}

/// Active GPU/CPU rendering pipeline backed by `GpuSdfBackend`, `vello`, or software upload.
pub struct GpuSurfacePipeline {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    active: ActivePipelineKind,
}

fn default_wgpu_backends() -> wgpu::Backends {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Backends::from_env().unwrap_or_default()
    }
}

fn default_instance_flags() -> wgpu::InstanceFlags {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::InstanceFlags::default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::InstanceFlags::from_env_or_default()
    }
}

#[derive(Clone, Copy)]
struct AdapterAttempt {
    backends: wgpu::Backends,
    flags: wgpu::InstanceFlags,
    force_fallback_adapter: bool,
}

impl GpuSurfacePipeline {
    /// Synchronously initialize `wgpu` surface + active GPU/CPU renderer (native platforms only).
    ///
    /// On `wasm32`, use [`GpuSurfacePipeline::new_async`].
    pub fn new(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (window, width, height, force_cpu);
            Err(CraftError::Render(
                "GpuSurfacePipeline::new is unsupported on wasm32; use GpuSurfacePipeline::new_async"
                    .into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            pollster::block_on(Self::new_async(window, width, height, force_cpu))
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_display<T>(
        target_factory: impl Fn() -> T,
        display_factory: impl Fn() -> Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        pollster::block_on(Self::new_with_display_async(
            target_factory,
            display_factory,
            width,
            height,
            force_cpu,
        ))
    }

    pub async fn new_with_display_async<T>(
        target_factory: impl Fn() -> T,
        display_factory: impl Fn() -> Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self>
    where
        T: Into<wgpu::SurfaceTarget<'static>>,
    {
        let base_backends = default_wgpu_backends();
        let base_flags = default_instance_flags();
        let env_pinned_backend = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                wgpu::Backends::from_env().is_some()
            }
            #[cfg(target_arch = "wasm32")]
            {
                false
            }
        };

        let attempts = if !env_pinned_backend && cfg!(not(target_arch = "wasm32")) {
            vec![
                AdapterAttempt {
                    backends: wgpu::Backends::PRIMARY,
                    flags: base_flags,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: wgpu::Backends::PRIMARY,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: wgpu::Backends::GL,
                    flags: base_flags,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: true,
                },
            ]
        } else {
            vec![
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: false,
                },
                AdapterAttempt {
                    backends: base_backends,
                    flags: base_flags | wgpu::InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER,
                    force_fallback_adapter: true,
                },
            ]
        };

        let mut errors = Vec::new();
        for attempt in attempts {
            let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(display_factory());
            desc.backends = attempt.backends;
            desc.flags = attempt.flags;
            #[cfg(not(target_arch = "wasm32"))]
            {
                desc.backend_options = desc.backend_options.with_env();
            }

            let instance = wgpu::Instance::new(desc);
            let surface = match instance.create_surface(target_factory()) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("{:?} create_surface: {e}", attempt.backends));
                    continue;
                }
            };

            match Self::from_instance_and_surface_with_fallback(
                instance,
                surface,
                width,
                height,
                force_cpu,
                attempt.force_fallback_adapter,
            )
            .await
            {
                Ok(pipeline) => return Ok(pipeline),
                Err(e) => {
                    errors.push(format!(
                        "{:?} (fallback={}): {e}",
                        attempt.backends, attempt.force_fallback_adapter
                    ));
                }
            }
        }

        Err(CraftError::Render(format!(
            "all wgpu surface/adapter attempts failed: {}",
            errors.join(" | ")
        )))
    }

    pub async fn new_async(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = default_wgpu_backends();
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window)
            .map_err(|e| CraftError::Render(format!("create_surface failed: {e}")))?;
        Self::from_instance_and_surface_async(instance, surface, width, height, force_cpu).await
    }

    /// Initialize GPU/CPU pipeline from an unsafe raw native surface target (e.g. Android `ANativeWindow` or iOS `CAMetalLayer`).
    ///
    /// # Safety
    /// Caller must ensure `target` references a live native window or layer handle that outlives the returned [`GpuSurfacePipeline`].
    pub unsafe fn new_unsafe(
        target: wgpu::SurfaceTargetUnsafe,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (target, width, height, force_cpu);
            Err(CraftError::Render(
                "GpuSurfacePipeline::new_unsafe is unsupported on wasm32".into(),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            desc.backends = default_wgpu_backends();
            let instance = wgpu::Instance::new(desc);
            // SAFETY: forwarded from `new_unsafe` caller invariant that `target` outlives the surface.
            let surface = unsafe { instance.create_surface_unsafe(target) }
                .map_err(|e| CraftError::Render(format!("create_surface_unsafe failed: {e}")))?;
            pollster::block_on(Self::from_instance_and_surface_async(
                instance, surface, width, height, force_cpu,
            ))
        }
    }

    pub async fn from_instance_and_surface_async(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        force_cpu: bool,
    ) -> Result<Self> {
        Self::from_instance_and_surface_with_fallback(
            instance, surface, width, height, force_cpu, false,
        )
        .await
    }

    async fn from_instance_and_surface_with_fallback(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        force_cpu: bool,
        force_fallback_adapter: bool,
    ) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| CraftError::Render(format!("no compatible wgpu adapter: {e}")))?;

        let adapter_info = adapter.get_info();
        let supports_compute = adapter_info.backend != wgpu::Backend::Gl
            && adapter.limits().max_compute_workgroups_per_dimension > 0
            && adapter.limits().max_storage_buffers_per_shader_stage >= 4;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("craftui-wgpu-device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| CraftError::Render(format!("request_device failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        let default_fmt = *caps.formats.first().ok_or_else(|| {
            CraftError::Render("wgpu surface reports no supported pixel formats".into())
        })?;
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .unwrap_or(default_fmt);

        let w = width.max(1);
        let h = height.max(1);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let prefer_vello = {
            #[cfg(not(target_arch = "wasm32"))]
            {
                std::env::var("CRAFTUI_RENDERER")
                    .map(|v| v.eq_ignore_ascii_case("vello"))
                    .unwrap_or(false)
            }
            #[cfg(target_arch = "wasm32")]
            {
                false
            }
        };

        let active = Self::select_backend(
            &device,
            &queue,
            w,
            h,
            format,
            force_cpu,
            prefer_vello,
            supports_compute,
        );

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface,
            config,
            active,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn select_backend(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        format: wgpu::TextureFormat,
        force_cpu: bool,
        prefer_vello: bool,
        supports_compute: bool,
    ) -> ActivePipelineKind {
        #[cfg(feature = "gpu-vello")]
        if !force_cpu && (prefer_vello || !cfg!(feature = "gpu-sdf")) && supports_compute {
            match Renderer::new(
                device,
                RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::area_only(),
                    num_init_threads: NonZeroUsize::new(1),
                    pipeline_cache: None,
                },
            ) {
                Ok(renderer) => {
                    let (target_texture, target_view) =
                        create_vello_target(device, w, h, format.is_srgb());
                    let blitter = TextureBlitter::new(device, format);
                    return ActivePipelineKind::VelloGpu(Box::new(VelloGpuPipeline {
                        renderer: Box::new(renderer),
                        target_texture,
                        target_view,
                        scene: Box::new(Scene::new()),
                        image_cache: HashMap::new(),
                        blitter,
                    }));
                }
                Err(err) => {
                    tracing::warn!("Vello GPU init failed ({err}); falling back");
                }
            }
        }
        #[cfg(not(feature = "gpu-vello"))]
        let _ = (prefer_vello, supports_compute);

        #[cfg(feature = "gpu-sdf")]
        if !force_cpu {
            let backend = GpuSdfBackend::new(device, queue, w, h, format);
            return ActivePipelineKind::GpuSdf(Box::new(backend));
        }
        #[cfg(not(feature = "gpu-sdf"))]
        let _ = queue;

        let (upload_texture, upload_view) =
            create_cpu_upload_target(device, w, h, format.is_srgb());
        let blitter = TextureBlitter::new(device, format);
        ActivePipelineKind::CpuUpload(Box::new(CpuUploadPipeline {
            cpu: Box::new(SoftwareRenderer::new(w, h)),
            upload_texture,
            upload_view,
            blitter,
        }))
    }

    pub fn is_using_gpu(&self) -> bool {
        !matches!(self.active, ActivePipelineKind::CpuUpload(_))
    }

    pub fn active_backend_name(&self) -> &'static str {
        match &self.active {
            #[cfg(feature = "gpu-sdf")]
            ActivePipelineKind::GpuSdf(_) => "wgpu GpuSdf",
            #[cfg(feature = "gpu-vello")]
            ActivePipelineKind::VelloGpu(_) => "wgpu Vello GPU",
            ActivePipelineKind::CpuUpload(_) => "wgpu + CPU fallback",
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.config.width == w && self.config.height == h {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        let srgb = self.config.format.is_srgb();
        match &mut self.active {
            #[cfg(feature = "gpu-sdf")]
            ActivePipelineKind::GpuSdf(sdf) => sdf.resize(&self.device, w, h),
            #[cfg(feature = "gpu-vello")]
            ActivePipelineKind::VelloGpu(vello) => {
                let (tex, view) = create_vello_target(&self.device, w, h, srgb);
                vello.target_texture = tex;
                vello.target_view = view;
            }
            ActivePipelineKind::CpuUpload(upload) => {
                let (tex, view) = create_cpu_upload_target(&self.device, w, h, srgb);
                upload.upload_texture = tex;
                upload.upload_view = view;
            }
        }
    }

    pub fn render_frame(
        &mut self,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) -> Result<()> {
        let w = self.config.width;
        let h = self.config.height;

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                    wgpu::CurrentSurfaceTexture::Timeout
                    | wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
                    other => {
                        return Err(CraftError::Render(format!(
                            "surface acquire failed: {other:?}"
                        )));
                    }
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            other => {
                return Err(CraftError::Render(format!("surface error: {other:?}")));
            }
        };

        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("craftui-render-encoder"),
            });

        match &mut self.active {
            #[cfg(feature = "gpu-sdf")]
            ActivePipelineKind::GpuSdf(sdf) => {
                sdf.render_to_scene(
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    paint_scene,
                    scale_factor,
                    background,
                    textures,
                    text_engine,
                );
                sdf.present_to_surface(
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    &surface_view,
                    self.config.format,
                );
            }
            #[cfg(feature = "gpu-vello")]
            ActivePipelineKind::VelloGpu(vello) => {
                encode_vello_scene(
                    &mut vello.scene,
                    paint_scene,
                    scale_factor,
                    textures,
                    text_engine,
                    &mut vello.image_cache,
                );
                vello
                    .renderer
                    .render_to_texture(
                        &self.device,
                        &self.queue,
                        &vello.scene,
                        &vello.target_view,
                        &RenderParams {
                            base_color: background.to_peniko(),
                            width: w,
                            height: h,
                            antialiasing_method: AaConfig::Area,
                        },
                    )
                    .map_err(|e| CraftError::Render(format!("vello render failed: {e}")))?;
                vello.blitter.copy(
                    &self.device,
                    &mut encoder,
                    &vello.target_view,
                    &surface_view,
                );
            }
            ActivePipelineKind::CpuUpload(upload) => {
                let straight = upload.cpu.render_straight_rgba(
                    paint_scene,
                    w,
                    h,
                    scale_factor,
                    background,
                    textures,
                    text_engine,
                );
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &upload.upload_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &straight,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                upload.blitter.copy(
                    &self.device,
                    &mut encoder,
                    &upload.upload_view,
                    &surface_view,
                );
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}

#[cfg(feature = "gpu-vello")]
fn create_vello_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    surface_is_srgb: bool,
) -> (wgpu::Texture, wgpu::TextureView) {
    let view_formats = if surface_is_srgb {
        vec![wgpu::TextureFormat::Rgba8UnormSrgb]
    } else {
        vec![]
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("craftui-vello-target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &view_formats,
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_cpu_upload_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    surface_is_srgb: bool,
) -> (wgpu::Texture, wgpu::TextureView) {
    let view_formats = if surface_is_srgb {
        vec![wgpu::TextureFormat::Rgba8UnormSrgb]
    } else {
        vec![]
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("craftui-cpu-upload"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &view_formats,
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(if surface_is_srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        }),
        ..Default::default()
    });
    (texture, view)
}

#[cfg(feature = "gpu-vello")]
fn to_vello_blend(mode: BlendMode) -> peniko::BlendMode {
    match mode {
        BlendMode::Normal => peniko::BlendMode::default(),
        BlendMode::Multiply => peniko::Mix::Multiply.into(),
        BlendMode::Screen => peniko::Mix::Screen.into(),
        BlendMode::Overlay => peniko::Mix::Overlay.into(),
        BlendMode::Darken => peniko::Mix::Darken.into(),
        BlendMode::Lighten => peniko::Mix::Lighten.into(),
        BlendMode::Plus => peniko::Compose::Plus.into(),
    }
}

#[cfg(feature = "gpu-vello")]
fn encode_vello_scene(
    scene: &mut Scene,
    paint_scene: &PaintScene,
    scale_factor: f32,
    textures: &TextureRegistry,
    text_engine: &mut TextEngine,
    image_cache: &mut HashMap<TextureId, CachedGpuImage>,
) {
    scene.reset();
    let base_scale = Affine::scale(scale_factor as f64);
    let mut xform_stack = vec![Transform2D::IDENTITY];
    let mut layer_depth = 0usize;

    for cmd in &paint_scene.commands {
        let cur_xform = *xform_stack.last().unwrap();
        let eff = base_scale * cur_xform.to_kurbo();
        match cmd {
            PaintCommand::FillRect {
                rect,
                brush,
                radius,
            } => {
                let p_brush = brush_to_peniko(brush);
                if radius.is_zero() {
                    scene.fill(peniko::Fill::NonZero, eff, &p_brush, None, &rect.to_kurbo());
                } else {
                    scene.fill(
                        peniko::Fill::NonZero,
                        eff,
                        &p_brush,
                        None,
                        &rect.to_rounded_kurbo(*radius),
                    );
                }
            }
            PaintCommand::StrokeRect {
                rect,
                color,
                width,
                radius,
            } => {
                let stroke = Stroke::new(*width as f64);
                if radius.is_zero() {
                    scene.stroke(&stroke, eff, color.to_peniko(), None, &rect.to_kurbo());
                } else {
                    scene.stroke(
                        &stroke,
                        eff,
                        color.to_peniko(),
                        None,
                        &rect.to_rounded_kurbo(*radius),
                    );
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
                let avg_r =
                    (radius.top_left + radius.top_right + radius.bottom_right + radius.bottom_left)
                        * 0.25;
                if *inset {
                    scene.push_clip_layer(
                        peniko::Fill::NonZero,
                        eff,
                        &rect.to_rounded_kurbo(*radius),
                    );
                    let border = (*std_dev + spread.abs()).max(1.0);
                    let s = Stroke::new(border as f64);
                    scene.stroke(
                        &s,
                        eff,
                        color.to_peniko(),
                        None,
                        &rect
                            .translate(*offset_x, *offset_y)
                            .to_rounded_kurbo(*radius),
                    );
                    scene.pop_layer();
                } else {
                    let shifted = rect
                        .translate(*offset_x, *offset_y)
                        .inflate(*spread, *spread)
                        .to_kurbo();
                    let adj_r = if avg_r > 0.0 || *spread > 0.0 {
                        (avg_r + *spread).max(0.0)
                    } else {
                        0.0
                    };
                    scene.draw_blurred_rounded_rect(
                        eff,
                        shifted,
                        color.to_peniko(),
                        adj_r as f64,
                        *std_dev as f64,
                    );
                }
            }
            PaintCommand::BackdropBlur {
                rect, radius, tint, ..
            } => {
                // Vello compute scenes are forward-only; draw translucent frosted tint surface cleanly
                let frosted = Color::rgba(tint.r, tint.g, tint.b, tint.a.max(96));
                scene.fill(
                    peniko::Fill::NonZero,
                    eff,
                    frosted.to_peniko(),
                    None,
                    &rect.to_rounded_kurbo(*radius),
                );
            }
            PaintCommand::FillPath { path, brush } => {
                let p_brush = brush_to_peniko(brush);
                scene.fill(peniko::Fill::NonZero, eff, &p_brush, None, path);
            }
            PaintCommand::StrokePath { path, color, width } => {
                scene.stroke(
                    &Stroke::new(*width as f64),
                    eff,
                    color.to_peniko(),
                    None,
                    path,
                );
            }
            PaintCommand::DrawImage {
                rect,
                texture,
                fit,
                radius,
                opacity,
            } => {
                if let Some(handle) = textures.get(*texture) {
                    let ver = handle.version();
                    let need_refresh = image_cache.get(texture).is_none_or(|c| c.version != ver);
                    let (w, h) = if need_refresh {
                        let (w, h, ver, rgba) = handle.snapshot();
                        if w == 0 || h == 0 {
                            continue;
                        }
                        let brush = peniko::ImageBrush::new(peniko::ImageData {
                            data: peniko::Blob::from(rgba),
                            format: peniko::ImageFormat::Rgba8,
                            alpha_type: peniko::ImageAlphaType::Alpha,
                            width: w,
                            height: h,
                        });
                        image_cache.insert(
                            *texture,
                            CachedGpuImage {
                                version: ver,
                                brush,
                            },
                        );
                        (w, h)
                    } else {
                        handle.dimensions()
                    };
                    if let Some(cached) = image_cache.get(texture) {
                        let (_, img_aff) = compute_image_transform(*rect, w, h, *fit);
                        scene.push_clip_layer(
                            peniko::Fill::NonZero,
                            eff,
                            &rect.to_rounded_kurbo(*radius),
                        );
                        let brush = cached.brush.clone().with_alpha(*opacity);
                        scene.draw_image(&brush, eff * img_aff);
                        scene.pop_layer();
                    }
                }
            }
            PaintCommand::DrawText { origin, .. } | PaintCommand::DrawRichText { origin, .. } => {
                let Some((layout, color_override)) = cmd.shaped_text(text_engine) else {
                    continue;
                };
                let text_xform = eff * Affine::translate((origin.x as f64, origin.y as f64));
                let mut coords_buf: Vec<i16> = Vec::new();
                for line in layout.lines() {
                    for item in line.items() {
                        if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                            let paint_color = color_override
                                .unwrap_or(glyph_run.style().brush)
                                .to_peniko();
                            let run = glyph_run.run();
                            let font = run.font();
                            let norm = run.normalized_coords();
                            let coords_slice: &[i16] = if norm.is_empty() {
                                &[]
                            } else {
                                coords_buf.clear();
                                coords_buf.extend(norm.iter().map(|c| c.to_bits()));
                                &coords_buf
                            };
                            let glyphs = glyph_run.positioned_glyphs().map(|g| vello::Glyph {
                                id: g.id,
                                x: g.x,
                                y: g.y,
                            });
                            scene
                                .draw_glyphs(font)
                                .font_size(run.font_size())
                                .hint(true)
                                .transform(text_xform)
                                .normalized_coords(coords_slice)
                                .brush(paint_color)
                                .draw(peniko::Fill::NonZero, glyphs);
                            if let Some((underline, color)) =
                                crate::scene::underline_rect(&glyph_run, color_override)
                            {
                                scene.fill(
                                    peniko::Fill::NonZero,
                                    text_xform,
                                    color.to_peniko(),
                                    None,
                                    &underline.to_kurbo(),
                                );
                            }
                        }
                    }
                }
            }
            PaintCommand::PushClip { rect, radius } => {
                if radius.is_zero() {
                    scene.push_clip_layer(peniko::Fill::NonZero, eff, &rect.to_kurbo());
                } else {
                    scene.push_clip_layer(
                        peniko::Fill::NonZero,
                        eff,
                        &rect.to_rounded_kurbo(*radius),
                    );
                }
                layer_depth += 1;
            }
            PaintCommand::PopClip => {
                if layer_depth > 0 {
                    scene.pop_layer();
                    layer_depth -= 1;
                }
            }
            PaintCommand::PushLayer {
                rect,
                opacity,
                blend_mode,
            } => {
                let bounded = if rect.width > 0.0 && rect.height > 0.0 {
                    rect.inflate(64.0, 64.0).to_kurbo()
                } else {
                    kurbo::Rect::new(-4096.0, -4096.0, 4096.0, 4096.0)
                };
                scene.push_layer(
                    peniko::Fill::NonZero,
                    to_vello_blend(*blend_mode),
                    *opacity,
                    eff,
                    &bounded,
                );
                layer_depth += 1;
            }
            PaintCommand::PopLayer => {
                if layer_depth > 0 {
                    scene.pop_layer();
                    layer_depth -= 1;
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
                scene.push_layer(
                    peniko::Fill::NonZero,
                    peniko::BlendMode::from(peniko::Compose::Clear),
                    1.0,
                    eff,
                    &r,
                );
                scene.fill(
                    peniko::Fill::NonZero,
                    eff,
                    Color::BLACK.to_peniko(),
                    None,
                    &r,
                );
                scene.pop_layer();
            }
        }
    }

    while layer_depth > 0 {
        scene.pop_layer();
        layer_depth -= 1;
    }
}

/// Render a scene headlessly via `GpuSdfBackend` onto an offscreen `wgpu` texture and read back
/// straight RGBA8 bytes (falls back to `None` if no GPU/software Vulkan/GL adapter is present).
#[cfg(all(feature = "gpu-sdf", not(target_arch = "wasm32")))]
pub fn try_render_gpu_sdf_headless(
    scene: &PaintScene,
    width: u32,
    height: u32,
    scale_factor: f32,
    background: Color,
    textures: &TextureRegistry,
    text_engine: &mut TextEngine,
) -> Option<Vec<u8>> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()
    .or_else(|| {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: true,
            apply_limit_buckets: false,
        }))
        .ok()
    })?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("craftui-headless-sdf"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    let w = width.max(1);
    let h = height.max(1);
    let mut sdf = GpuSdfBackend::new(&device, &queue, w, h, wgpu::TextureFormat::Rgba8Unorm);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    sdf.render_to_scene(
        &device,
        &queue,
        &mut encoder,
        scene,
        scale_factor,
        background,
        textures,
        text_engine,
    );

    let unpadded_bpr = w * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bpr = unpadded_bpr.div_ceil(align) * align;
    let read_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("craftui-sdf-readback"),
        size: (padded_bpr * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: sdf.scene_texture(),
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &read_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = read_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().ok()?.ok()?;

    let mapped = slice.get_mapped_range().ok()?;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for row in 0..h as usize {
        let src_start = row * padded_bpr as usize;
        let dst_start = row * (w * 4) as usize;
        out[dst_start..dst_start + (w * 4) as usize]
            .copy_from_slice(&mapped[src_start..src_start + (w * 4) as usize]);
    }
    drop(mapped);
    read_buf.unmap();

    // Unpremultiply premul Rgba8Unorm target into straight RGBA8
    for px in out.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a > 0 && a < 255 {
            let af = 255.0 / a as f32;
            px[0] = ((px[0] as f32 * af).round() as u16).min(255) as u8;
            px[1] = ((px[1] as f32 * af).round() as u16).min(255) as u8;
            px[2] = ((px[2] as f32 * af).round() as u16).min(255) as u8;
        }
    }
    Some(out)
}
