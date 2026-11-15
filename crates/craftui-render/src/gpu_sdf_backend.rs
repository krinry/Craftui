//! Pure render-pipeline WGSL GPU backend (`gpu-sdf`) supporting WebGPU, WebGL2, OpenGL ES 3,
//! Vulkan, Metal, and DX12 with zero compute shaders or vertex storage buffers.

use crate::glyph_raster::{rasterize_bez_path, rasterize_glyph};
use crate::scene::{PaintCommand, PaintScene, compute_image_transform};
use crate::texture::TextureRegistry;
use bytemuck::{Pod, Zeroable};
use craftui_core::{BlendMode, Brush, Color, ColorStop, CornerRadii, Rect, TextureId, Transform2D};
use craftui_text::TextEngine;
use guillotiere::{AtlasAllocator, size2};
use kurbo::{BezPath, PathEl, Stroke, StrokeOpts, stroke as kurbo_stroke};
use parley::PositionedLayoutItem;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use wgpu::util::DeviceExt;

const SDF_QUAD_WGSL: &str = include_str!("sdf_quad.wgsl");

const PRESENT_BLEND_WGSL: &str = r#"
struct FullscreenOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) idx: u32) -> FullscreenOut {
    let x = select(-1.0, 3.0, idx == 1u);
    let y = select(-1.0, 3.0, idx == 2u);
    var out: FullscreenOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct PresentUniforms {
    surface_is_srgb: f32,
    opacity: f32,
    blend_mode: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> u_present: PresentUniforms;
@group(0) @binding(1) var u_tex0: texture_2d<f32>;
@group(0) @binding(2) var u_tex1: texture_2d<f32>;
@group(0) @binding(3) var u_sampler: sampler;

fn srgb_chan_to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_present(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = textureSample(u_tex0, u_sampler, in.uv);
    let unused_keep = textureSample(u_tex1, u_sampler, vec2<f32>(0.5, 0.5)).a * 0.0;
    if (u_present.surface_is_srgb > 0.5) {
        let a = max(p.a, 1e-5);
        let straight = p.rgb / a;
        let lin = vec3<f32>(
            srgb_chan_to_linear(straight.r),
            srgb_chan_to_linear(straight.g),
            srgb_chan_to_linear(straight.b),
        );
        return vec4<f32>(lin * p.a, p.a + unused_keep);
    }
    return vec4<f32>(p.rgb, p.a + unused_keep);
}

fn mix_blend_mode(cb: vec3<f32>, cs: vec3<f32>, mode: i32) -> vec3<f32> {
    if (mode == 1) { return cb * cs; }
    if (mode == 2) { return cb + cs - cb * cs; }
    if (mode == 3) {
        let lo = 2.0 * cb * cs;
        let hi = vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - cb) * (vec3<f32>(1.0) - cs);
        return select(hi, lo, cb <= vec3<f32>(0.5));
    }
    if (mode == 4) { return min(cb, cs); }
    if (mode == 5) { return max(cb, cs); }
    return cs;
}

@fragment
fn fs_blend_layer(in: FullscreenOut) -> @location(0) vec4<f32> {
    let src = textureSample(u_tex0, u_sampler, in.uv) * u_present.opacity;
    let dst = textureSample(u_tex1, u_sampler, in.uv);
    let mode = i32(round(u_present.blend_mode));

    if (mode == 6) {
        return min(dst + src, vec4<f32>(1.0));
    }
    if (mode == 0) {
        return src + dst * (1.0 - src.a);
    }
    let sa = src.a;
    let da = dst.a;
    if (sa <= 1e-4) {
        return dst;
    }
    if (da <= 1e-4) {
        return src;
    }
    let cs = clamp(src.rgb / sa, vec3<f32>(0.0), vec3<f32>(1.0));
    let cb = clamp(dst.rgb / da, vec3<f32>(0.0), vec3<f32>(1.0));
    let blended = mix_blend_mode(cb, cs, mode);
    let out_rgb = (1.0 - da) * src.rgb + (1.0 - sa) * dst.rgb + sa * da * blended;
    let out_a = sa + da - sa * da;
    return vec4<f32>(out_rgb, out_a);
}
"#;

const BLUR_WGSL: &str = r#"
struct FullscreenOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) idx: u32) -> FullscreenOut {
    let x = select(-1.0, 3.0, idx == 1u);
    let y = select(-1.0, 3.0, idx == 2u);
    var out: FullscreenOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct BlurParams {
    texel_size: vec2<f32>,
    offset: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> u_blur: BlurParams;
@group(0) @binding(1) var u_blur_tex: texture_2d<f32>;
@group(0) @binding(2) var u_blur_sampler: sampler;

@fragment
fn fs_kawase_down(in: FullscreenOut) -> @location(0) vec4<f32> {
    let hp = u_blur.texel_size * u_blur.offset;
    var sum = textureSample(u_blur_tex, u_blur_sampler, in.uv) * 4.0;
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(-hp.x, -hp.y));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(hp.x, -hp.y));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(-hp.x, hp.y));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(hp.x, hp.y));
    return sum * 0.125;
}

@fragment
fn fs_kawase_up(in: FullscreenOut) -> @location(0) vec4<f32> {
    let hp = u_blur.texel_size * u_blur.offset;
    var sum = textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(-hp.x * 2.0, 0.0));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(-hp.x, hp.y)) * 2.0;
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(0.0, hp.y * 2.0));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(hp.x, hp.y)) * 2.0;
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(hp.x * 2.0, 0.0));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(hp.x, -hp.y)) * 2.0;
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(0.0, -hp.y * 2.0));
    sum = sum + textureSample(u_blur_tex, u_blur_sampler, in.uv + vec2<f32>(-hp.x, -hp.y)) * 2.0;
    return sum / 12.0;
}
"#;

