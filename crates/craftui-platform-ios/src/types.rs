use craftui_core::{Insets, PlatformViewId};
use std::ffi::c_void;
use crate::host::IosAppHost;

/// iOS native view hosting strategy inside CraftUI `PlatformView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosPlatformViewMode {
    /// Zero-copy `CVPixelBufferRef` -> `MTLTexture` / `TextureHandle` composited directly inside Vello.
    ExternalCvPixelBuffer { cv_pixel_buffer_ptr: usize },
    /// Transparent UIKit `UIView` overlay cutout coordinated via `PaintCommand::PlatformViewHole`.
    HolePunchUiView { view_id: PlatformViewId },
}

/// Pixel format reported by a CoreVideo [`CvPixelBufferBridge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CvPixelBufferFormat {
    Rgba8,
    /// `kCVPixelFormatType_32BGRA` (native iOS `AVCaptureVideoDataOutput` / `CAMetalLayer` format).
    Bgra8,
}

/// CPU/IOSurface frame payload acquired from a CoreVideo `CVPixelBufferRef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CvPixelBufferFrame {
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: usize,
    pub format: CvPixelBufferFormat,
    pub bytes: Vec<u8>,
}

/// Trait bridging native `CVPixelBufferRef` (Swift/ObjC camera, video, or ARKit feeds) into CraftUI.
pub trait CvPixelBufferBridge: Send + Sync {
    fn acquire_latest_frame(&mut self) -> Option<CvPixelBufferFrame>;
}

/// Convert packed or strided `BGRA8` pixels from `kCVPixelFormatType_32BGRA` into packed straight `RGBA8`.
pub fn bgra_frame_to_packed_rgba(frame: &CvPixelBufferFrame) -> Vec<u8> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    let row_bytes = w.saturating_mul(4);
    let mut out = vec![0u8; row_bytes.saturating_mul(h)];
    let stride = frame.bytes_per_row.max(row_bytes);

    for y in 0..h {
        let src_start = y * stride;
        let dst_start = y * row_bytes;
        if src_start + row_bytes > frame.bytes.len() {
            break;
        }
        let src_row = &frame.bytes[src_start..src_start + row_bytes];
        let dst_row = &mut out[dst_start..dst_start + row_bytes];
        match frame.format {
            CvPixelBufferFormat::Rgba8 => dst_row.copy_from_slice(src_row),
            CvPixelBufferFormat::Bgra8 => {
                for (dst_px, src_px) in dst_row
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(src_row.as_chunks::<4>().0)
                {
                    dst_px[0] = src_px[2];
                    dst_px[1] = src_px[1];
                    dst_px[2] = src_px[0];
                    dst_px[3] = src_px[3];
                }
            }
        }
    }
    out
}

/// iOS window & notch / Dynamic Island safe area insets in logical points (`pt`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IosSafeAreaInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl From<IosSafeAreaInsets> for Insets {
    fn from(v: IosSafeAreaInsets) -> Self {
        Insets::new(v.top, v.right, v.bottom, v.left)
    }
}

/// `UIResponder.keyboardWillChangeFrameNotification` transition state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IosKeyboardFrame {
    pub overlap_height_pt: f32,
    pub visible: bool,
}

/// Desired UIKit keyboard state emitted after each iOS frame step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IosImeCommand {
    BecomeFirstResponder,
    ResignFirstResponder,
    SyncText(String),
}

/// C-ABI callback invoked by UIKit launcher to populate the root CraftUI [`WidgetTree`].
pub type IosBuildUiCallback = extern "C" fn(host: *mut IosAppHost, user_data: *mut c_void);