const BACKDROP_WGSL: &str = r#"
struct FullscreenOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) idx: u32) -> FullscreenOut {
    let x = select(-1.0, 3.0, idx == 1u);
    let y = select(-1.0, 3.0, idx == 2u);
    var out: FullscreenOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct BackdropCompositeUniforms {
    target_size: vec2<f32>,
    scale_factor: f32,
    pad0: f32,
    inv_xform_0: vec4<f32>,
    inv_xform_1: vec4<f32>,
    rect: vec4<f32>,
    radii: vec4<f32>,
    tint: vec4<f32>,
    clip_rect: vec4<f32>,
    clip_radii: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u_backdrop: BackdropCompositeUniforms;
@group(0) @binding(1) var u_blurred_tex: texture_2d<f32>;
@group(0) @binding(2) var u_bd_sampler: sampler;

fn sd_rbox_bd(p_pos: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>) -> f32 {
    let half_size = rect.zw * 0.5;
    let p = p_pos - (rect.xy + half_size);
    let top_r = select(radii.x, radii.y, p.x >= 0.0);
    let bot_r = select(radii.w, radii.z, p.x >= 0.0);
    let r = min(select(bot_r, top_r, p.y < 0.0), min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

@fragment
fn fs_backdrop_composite(in: FullscreenOut) -> @location(0) vec4<f32> {
    let screen_pos = (in.uv * u_backdrop.target_size) / max(u_backdrop.scale_factor, 1.0);
    let local_pos = vec2<f32>(
        dot(u_backdrop.inv_xform_0.xy, screen_pos) + u_backdrop.inv_xform_1.x,
        dot(u_backdrop.inv_xform_0.zw, screen_pos) + u_backdrop.inv_xform_1.y,
    );

    let sd_shape = sd_rbox_bd(local_pos, u_backdrop.rect, u_backdrop.radii);
    let aa_shape = clamp(fwidth(sd_shape) * 0.75, 0.35 / max(u_backdrop.scale_factor, 1.0), 2.5);
    let cov = clamp(0.5 - sd_shape / aa_shape, 0.0, 1.0);

    let sd_clip = sd_rbox_bd(screen_pos, u_backdrop.clip_rect, u_backdrop.clip_radii);
    let aa_clip = clamp(fwidth(sd_clip) * 0.75, 0.35 / max(u_backdrop.scale_factor, 1.0), 2.0);
    let clip_cov = clamp(0.5 - sd_clip / aa_clip, 0.0, 1.0);

    let blur_sample = textureSample(u_blurred_tex, u_bd_sampler, in.uv);
    let alpha = cov * clip_cov;
    if (alpha <= 0.001) {
        discard;
    }
    let ba = max(blur_sample.a, 1e-4);
    let blur_rgb = blur_sample.rgb / ba;
    let mixed_rgb = mix(blur_rgb, u_backdrop.tint.rgb, u_backdrop.tint.a);
    let out_a = clamp(max(blur_sample.a, u_backdrop.tint.a), 0.0, 1.0) * alpha;
    return vec4<f32>(mixed_rgb * out_a, out_a);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalsUniform {
    target_size: [f32; 2],
    scale_factor: f32,
    surface_is_srgb: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PresentUniforms {
    surface_is_srgb: f32,
    opacity: f32,
    blend_mode: f32,
    pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniforms {
    texel_size: [f32; 2],
    offset: f32,
    pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackdropCompositeUniforms {
    target_size: [f32; 2],
    scale_factor: f32,
    pad0: f32,
    inv_xform_0: [f32; 4],
    inv_xform_1: [f32; 4],
    rect: [f32; 4],
    radii: [f32; 4],
    tint: [f32; 4],
    clip_rect: [f32; 4],
    clip_radii: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SdfQuadInstance {
    xform_0: [f32; 4],
    xform_1: [f32; 4],
    rect: [f32; 4],
    radii: [f32; 4],
    color0: [f32; 4],
    color1: [f32; 4],
    grad_pts: [f32; 4],
    params: [f32; 4],
    uv_rect: [f32; 4],
    clip_rect: [f32; 4],
    clip_radii: [f32; 4],
}

impl SdfQuadInstance {
    const ATTRS: [wgpu::VertexAttribute; 11] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32x4,
        7 => Float32x4,
        8 => Float32x4,
        9 => Float32x4,
        10 => Float32x4,
    ];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GlyphAtlasKey {
    font_id: u64,
    font_index: u32,
    glyph_id: u32,
    size_q: u16,
    sub_x: u8,
    coords_hash: u64,
}

#[derive(Debug, Clone, Copy)]
struct AtlasEntry {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    width: u32,
    height: u32,
    offset_x: i32,
    offset_y: i32,
}

#[derive(Debug, Clone)]
enum PathAtlasTiles {
    Single(AtlasEntry),
    Multiple(Vec<AtlasEntry>),
}

impl PathAtlasTiles {
    pub fn as_slice(&self) -> &[AtlasEntry] {
        match self {
            Self::Single(entry) => std::slice::from_ref(entry),
            Self::Multiple(vec) => vec.as_slice(),
        }
    }
}

struct CachedGpuTex {
    version: u64,
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

struct RenderTargetTex {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTargetTex {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            width: w,
            height: h,
            texture,
            view,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuadBatchKind {
    Sdf(Option<TextureId>),
    Hole,
}

struct QuadBatch {
    kind: QuadBatchKind,
    range: Range<u32>,
}

struct ActiveLayer {
    tex_idx: usize,
    bounds: Rect,
    opacity: f32,
    blend_mode: BlendMode,
    cleared: bool,
}

struct PooledUniformBuf {
    buf: wgpu::Buffer,
}

struct PooledInstanceBuf {
    capacity_bytes: usize,
    buf: wgpu::Buffer,
}

/// Pure render-pipeline WGSL GPU backend (`gpu-sdf`) compatible with WebGPU, WebGL2, GLES3,
/// Vulkan, Metal, and DirectX 12.
pub struct GpuSdfBackend {
    width: u32,
    height: u32,
    surface_is_srgb: bool,
    atlas_size: u32,

    globals_buf: wgpu::Buffer,
    linear_sampler: wgpu::Sampler,
    group0_bg: wgpu::BindGroup,
    image_bgl: wgpu::BindGroupLayout,
    default_image_bg: wgpu::BindGroup,
    sdf_pipeline: wgpu::RenderPipeline,
    hole_pipeline: wgpu::RenderPipeline,

    present_bgl: wgpu::BindGroupLayout,
    present_u_buf: wgpu::Buffer,
    present_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,

    blur_bgl: wgpu::BindGroupLayout,
    kawase_down_pipeline: wgpu::RenderPipeline,
    kawase_up_pipeline: wgpu::RenderPipeline,

    backdrop_bgl: wgpu::BindGroupLayout,
    backdrop_pipeline: wgpu::RenderPipeline,

    atlas_tex: wgpu::Texture,
    _atlas_view: wgpu::TextureView,
    atlas_alloc: AtlasAllocator,
    atlas_generation: u64,
    glyph_cache: HashMap<GlyphAtlasKey, Option<AtlasEntry>>,
    path_cache: HashMap<u64, Option<PathAtlasTiles>>,
    ramp_cache: HashMap<u64, AtlasEntry>,

    image_cache: HashMap<TextureId, CachedGpuTex>,
    _dummy_tex: wgpu::Texture,
    dummy_view: wgpu::TextureView,

    scene_tex: RenderTargetTex,
    snapshot_tex: Option<RenderTargetTex>,
    layer_pool: Vec<RenderTargetTex>,
    blur_half_a: Option<RenderTargetTex>,
    blur_half_b: Option<RenderTargetTex>,
    blur_quarter: Option<RenderTargetTex>,

    inst_pool: Vec<PooledInstanceBuf>,
    uniform_pool: Vec<PooledUniformBuf>,

    pending_instances: Vec<SdfQuadInstance>,
    pending_batches: Vec<QuadBatch>,
    layer_stack: Vec<ActiveLayer>,
    scene_cleared: bool,
}

impl GpuSdfBackend {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let surface_is_srgb = surface_format.is_srgb();
        let atlas_size = device.limits().max_texture_dimension_2d.clamp(1024, 4096);

        let globals_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("craftui-sdf-globals"),
            contents: bytemuck::bytes_of(&GlobalsUniform {
                target_size: [w as f32, h as f32],
                scale_factor: 1.0,
                surface_is_srgb: if surface_is_srgb { 1.0 } else { 0.0 },
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("craftui-sdf-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("craftui-sdf-atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("craftui-sdf-dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &dummy_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let group0_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("craftui-sdf-g0-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let group0_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("craftui-sdf-g0-bg"),
            layout: &group0_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
            ],
        });

        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("craftui-sdf-img-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let default_image_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("craftui-sdf-default-img-bg"),
            layout: &image_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&dummy_view),
            }],
        });

        let sdf_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("craftui-sdf-quad-wgsl"),
            source: wgpu::ShaderSource::Wgsl(SDF_QUAD_WGSL.into()),
        });
        let sdf_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("craftui-sdf-pl"),
            bind_group_layouts: &[Some(&group0_bgl), Some(&image_bgl)],
            immediate_size: 0,
        });

        let make_sdf_pipeline = |label: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&sdf_pl),
                vertex: wgpu::VertexState {
                    module: &sdf_module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(SdfQuadInstance::layout())],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &sdf_module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let sdf_pipeline = make_sdf_pipeline(
            "craftui-sdf-premul",
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let hole_pipeline = make_sdf_pipeline("craftui-sdf-hole", wgpu::BlendState::REPLACE);

        // Present & Blend pipelines
        let present_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("craftui-present-blend-wgsl"),
            source: wgpu::ShaderSource::Wgsl(PRESENT_BLEND_WGSL.into()),
        });
        let present_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("craftui-present-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let present_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("craftui-present-pl"),
            bind_group_layouts: &[Some(&present_bgl)],
            immediate_size: 0,
        });

        let make_fullscreen_pipeline =
            |label: &str,
             pl: &wgpu::PipelineLayout,
             module: &wgpu::ShaderModule,
             fs: &str,
             fmt: wgpu::TextureFormat,
             blend: wgpu::BlendState| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(pl),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("vs_full"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(fs),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: fmt,
                            blend: Some(blend),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };

        let present_u_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("craftui-present-uniform"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let present_pipeline = make_fullscreen_pipeline(
            "craftui-present",
            &present_pl,
            &present_module,
            "fs_present",
            surface_format,
            wgpu::BlendState::REPLACE,
        );
        let blend_pipeline = make_fullscreen_pipeline(
            "craftui-blend-layer",
            &present_pl,
            &present_module,
            "fs_blend_layer",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::BlendState::REPLACE,
        );

        // Kawase Blur pipelines
        let blur_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("craftui-kawase-wgsl"),
            source: wgpu::ShaderSource::Wgsl(BLUR_WGSL.into()),
        });
        let blur_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("craftui-blur-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blur_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("craftui-blur-pl"),
            bind_group_layouts: &[Some(&blur_bgl)],
            immediate_size: 0,
        });
        let kawase_down_pipeline = make_fullscreen_pipeline(
            "craftui-kawase-down",
            &blur_pl,
            &blur_module,
            "fs_kawase_down",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::BlendState::REPLACE,
        );
        let kawase_up_pipeline = make_fullscreen_pipeline(
            "craftui-kawase-up",
            &blur_pl,
            &blur_module,
            "fs_kawase_up",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::BlendState::REPLACE,
        );

        // Backdrop composite pipeline
        let backdrop_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("craftui-backdrop-wgsl"),
            source: wgpu::ShaderSource::Wgsl(BACKDROP_WGSL.into()),
        });
        let backdrop_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("craftui-backdrop-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let backdrop_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("craftui-backdrop-pl"),
            bind_group_layouts: &[Some(&backdrop_bgl)],
            immediate_size: 0,
        });
        let backdrop_pipeline = make_fullscreen_pipeline(
            "craftui-backdrop-comp",
            &backdrop_pl,
            &backdrop_module,
            "fs_backdrop_composite",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );

        let scene_tex = RenderTargetTex::new(device, "craftui-sdf-scene", w, h);

        Self {
            width: w,
            height: h,
            surface_is_srgb,
            atlas_size,
            globals_buf,
            linear_sampler,
            group0_bg,
            image_bgl,
            default_image_bg,
            sdf_pipeline,
            hole_pipeline,
            present_bgl,
            present_u_buf,
            present_pipeline,
            blend_pipeline,
            blur_bgl,
            kawase_down_pipeline,
            kawase_up_pipeline,
            backdrop_bgl,
            backdrop_pipeline,
            atlas_tex,
            _atlas_view: atlas_view,
            atlas_alloc: AtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32)),
            atlas_generation: 0,
            glyph_cache: HashMap::new(),
            path_cache: HashMap::new(),
            ramp_cache: HashMap::new(),
            image_cache: HashMap::new(),
            _dummy_tex: dummy_tex,
            dummy_view,
            scene_tex,
            snapshot_tex: None,
            layer_pool: Vec::new(),
            blur_half_a: None,
            blur_half_b: None,
            blur_quarter: None,
            inst_pool: Vec::new(),
            uniform_pool: Vec::new(),
            pending_instances: Vec::new(),
            pending_batches: Vec::new(),
            layer_stack: Vec::new(),
            scene_cleared: false,
        }
    }

    pub fn set_surface_format(&mut self, fmt: wgpu::TextureFormat) {
        self.surface_is_srgb = fmt.is_srgb();
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return;
        }
        self.width = w;
        self.height = h;
        self.scene_tex = RenderTargetTex::new(device, "craftui-sdf-scene", w, h);
        self.snapshot_tex = None;
        self.layer_pool.clear();
        self.blur_half_a = None;
        self.blur_half_b = None;
        self.blur_quarter = None;
    }

    pub fn scene_texture(&self) -> &wgpu::Texture {
        &self.scene_tex.texture
    }

    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_tex.view
    }

    fn ensure_uniform_buf<'a>(
        pool: &'a mut Vec<PooledUniformBuf>,
        device: &wgpu::Device,
        idx: usize,
    ) -> &'a wgpu::Buffer {
        while idx >= pool.len() {
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("craftui-sdf-uniform-scratch"),
                size: 256,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            pool.push(PooledUniformBuf { buf });
        }
        &pool[idx].buf
    }

    #[allow(clippy::too_many_arguments)]
    fn alloc_atlas_region(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        background: Color,
        w: u32,
        h: u32,
    ) -> Option<(u32, u32)> {
        let req_w = w.saturating_add(2);
        let req_h = h.saturating_add(2);
        if req_w > self.atlas_size || req_h > self.atlas_size {
            return None;
        }
        if let Some(alloc) = self.atlas_alloc.allocate(size2(req_w as i32, req_h as i32)) {
            return Some((
                alloc.rectangle.min.x as u32 + 1,
                alloc.rectangle.min.y as u32 + 1,
            ));
        }
        // Flush queued geometry that still references old atlas UVs before resetting atlas!
        self.flush_pending(device, queue, encoder, inst_cursor, background);
        let old_cmd = std::mem::replace(
            encoder,
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("craftui-render-encoder-reset"),
            }),
        );
        queue.submit(std::iter::once(old_cmd.finish()));
        self.atlas_alloc.clear();
        self.glyph_cache.clear();
        self.path_cache.clear();
        self.ramp_cache.clear();
        self.atlas_generation = self.atlas_generation.wrapping_add(1);
        self.atlas_alloc
            .allocate(size2(req_w as i32, req_h as i32))
            .map(|a| (a.rectangle.min.x as u32 + 1, a.rectangle.min.y as u32 + 1))
    }

    fn upload_rgba_to_atlas(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) {
        if x + w > self.atlas_size || y + h > self.atlas_size {
            return;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
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
    }

    #[allow(clippy::too_many_arguments)]
    fn get_or_upload_glyph(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        background: Color,
        font: &parley::FontData,
        glyph_id: u32,
        font_size_px: f32,
        sub_x_bin: u8,
        norm_coords: &[i16],
    ) -> Option<AtlasEntry> {
        let mut h = DefaultHasher::new();
        norm_coords.hash(&mut h);
        let key = GlyphAtlasKey {
            font_id: font.data.id(),
            font_index: font.index,
            glyph_id,
            size_q: (font_size_px * 4.0).round().clamp(1.0, 60000.0) as u16,
            sub_x: sub_x_bin,
            coords_hash: h.finish(),
        };
        if let Some(cached) = self.glyph_cache.get(&key) {
            return *cached;
        }
        let sub_x = (sub_x_bin as f32) * 0.25;
        let mask = rasterize_glyph(
            font.data.as_ref(),
            font.index,
            glyph_id,
            font_size_px,
            sub_x,
            0.0,
            norm_coords,
        );
        let Some(mask) = mask else {
            self.glyph_cache.insert(key, None);
            return None;
        };
        let (ax, ay) = self.alloc_atlas_region(
            device,
            queue,
            encoder,
            inst_cursor,
            background,
            mask.width,
            mask.height,
        )?;
        let mut rgba = vec![255u8; (mask.width * mask.height * 4) as usize];
        for (i, &cov) in mask.coverage.iter().enumerate() {
            rgba[i * 4 + 3] = cov;
        }
        self.upload_rgba_to_atlas(queue, ax, ay, mask.width, mask.height, &rgba);
        let inv = 1.0 / self.atlas_size as f32;
        let entry = AtlasEntry {
            u0: ax as f32 * inv,
            v0: ay as f32 * inv,
            u1: (ax + mask.width) as f32 * inv,
            v1: (ay + mask.height) as f32 * inv,
            width: mask.width,
            height: mask.height,
            offset_x: mask.offset_x,
            offset_y: mask.offset_y,
        };
        self.glyph_cache.insert(key, Some(entry));
        Some(entry)
    }

    #[allow(clippy::too_many_arguments)]
    fn get_or_upload_path(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        background: Color,
        path: &BezPath,
        stroke_width: Option<f32>,
        scale_factor: f32,
    ) -> Option<PathAtlasTiles> {
        let mut hasher = DefaultHasher::new();
        for el in path.elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    0u8.hash(&mut hasher);
                    (p.x as f32).to_bits().hash(&mut hasher);
                    (p.y as f32).to_bits().hash(&mut hasher);
                }
                PathEl::LineTo(p) => {
                    1u8.hash(&mut hasher);
                    (p.x as f32).to_bits().hash(&mut hasher);
                    (p.y as f32).to_bits().hash(&mut hasher);
                }
                PathEl::QuadTo(c, p) => {
                    2u8.hash(&mut hasher);
                    (c.x as f32).to_bits().hash(&mut hasher);
                    (c.y as f32).to_bits().hash(&mut hasher);
                    (p.x as f32).to_bits().hash(&mut hasher);
                    (p.y as f32).to_bits().hash(&mut hasher);
                }
                PathEl::CurveTo(c0, c1, p) => {
                    3u8.hash(&mut hasher);
                    (c0.x as f32).to_bits().hash(&mut hasher);
                    (c0.y as f32).to_bits().hash(&mut hasher);
                    (c1.x as f32).to_bits().hash(&mut hasher);
                    (c1.y as f32).to_bits().hash(&mut hasher);
                    (p.x as f32).to_bits().hash(&mut hasher);
                    (p.y as f32).to_bits().hash(&mut hasher);
                }
                PathEl::ClosePath => 4u8.hash(&mut hasher),
            }
        }
        stroke_width.map(f32::to_bits).hash(&mut hasher);
        ((scale_factor * 100.0).round() as u32).hash(&mut hasher);
        let key = hasher.finish();
        if let Some(cached) = self.path_cache.get(&key) {
            return cached.clone();
        }
        let expanded = if let Some(sw) = stroke_width {
            kurbo_stroke(
                path.elements().iter().copied(),
                &Stroke::new(sw as f64),
                &StrokeOpts::default(),
                0.25,
            )
        } else {
            path.clone()
        };
        let mask = rasterize_bez_path(&expanded, scale_factor);
        let Some(mask) = mask else {
            self.path_cache.insert(key, None);
            return None;
        };
        let max_tile = self.atlas_size.saturating_sub(2);
        if mask.width <= max_tile && mask.height <= max_tile {
            let (ax, ay) = self.alloc_atlas_region(
                device,
                queue,
                encoder,
                inst_cursor,
                background,
                mask.width,
                mask.height,
            )?;
            let mut rgba = vec![255u8; (mask.width * mask.height * 4) as usize];
            for (i, &cov) in mask.coverage.iter().enumerate() {
                rgba[i * 4 + 3] = cov;
            }
            self.upload_rgba_to_atlas(queue, ax, ay, mask.width, mask.height, &rgba);
            let inv = 1.0 / self.atlas_size as f32;
            let entry = AtlasEntry {
                u0: ax as f32 * inv,
                v0: ay as f32 * inv,
                u1: (ax + mask.width) as f32 * inv,
                v1: (ay + mask.height) as f32 * inv,
                width: mask.width,
                height: mask.height,
                offset_x: mask.offset_x,
                offset_y: mask.offset_y,
            };
            let tiles = PathAtlasTiles::Single(entry);
            self.path_cache.insert(key, Some(tiles.clone()));
            return Some(tiles);
        }

        let tile_dim = max_tile.clamp(64, 1024);
        let mut tiles = Vec::new();
        let inv = 1.0 / self.atlas_size as f32;

        let mut start_gen = self.atlas_generation;
        let mut attempt = 0;
        'retry: loop {
            tiles.clear();
            for tile_y in (0..mask.height).step_by(tile_dim as usize) {
                let cur_h = (mask.height - tile_y).min(tile_dim);
                for tile_x in (0..mask.width).step_by(tile_dim as usize) {
                    let cur_w = (mask.width - tile_x).min(tile_dim);

                    let mut has_coverage = false;
                    let mut rgba = vec![255u8; (cur_w * cur_h * 4) as usize];
                    for row in 0..cur_h {
                        let src_row_start = ((tile_y + row) * mask.width + tile_x) as usize;
                        let dst_row_start = (row * cur_w) as usize;
                        for col in 0..cur_w {
                            let cov = mask.coverage[src_row_start + col as usize];
                            if cov > 0 {
                                has_coverage = true;
                            }
                            rgba[(dst_row_start + col as usize) * 4 + 3] = cov;
                        }
                    }

                    if !has_coverage {
                        continue;
                    }

                    let Some((ax, ay)) = self.alloc_atlas_region(
                        device,
                        queue,
                        encoder,
                        inst_cursor,
                        background,
                        cur_w,
                        cur_h,
                    ) else {
                        self.path_cache.insert(key, None);
                        return None;
                    };

                    if self.atlas_generation != start_gen {
                        start_gen = self.atlas_generation;
                        attempt += 1;
                        if attempt < 2 {
                            self.atlas_alloc.clear();
                            continue 'retry;
                        } else {
                            self.path_cache.insert(key, None);
                            return None;
                        }
                    }

                    self.upload_rgba_to_atlas(queue, ax, ay, cur_w, cur_h, &rgba);

                    tiles.push(AtlasEntry {
                        u0: (ax as f32 + 0.5) * inv,
                        v0: (ay as f32 + 0.5) * inv,
                        u1: ((ax + cur_w) as f32 - 0.5).max(ax as f32 + 0.5) * inv,
                        v1: ((ay + cur_h) as f32 - 0.5).max(ay as f32 + 0.5) * inv,
                        width: cur_w,
                        height: cur_h,
                        offset_x: mask.offset_x + tile_x as i32,
                        offset_y: mask.offset_y + tile_y as i32,
                    });
                }
            }
            break;
        }

        if self.atlas_generation != start_gen || tiles.is_empty() {
            self.path_cache.insert(key, None);
            return None;
        }

        let res = PathAtlasTiles::Multiple(tiles);
        self.path_cache.insert(key, Some(res.clone()));
        Some(res)
    }

    #[allow(clippy::too_many_arguments)]
    fn get_or_upload_ramp(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        background: Color,
        stops: &[ColorStop],
    ) -> Option<AtlasEntry> {
        let mut hasher = DefaultHasher::new();
        for s in stops {
            s.offset.to_bits().hash(&mut hasher);
            s.color.hash(&mut hasher);
        }
        let key = hasher.finish();
        if let Some(&e) = self.ramp_cache.get(&key) {
            return Some(e);
        }
        const RAMP_W: u32 = 64;
        let (ax, ay) =
            self.alloc_atlas_region(device, queue, encoder, inst_cursor, background, RAMP_W, 1)?;
        let mut sorted: Vec<ColorStop> = stops.to_vec();
        sorted.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        let mut rgba = vec![0u8; (RAMP_W * 4) as usize];
        for i in 0..RAMP_W {
            let t = i as f32 / (RAMP_W - 1) as f32;
            let c = sample_stops(&sorted, t).to_rgba_f32();
            let a = c[3];
            rgba[i as usize * 4] = ((c[0] * a * 255.0).round() as u32).min(255) as u8;
            rgba[i as usize * 4 + 1] = ((c[1] * a * 255.0).round() as u32).min(255) as u8;
            rgba[i as usize * 4 + 2] = ((c[2] * a * 255.0).round() as u32).min(255) as u8;
            rgba[i as usize * 4 + 3] = ((a * 255.0).round() as u32).min(255) as u8;
        }
        self.upload_rgba_to_atlas(queue, ax, ay, RAMP_W, 1, &rgba);
        let inv = 1.0 / self.atlas_size as f32;
        let entry = AtlasEntry {
            u0: (ax as f32 + 0.5) * inv,
            v0: (ay as f32 + 0.5) * inv,
            u1: (ax as f32 + RAMP_W as f32 - 0.5) * inv,
            v1: (ay as f32 + 0.5) * inv,
            width: RAMP_W,
            height: 1,
            offset_x: 0,
            offset_y: 0,
        };
        self.ramp_cache.insert(key, entry);
        Some(entry)
    }

    fn ensure_image_uploaded(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: TextureId,
        textures: &TextureRegistry,
    ) -> Option<(u32, u32)> {
        let handle = textures.get(id)?;
        let ver = handle.version();
        if let Some(existing) = self.image_cache.get(&id)
            && existing.version == ver
        {
            return Some((existing.width, existing.height));
        }
        let (w, h, ver, rgba) = handle.snapshot();
        if w == 0 || h == 0 {
            return None;
        }
        // Convert straight RGBA8 to premultiplied RGBA8 for filtered sampling without halos
        let mut premul = rgba;
        for px in premul.as_chunks_mut::<4>().0 {
            let a = px[3] as u16;
            px[0] = ((px[0] as u16 * a) / 255) as u8;
            px[1] = ((px[1] as u16 * a) / 255) as u8;
            px[2] = ((px[2] as u16 * a) / 255) as u8;
        }

        let reuse = self
            .image_cache
            .get(&id)
            .is_some_and(|e| e.width == w && e.height == h);
        if reuse {
            if let Some(entry) = self.image_cache.get_mut(&id) {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &entry.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &premul,
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
                entry.version = ver;
            }
            return Some((w, h));
        }

        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("craftui-sdf-image"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &premul,
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
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("craftui-sdf-image-bg"),
            layout: &self.image_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        self.image_cache.insert(
            id,
            CachedGpuTex {
                version: ver,
                width: w,
                height: h,
                texture: tex,
                _view: view,
                bind_group,
            },
        );
        Some((w, h))
    }

    fn push_quad(&mut self, kind: QuadBatchKind, inst: SdfQuadInstance) {
        let idx = self.pending_instances.len() as u32;
        self.pending_instances.push(inst);
        if let Some(last) = self.pending_batches.last_mut()
            && last.kind == kind
        {
            last.range.end = idx + 1;
        } else {
            self.pending_batches.push(QuadBatch {
                kind,
                range: idx..(idx + 1),
            });
        }
    }

    fn flush_pending(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        background: Color,
    ) {
        let need_clear_only = if let Some(top) = self.layer_stack.last_mut() {
            let need = !top.cleared;
            top.cleared = true;
            need
        } else {
            let need = !self.scene_cleared;
            self.scene_cleared = true;
            need
        };

        if self.pending_instances.is_empty() && !need_clear_only {
            self.pending_batches.clear();
            return;
        }

        let (buf_idx_opt, byte_len) = if !self.pending_instances.is_empty() {
            let bytes = bytemuck::cast_slice(&self.pending_instances);
            let slot = *inst_cursor;
            *inst_cursor += 1;
            if slot >= self.inst_pool.len() || self.inst_pool[slot].capacity_bytes < bytes.len() {
                let cap = bytes.len().max(4096).next_power_of_two();
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("craftui-sdf-inst-buf"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let item = PooledInstanceBuf {
                    capacity_bytes: cap,
                    buf,
                };
                if slot < self.inst_pool.len() {
                    self.inst_pool[slot] = item;
                } else {
                    self.inst_pool.push(item);
                }
            }
            queue.write_buffer(&self.inst_pool[slot].buf, 0, bytes);
            (Some(slot), bytes.len())
        } else {
            (None, 0)
        };

        let target_view = if let Some(top) = self.layer_stack.last() {
            &self.layer_pool[top.tex_idx].view
        } else {
            &self.scene_tex.view
        };

        let load_op = if need_clear_only {
            let clear = if self.layer_stack.is_empty() {
                let [r, g, b, a] = background.to_rgba_f32();
                wgpu::Color {
                    r: (r * a) as f64,
                    g: (g * a) as f64,
                    b: (b * a) as f64,
                    a: a as f64,
                }
            } else {
                wgpu::Color::TRANSPARENT
            };
            wgpu::LoadOp::Clear(clear)
        } else {
            wgpu::LoadOp::Load
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("craftui-sdf-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(slot) = buf_idx_opt {
                pass.set_bind_group(0, &self.group0_bg, &[]);
                pass.set_vertex_buffer(0, self.inst_pool[slot].buf.slice(0..byte_len as u64));

                let mut last_pipeline_is_hole = None;
                for batch in &self.pending_batches {
                    let is_hole = matches!(batch.kind, QuadBatchKind::Hole);
                    if last_pipeline_is_hole != Some(is_hole) {
                        pass.set_pipeline(if is_hole {
                            &self.hole_pipeline
                        } else {
                            &self.sdf_pipeline
                        });
                        last_pipeline_is_hole = Some(is_hole);
                    }
                    let img_bg = match batch.kind {
                        QuadBatchKind::Sdf(Some(tex_id)) => self
                            .image_cache
                            .get(&tex_id)
                            .map(|c| &c.bind_group)
                            .unwrap_or(&self.default_image_bg),
                        _ => &self.default_image_bg,
                    };
                    pass.set_bind_group(1, img_bg, &[]);
                    pass.draw(0..4, batch.range.clone());
                }
            }
        }

        self.pending_instances.clear();
        self.pending_batches.clear();
    }

    fn brush_to_shader_params(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        background: Color,
        brush: &Brush,
    ) -> ([f32; 4], [f32; 4], [f32; 4], f32) {
        match brush {
            Brush::Solid(c) => (c.to_rgba_f32(), [0.0; 4], [0.0; 4], 0.0),
            Brush::LinearGradient { start, end, stops } => {
                if stops.is_empty() {
                    ([0.0; 4], [0.0; 4], [0.0; 4], 0.0)
                } else if stops.len() == 1 {
                    (stops[0].color.to_rgba_f32(), [0.0; 4], [0.0; 4], 0.0)
                } else if stops.len() == 2 && stops[0].offset <= 0.001 && stops[1].offset >= 0.999 {
                    (
                        stops[0].color.to_rgba_f32(),
                        stops[1].color.to_rgba_f32(),
                        [start.x, start.y, end.x, end.y],
                        1.0,
                    )
                } else if let Some(ramp) =
                    self.get_or_upload_ramp(device, queue, encoder, inst_cursor, background, stops)
                {
                    (
                        [1.0; 4],
                        [ramp.u0, ramp.u1, ramp.v0, 0.0],
                        [start.x, start.y, end.x, end.y],
                        3.0,
                    )
                } else {
                    (
                        stops[0].color.to_rgba_f32(),
                        stops.last().unwrap().color.to_rgba_f32(),
                        [start.x, start.y, end.x, end.y],
                        1.0,
                    )
                }
            }
            Brush::RadialGradient {
                center,
                radius,
                stops,
            } => {
                if stops.is_empty() {
                    ([0.0; 4], [0.0; 4], [0.0; 4], 0.0)
                } else if stops.len() == 1 {
                    (stops[0].color.to_rgba_f32(), [0.0; 4], [0.0; 4], 0.0)
                } else if stops.len() == 2 && stops[0].offset <= 0.001 && stops[1].offset >= 0.999 {
                    (
                        stops[0].color.to_rgba_f32(),
                        stops[1].color.to_rgba_f32(),
                        [center.x, center.y, *radius, 0.0],
                        2.0,
                    )
                } else if let Some(ramp) =
                    self.get_or_upload_ramp(device, queue, encoder, inst_cursor, background, stops)
                {
                    (
                        [1.0; 4],
                        [ramp.u0, ramp.u1, ramp.v0, 0.0],
                        [center.x, center.y, *radius, 0.0],
                        4.0,
                    )
                } else {
                    (
                        stops[0].color.to_rgba_f32(),
                        stops.last().unwrap().color.to_rgba_f32(),
                        [center.x, center.y, *radius, 0.0],
                        2.0,
                    )
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_backdrop_blur(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        uniform_cursor: &mut usize,
        background: Color,
        rect: Rect,
        radius: CornerRadii,
        blur_radius: f32,
        tint: Color,
        scale_factor: f32,
        xform: Transform2D,
        clip_rect: [f32; 4],
        clip_radii: [f32; 4],
    ) {
        self.flush_pending(device, queue, encoder, inst_cursor, background);

        let hw = (self.width / 2).max(1);
        let hh = (self.height / 2).max(1);
        let qw = (self.width / 4).max(1);
        let qh = (self.height / 4).max(1);

        if self
            .blur_half_a
            .as_ref()
            .is_none_or(|t| t.width != hw || t.height != hh)
        {
            self.blur_half_a = Some(RenderTargetTex::new(device, "craftui-blur-ha", hw, hh));
            self.blur_half_b = Some(RenderTargetTex::new(device, "craftui-blur-hb", hw, hh));
            self.blur_quarter = Some(RenderTargetTex::new(device, "craftui-blur-q", qw, qh));
        }

        let src_tex_idx = self.layer_stack.last().map(|l| l.tex_idx);
        let src_view = match src_tex_idx {
            Some(idx) => &self.layer_pool[idx].view,
            None => &self.scene_tex.view,
        };

        let offset_base = (blur_radius * scale_factor * 0.25).clamp(0.5, 4.0);

        // Pass 1: src -> half_a (downsample)
        let u1_idx = *uniform_cursor;
        *uniform_cursor += 1;
        let u1_buf = Self::ensure_uniform_buf(&mut self.uniform_pool, device, u1_idx);
        queue.write_buffer(
            u1_buf,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                texel_size: [1.0 / self.width as f32, 1.0 / self.height as f32],
                offset: offset_base,
                pad: 0.0,
            }),
        );
        let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blur_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: u1_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });
        {
            let ha_view = &self.blur_half_a.as_ref().unwrap().view;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("craftui-kawase-down-1"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: ha_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.kawase_down_pipeline);
            pass.set_bind_group(0, &bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 2: half_a -> quarter (downsample)
        let u2_idx = *uniform_cursor;
        *uniform_cursor += 1;
        let u2_buf = Self::ensure_uniform_buf(&mut self.uniform_pool, device, u2_idx);
        queue.write_buffer(
            u2_buf,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                texel_size: [1.0 / hw as f32, 1.0 / hh as f32],
                offset: offset_base * 1.25,
                pad: 0.0,
            }),
        );
        let ha_view = &self.blur_half_a.as_ref().unwrap().view;
        let bg2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blur_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: u2_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(ha_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });
        {
            let q_view = &self.blur_quarter.as_ref().unwrap().view;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("craftui-kawase-down-2"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: q_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.kawase_down_pipeline);
            pass.set_bind_group(0, &bg2, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 3: quarter -> half_b (upsample)
        let u3_idx = *uniform_cursor;
        *uniform_cursor += 1;
        let u3_buf = Self::ensure_uniform_buf(&mut self.uniform_pool, device, u3_idx);
        queue.write_buffer(
            u3_buf,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                texel_size: [1.0 / qw as f32, 1.0 / qh as f32],
                offset: offset_base,
                pad: 0.0,
            }),
        );
        let q_view = &self.blur_quarter.as_ref().unwrap().view;
        let bg3 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blur_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: u3_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(q_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });
        {
            let hb_view = &self.blur_half_b.as_ref().unwrap().view;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("craftui-kawase-up-1"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: hb_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.kawase_up_pipeline);
            pass.set_bind_group(0, &bg3, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 4: Composite blurred texture + tint back into active render target with SDF clip
        let inv = xform.inverse().unwrap_or(Transform2D::IDENTITY).matrix;
        let uc_idx = *uniform_cursor;
        *uniform_cursor += 1;
        let uc_buf = Self::ensure_uniform_buf(&mut self.uniform_pool, device, uc_idx);
        queue.write_buffer(
            uc_buf,
            0,
            bytemuck::bytes_of(&BackdropCompositeUniforms {
                target_size: [self.width as f32, self.height as f32],
                scale_factor,
                pad0: 0.0,
                inv_xform_0: [inv[0], inv[2], inv[1], inv[3]],
                inv_xform_1: [inv[4], inv[5], 0.0, 0.0],
                rect: [rect.x, rect.y, rect.width, rect.height],
                radii: [
                    radius.top_left,
                    radius.top_right,
                    radius.bottom_right,
                    radius.bottom_left,
                ],
                tint: tint.to_rgba_f32(),
                clip_rect,
                clip_radii,
            }),
        );
        let hb_view = &self.blur_half_b.as_ref().unwrap().view;
        let bg_comp = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.backdrop_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uc_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(hb_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });
        let dst_view = match src_tex_idx {
            Some(idx) => &self.layer_pool[idx].view,
            None => &self.scene_tex.view,
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("craftui-backdrop-comp-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.backdrop_pipeline);
            pass.set_bind_group(0, &bg_comp, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pop_active_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inst_cursor: &mut usize,
        uniform_cursor: &mut usize,
        background: Color,
    ) {
        self.flush_pending(device, queue, encoder, inst_cursor, background);
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        if self.snapshot_tex.is_none() {
            self.snapshot_tex = Some(RenderTargetTex::new(
                device,
                "craftui-sdf-snapshot",
                self.width,
                self.height,
            ));
        }

        let parent_tex = match self.layer_stack.last() {
            Some(p) => &self.layer_pool[p.tex_idx].texture,
            None => &self.scene_tex.texture,
        };
        let snap = self.snapshot_tex.as_ref().unwrap();
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: parent_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &snap.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        let u_idx = *uniform_cursor;
        *uniform_cursor += 1;
        let mode_id = match layer.blend_mode {
            BlendMode::Normal => 0.0,
            BlendMode::Multiply => 1.0,
            BlendMode::Screen => 2.0,
            BlendMode::Overlay => 3.0,
            BlendMode::Darken => 4.0,
            BlendMode::Lighten => 5.0,
            BlendMode::Plus => 6.0,
        };
        let u_buf = Self::ensure_uniform_buf(&mut self.uniform_pool, device, u_idx);
        queue.write_buffer(
            u_buf,
            0,
            bytemuck::bytes_of(&PresentUniforms {
                surface_is_srgb: 0.0,
                opacity: layer.opacity.clamp(0.0, 1.0),
                blend_mode: mode_id,
                pad: 0.0,
            }),
        );
        let src_view = &self.layer_pool[layer.tex_idx].view;
        let snap_view = &self.snapshot_tex.as_ref().unwrap().view;
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.present_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: u_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(snap_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });

        let dst_view = match self.layer_stack.last() {
            Some(p) => &self.layer_pool[p.tex_idx].view,
            None => &self.scene_tex.view,
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("craftui-blend-layer-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.blend_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        // Tight scissor rect when bounds are finite and non-empty
        if layer.bounds.width > 0.0
            && layer.bounds.height > 0.0
            && layer.bounds.width < self.width as f32 * 2.0
        {
            let _ = layer.bounds;
        }
        pass.draw(0..3, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_to_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        paint_scene: &PaintScene,
        scale_factor: f32,
        background: Color,
        textures: &TextureRegistry,
        text_engine: &mut TextEngine,
    ) {
        self.pending_instances.clear();
        self.pending_batches.clear();
        self.layer_stack.clear();
        self.scene_cleared = false;

        queue.write_buffer(
            &self.globals_buf,
            0,
            bytemuck::bytes_of(&GlobalsUniform {
                target_size: [self.width as f32, self.height as f32],
                scale_factor: scale_factor.max(1.0),
                surface_is_srgb: if self.surface_is_srgb { 1.0 } else { 0.0 },
            }),
        );

        let mut inst_cursor = 0usize;
        let mut uniform_cursor = 0usize;
        let mut xform_stack = vec![Transform2D::IDENTITY];
        let mut clip_stack: Vec<(Rect, CornerRadii)> =
            vec![(Rect::new(-1e5, -1e5, 2e5, 2e5), CornerRadii::ZERO)];

        for cmd in &paint_scene.commands {
            let cur_xform = *xform_stack.last().unwrap();
            let xm = cur_xform.matrix;
            let xf0 = [xm[0], xm[2], xm[1], xm[3]];
            let (cur_clip, cur_clip_r) = *clip_stack.last().unwrap();
            let clip_rect = [cur_clip.x, cur_clip.y, cur_clip.width, cur_clip.height];
            let clip_radii = [
                cur_clip_r.top_left,
                cur_clip_r.top_right,
                cur_clip_r.bottom_right,
                cur_clip_r.bottom_left,
            ];

            match cmd {
                PaintCommand::FillRect {
                    rect,
                    brush,
                    radius,
                } => {
                    let (c0, c1, gpts, mode) = self.brush_to_shader_params(
                        device,
                        queue,
                        encoder,
                        &mut inst_cursor,
                        background,
                        brush,
                    );
                    self.push_quad(
                        QuadBatchKind::Sdf(None),
                        SdfQuadInstance {
                            xform_0: xf0,
                            xform_1: [xm[4], xm[5], 0.0, 0.0],
                            rect: [rect.x, rect.y, rect.width, rect.height],
                            radii: [
                                radius.top_left,
                                radius.top_right,
                                radius.bottom_right,
                                radius.bottom_left,
                            ],
                            color0: c0,
                            color1: c1,
                            grad_pts: gpts,
                            params: [0.0, mode, 1.0, 1.0],
                            uv_rect: [0.0, 0.0, 1.0, 1.0],
                            clip_rect,
                            clip_radii,
                        },
                    );
                }
                PaintCommand::StrokeRect {
                    rect,
                    color,
                    width,
                    radius,
                } => {
                    let pad = (*width * 0.5 + 1.5).max(1.5);
                    self.push_quad(
                        QuadBatchKind::Sdf(None),
                        SdfQuadInstance {
                            xform_0: xf0,
                            xform_1: [xm[4], xm[5], 0.0, 0.0],
                            rect: [rect.x, rect.y, rect.width, rect.height],
                            radii: [
                                radius.top_left,
                                radius.top_right,
                                radius.bottom_right,
                                radius.bottom_left,
                            ],
                            color0: color.to_rgba_f32(),
                            color1: [0.0; 4],
                            grad_pts: [0.0; 4],
                            params: [*width, 0.0, pad, 1.0],
                            uv_rect: [0.0, 0.0, 1.0, 1.0],
                            clip_rect,
                            clip_radii,
                        },
                    );
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
                    if *inset {
                        self.push_quad(
                            QuadBatchKind::Sdf(None),
                            SdfQuadInstance {
                                xform_0: xf0,
                                xform_1: [xm[4], xm[5], 1.0, 0.0],
                                rect: [rect.x, rect.y, rect.width, rect.height],
                                radii: [
                                    radius.top_left,
                                    radius.top_right,
                                    radius.bottom_right,
                                    radius.bottom_left,
                                ],
                                color0: color.to_rgba_f32(),
                                color1: [0.0; 4],
                                grad_pts: [*std_dev, *spread, *offset_x, *offset_y],
                                params: [0.0, 1.0, 1.0, 1.0],
                                uv_rect: [0.0, 0.0, 1.0, 1.0],
                                clip_rect,
                                clip_radii,
                            },
                        );
                    } else {
                        let sx = rect.x + offset_x - spread;
                        let sy = rect.y + offset_y - spread;
                        let sw = (rect.width + 2.0 * spread).max(0.0);
                        let sh = (rect.height + 2.0 * spread).max(0.0);
                        let adj = |r: f32| {
                            if r > 0.0 || *spread > 0.0 {
                                (r + spread).max(0.0)
                            } else {
                                0.0
                            }
                        };
                        let expand = 3.0 * std_dev.max(0.0) + 1.5;
                        self.push_quad(
                            QuadBatchKind::Sdf(None),
                            SdfQuadInstance {
                                xform_0: xf0,
                                xform_1: [xm[4], xm[5], 1.0, 0.0],
                                rect: [sx, sy, sw, sh],
                                radii: [
                                    adj(radius.top_left),
                                    adj(radius.top_right),
                                    adj(radius.bottom_right),
                                    adj(radius.bottom_left),
                                ],
                                color0: color.to_rgba_f32(),
                                color1: [0.0; 4],
                                grad_pts: [*std_dev, *spread, 0.0, 0.0],
                                params: [0.0, 0.0, expand, 1.0],
                                uv_rect: [0.0, 0.0, 1.0, 1.0],
                                clip_rect,
                                clip_radii,
                            },
                        );
                    }
                }
                PaintCommand::BackdropBlur {
                    rect,
                    radius,
                    blur_radius,
                    tint,
                } => {
                    self.run_backdrop_blur(
                        device,
                        queue,
                        encoder,
                        &mut inst_cursor,
                        &mut uniform_cursor,
                        background,
                        *rect,
                        *radius,
                        *blur_radius,
                        *tint,
                        scale_factor.max(1.0),
                        cur_xform,
                        clip_rect,
                        clip_radii,
                    );
                }
                PaintCommand::FillPath { path, brush } => {
                    if let Some(tiles) = self.get_or_upload_path(
                        device,
                        queue,
                        encoder,
                        &mut inst_cursor,
                        background,
                        path,
                        None,
                        scale_factor.max(1.0),
                    ) {
                        let (c0, c1, gpts, mode) = self.brush_to_shader_params(
                            device,
                            queue,
                            encoder,
                            &mut inst_cursor,
                            background,
                            brush,
                        );
                        let inv_s = 1.0 / scale_factor.max(1.0);
                        for entry in tiles.as_slice() {
                            self.push_quad(
                                QuadBatchKind::Sdf(None),
                                SdfQuadInstance {
                                    xform_0: xf0,
                                    xform_1: [xm[4], xm[5], 2.0, 0.0],
                                    rect: [
                                        entry.offset_x as f32 * inv_s,
                                        entry.offset_y as f32 * inv_s,
                                        entry.width as f32 * inv_s,
                                        entry.height as f32 * inv_s,
                                    ],
                                    radii: [0.0; 4],
                                    color0: c0,
                                    color1: c1,
                                    grad_pts: gpts,
                                    params: [0.0, mode, 0.0, 1.0],
                                    uv_rect: [entry.u0, entry.v0, entry.u1, entry.v1],
                                    clip_rect,
                                    clip_radii,
                                },
                            );
                        }
                    }
                }
                PaintCommand::StrokePath { path, color, width } => {
                    if let Some(tiles) = self.get_or_upload_path(
                        device,
                        queue,
                        encoder,
                        &mut inst_cursor,
                        background,
                        path,
                        Some(*width),
                        scale_factor.max(1.0),
                    ) {
                        let inv_s = 1.0 / scale_factor.max(1.0);
                        for entry in tiles.as_slice() {
                            self.push_quad(
                                QuadBatchKind::Sdf(None),
                                SdfQuadInstance {
                                    xform_0: xf0,
                                    xform_1: [xm[4], xm[5], 2.0, 0.0],
                                    rect: [
                                        entry.offset_x as f32 * inv_s,
                                        entry.offset_y as f32 * inv_s,
                                        entry.width as f32 * inv_s,
                                        entry.height as f32 * inv_s,
                                    ],
                                    radii: [0.0; 4],
                                    color0: color.to_rgba_f32(),
                                    color1: [0.0; 4],
                                    grad_pts: [0.0; 4],
                                    params: [0.0, 0.0, 0.0, 1.0],
                                    uv_rect: [entry.u0, entry.v0, entry.u1, entry.v1],
                                    clip_rect,
                                    clip_radii,
                                },
                            );
                        }
                    }
                }
                PaintCommand::DrawImage {
                    rect,
                    texture,
                    fit,
                    radius,
                    opacity,
                } => {
                    if let Some((iw, ih)) =
                        self.ensure_image_uploaded(device, queue, *texture, textures)
                    {
                        let (draw_rect, _) = compute_image_transform(*rect, iw, ih, *fit);
                        let dw = draw_rect.width.max(1e-4);
                        let dh = draw_rect.height.max(1e-4);
                        let u0 = (rect.x - draw_rect.x) / dw;
                        let v0 = (rect.y - draw_rect.y) / dh;
                        let u1 = (rect.max_x() - draw_rect.x) / dw;
                        let v1 = (rect.max_y() - draw_rect.y) / dh;
                        self.push_quad(
                            QuadBatchKind::Sdf(Some(*texture)),
                            SdfQuadInstance {
                                xform_0: xf0,
                                xform_1: [xm[4], xm[5], 3.0, 0.0],
                                rect: [rect.x, rect.y, rect.width, rect.height],
                                radii: [
                                    radius.top_left,
                                    radius.top_right,
                                    radius.bottom_right,
                                    radius.bottom_left,
                                ],
                                color0: [1.0; 4],
                                color1: [0.0; 4],
                                grad_pts: [0.0; 4],
                                params: [0.0, 0.0, 1.0, *opacity],
                                uv_rect: [u0, v0, u1, v1],
                                clip_rect,
                                clip_radii,
                            },
                        );
                    }
                }
                PaintCommand::DrawText {
                    origin,
                    text,
                    style,
                    max_width,
                } => {
                    if text.is_empty() {
                        continue;
                    }
                    let layout = text_engine.build_layout_cached(text, style, *max_width);
                    let color = style.color.to_rgba_f32();
                    let sf = scale_factor.max(1.0);
                    let inv_s = 1.0 / sf;
                    let mut coords_buf: Vec<i16> = Vec::new();
                    for line in layout.lines() {
                        for item in line.items() {
                            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                                let run = glyph_run.run();
                                let font = run.font();
                                let font_size_phys = run.font_size() * sf;
                                let norm = run.normalized_coords();
                                coords_buf.clear();
                                coords_buf.extend(norm.iter().map(|c| c.to_bits()));

                                for g in glyph_run.positioned_glyphs() {
                                    let gx_phys = (origin.x + g.x) * sf;
                                    let gy_phys = (origin.y + g.y) * sf;
                                    let gx_floor = gx_phys.floor();
                                    let gy_round = gy_phys.round();
                                    let frac = (gx_phys - gx_floor).clamp(0.0, 0.99);
                                    let sub_bin = ((frac * 4.0).floor() as u8).min(3);
                                    if let Some(entry) = self.get_or_upload_glyph(
                                        device,
                                        queue,
                                        encoder,
                                        &mut inst_cursor,
                                        background,
                                        font,
                                        g.id,
                                        font_size_phys,
                                        sub_bin,
                                        &coords_buf,
                                    ) {
                                        let lx = (gx_floor + entry.offset_x as f32) * inv_s;
                                        let ly = (gy_round + entry.offset_y as f32) * inv_s;
                                        let lw = entry.width as f32 * inv_s;
                                        let lh = entry.height as f32 * inv_s;
                                        self.push_quad(
                                            QuadBatchKind::Sdf(None),
                                            SdfQuadInstance {
                                                xform_0: xf0,
                                                xform_1: [xm[4], xm[5], 2.0, 0.0],
                                                rect: [lx, ly, lw, lh],
                                                radii: [0.0; 4],
                                                color0: color,
                                                color1: [0.0; 4],
                                                grad_pts: [0.0; 4],
                                                params: [0.0, 0.0, 0.0, 1.0],
                                                uv_rect: [entry.u0, entry.v0, entry.u1, entry.v1],
                                                clip_rect,
                                                clip_radii,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                PaintCommand::PushClip { rect, radius } => {
                    let t_rect = cur_xform.transform_rect_bounds(*rect);
                    let x0 = cur_clip.x.max(t_rect.x);
                    let y0 = cur_clip.y.max(t_rect.y);
                    let x1 = cur_clip.max_x().min(t_rect.max_x());
                    let y1 = cur_clip.max_y().min(t_rect.max_y());
                    let inter = Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0));
                    let approx_s = ((xm[0] * xm[0] + xm[1] * xm[1]).sqrt()).max(0.01);
                    let r_scaled = if !radius.is_zero() {
                        CornerRadii::new(
                            radius.top_left * approx_s,
                            radius.top_right * approx_s,
                            radius.bottom_right * approx_s,
                            radius.bottom_left * approx_s,
                        )
                    } else if cur_clip == inter {
                        cur_clip_r
                    } else {
                        CornerRadii::ZERO
                    };
                    clip_stack.push((inter, r_scaled));
                }
                PaintCommand::PopClip => {
                    if clip_stack.len() > 1 {
                        clip_stack.pop();
                    }
                }
                PaintCommand::PushLayer {
                    rect,
                    opacity,
                    blend_mode,
                } => {
                    self.flush_pending(device, queue, encoder, &mut inst_cursor, background);
                    let tex_idx = self.layer_stack.len();
                    if tex_idx >= self.layer_pool.len() {
                        self.layer_pool.push(RenderTargetTex::new(
                            device,
                            "craftui-sdf-layer",
                            self.width,
                            self.height,
                        ));
                    }
                    self.layer_stack.push(ActiveLayer {
                        tex_idx,
                        bounds: cur_xform.transform_rect_bounds(*rect),
                        opacity: *opacity,
                        blend_mode: *blend_mode,
                        cleared: false,
                    });
                }
                PaintCommand::PopLayer => {
                    self.pop_active_layer(
                        device,
                        queue,
                        encoder,
                        &mut inst_cursor,
                        &mut uniform_cursor,
                        background,
                    );
                }
                PaintCommand::PushTransform { transform } => {
                    let composed = cur_xform * *transform;
                    xform_stack.push(composed);
                }
                PaintCommand::PopTransform => {
                    if xform_stack.len() > 1 {
                        xform_stack.pop();
                    }
                }
                PaintCommand::PlatformViewHole { rect, .. } => {
                    self.push_quad(
                        QuadBatchKind::Hole,
                        SdfQuadInstance {
                            xform_0: xf0,
                            xform_1: [xm[4], xm[5], 4.0, 0.0],
                            rect: [rect.x, rect.y, rect.width, rect.height],
                            radii: [0.0; 4],
                            color0: [0.0; 4],
                            color1: [0.0; 4],
                            grad_pts: [0.0; 4],
                            params: [0.0, 0.0, 0.0, 1.0],
                            uv_rect: [0.0, 0.0, 1.0, 1.0],
                            clip_rect,
                            clip_radii,
                        },
                    );
                }
            }
        }

        while !self.layer_stack.is_empty() {
            self.pop_active_layer(
                device,
                queue,
                encoder,
                &mut inst_cursor,
                &mut uniform_cursor,
                background,
            );
        }

        self.flush_pending(device, queue, encoder, &mut inst_cursor, background);
    }

    pub fn present_to_surface(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        surface_format: wgpu::TextureFormat,
    ) {
        let is_srgb = surface_format.is_srgb();
        queue.write_buffer(
            &self.present_u_buf,
            0,
            bytemuck::bytes_of(&PresentUniforms {
                surface_is_srgb: if is_srgb { 1.0 } else { 0.0 },
                opacity: 1.0,
                blend_mode: 0.0,
                pad: 0.0,
            }),
        );
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.present_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.present_u_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.scene_tex.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.dummy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.linear_sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("craftui-sdf-present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.present_pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn sample_stops(sorted: &[ColorStop], t: f32) -> Color {
    if sorted.is_empty() {
        return Color::TRANSPARENT;
    }
    if t <= sorted[0].offset {
        return sorted[0].color;
    }
    for win in sorted.windows(2) {
        let a = win[0];
        let b = win[1];
        if t <= b.offset {
            let span = (b.offset - a.offset).max(1e-5);
            return a
                .color
                .lerp(b.color, ((t - a.offset) / span).clamp(0.0, 1.0));
        }
    }
    sorted.last().unwrap().color
}
