//! Minimal GDI+ FFI used only for anti-aliased rounded rectangles.
//!
//! `gdiplus.dll` ships with every Windows version since XP, so a startup
//! failure simply falls back to the legacy GDI path (no anti-aliasing).

use std::sync::OnceLock;

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::HDC;

type GpStatus = i32;
type GpGraphics = *mut core::ffi::c_void;
type GpPath = *mut core::ffi::c_void;
type GpBrush = *mut core::ffi::c_void;
type GpPen = *mut core::ffi::c_void;

const STATUS_OK: GpStatus = 0;
const SMOOTHING_MODE_ANTI_ALIAS: i32 = 4;
const FILL_MODE_ALTERNATE: i32 = 0;
const UNIT_PIXEL: i32 = 2;

#[repr(C)]
struct GdiplusStartupInput {
    version: u32,
    debug_event_callback: *const core::ffi::c_void,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[repr(C)]
struct GdiplusStartupOutput {
    notification_hook: *const core::ffi::c_void,
    notification_unhook: *const core::ffi::c_void,
}

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(
        token: *mut usize,
        input: *const GdiplusStartupInput,
        output: *mut GdiplusStartupOutput,
    ) -> GpStatus;
    fn GdipCreateFromHDC(hdc: HDC, graphics: *mut GpGraphics) -> GpStatus;
    fn GdipDeleteGraphics(graphics: GpGraphics) -> GpStatus;
    fn GdipSetSmoothingMode(graphics: GpGraphics, mode: i32) -> GpStatus;
    fn GdipCreatePath(fill_mode: i32, path: *mut GpPath) -> GpStatus;
    fn GdipDeletePath(path: GpPath) -> GpStatus;
    fn GdipAddPathArc(
        path: GpPath,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        start_angle: f32,
        sweep_angle: f32,
    ) -> GpStatus;
    fn GdipAddPathRectangle(path: GpPath, x: f32, y: f32, width: f32, height: f32) -> GpStatus;
    fn GdipClosePathFigure(path: GpPath) -> GpStatus;
    fn GdipCreateSolidFill(color: u32, brush: *mut GpBrush) -> GpStatus;
    fn GdipDeleteBrush(brush: GpBrush) -> GpStatus;
    fn GdipFillPath(graphics: GpGraphics, brush: GpBrush, path: GpPath) -> GpStatus;
    fn GdipCreatePen1(color: u32, width: f32, unit: i32, pen: *mut GpPen) -> GpStatus;
    fn GdipDeletePen(pen: GpPen) -> GpStatus;
    fn GdipDrawPath(graphics: GpGraphics, pen: GpPen, path: GpPath) -> GpStatus;
}

static TOKEN: OnceLock<Option<usize>> = OnceLock::new();

/// Process-lifetime GDI+ token; `None` when the library cannot start.
fn token() -> Option<usize> {
    *TOKEN.get_or_init(|| unsafe {
        let mut token = 0usize;
        let input = GdiplusStartupInput {
            version: 1,
            debug_event_callback: std::ptr::null(),
            suppress_background_thread: 1,
            suppress_external_codecs: 1,
        };
        let mut output = GdiplusStartupOutput {
            notification_hook: std::ptr::null(),
            notification_unhook: std::ptr::null(),
        };
        if GdiplusStartup(&mut token, &input, &mut output) == STATUS_OK && token != 0 {
            Some(token)
        } else {
            None
        }
    })
}

pub fn available() -> bool {
    token().is_some()
}

/// COLORREF (`0x00BBGGRR`) -> GDI+ ARGB (`0xAARRGGBB`), fully opaque.
fn argb(color: COLORREF) -> u32 {
    let c = color.0;
    0xFF00_0000 | ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

/// Anti-aliased rounded-rectangle fill with an optional 1px border.
/// Returns `false` when GDI+ is unavailable so the caller can fall back.
pub fn fill_round(hdc: HDC, r: RECT, radius: i32, fill: COLORREF, border: Option<COLORREF>) -> bool {
    if token().is_none() {
        return false;
    }
    unsafe {
        let mut graphics: GpGraphics = std::ptr::null_mut();
        if GdipCreateFromHDC(hdc, &mut graphics) != STATUS_OK || graphics.is_null() {
            return false;
        }
        let result = fill_round_inner(graphics, r, radius, fill, border);
        let _ = GdipDeleteGraphics(graphics);
        result
    }
}

unsafe fn fill_round_inner(
    graphics: GpGraphics,
    r: RECT,
    radius: i32,
    fill: COLORREF,
    border: Option<COLORREF>,
) -> bool {
    let mut path: GpPath = std::ptr::null_mut();
    if GdipCreatePath(FILL_MODE_ALTERNATE, &mut path) != STATUS_OK || path.is_null() {
        return false;
    }
    let x = r.left as f32;
    let y = r.top as f32;
    let w = (r.right - r.left) as f32;
    let h = (r.bottom - r.top) as f32;
    let d = (radius.max(0) as f32 * 2.0).min(w).min(h);
    if d < 1.0 {
        let _ = GdipAddPathRectangle(path, x, y, w, h);
    } else {
        let _ = GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        let _ = GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        let _ = GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        let _ = GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        let _ = GdipClosePathFigure(path);
    }

    let _ = GdipSetSmoothingMode(graphics, SMOOTHING_MODE_ANTI_ALIAS);

    let mut brush: GpBrush = std::ptr::null_mut();
    let filled = GdipCreateSolidFill(argb(fill), &mut brush) == STATUS_OK && !brush.is_null();
    if filled {
        let _ = GdipFillPath(graphics, brush, path);
        let _ = GdipDeleteBrush(brush);
    }

    let mut stroked = false;
    if let Some(bc) = border {
        let mut pen: GpPen = std::ptr::null_mut();
        if GdipCreatePen1(argb(bc), 1.0, UNIT_PIXEL, &mut pen) == STATUS_OK && !pen.is_null() {
            stroked = GdipDrawPath(graphics, pen, path) == STATUS_OK;
            let _ = GdipDeletePen(pen);
        }
    } else {
        stroked = true;
    }

    let _ = GdipDeletePath(path);
    filled && stroked
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::*;

    #[test]
    fn rounded_corners_are_antialiased() {
        if !available() {
            return; // GDI+ missing: fallback path is used instead
        }
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, 64, 64);
            let old = SelectObject(mem, HGDIOBJ(bmp.0));
            // White background.
            let white = CreateSolidBrush(COLORREF(0x00FFFFFF));
            let rc = RECT { left: 0, top: 0, right: 64, bottom: 64 };
            FillRect(mem, &rc, white);
            let _ = DeleteObject(HGDIOBJ(white.0));
            // Black rounded rect.
            let r = RECT { left: 4, top: 4, right: 60, bottom: 60 };
            assert!(fill_round(mem, r, 10, COLORREF(0), None));
            let mut has_white = false;
            let mut has_black = false;
            let mut has_midtone = false;
            for y in 0..64 {
                for x in 0..64 {
                    let c = GetPixel(mem, x, y).0;
                    if c == 0x00FFFFFF {
                        has_white = true;
                    } else if c == 0 {
                        has_black = true;
                    } else {
                        has_midtone = true;
                    }
                }
            }
            let _ = SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            assert!(has_white && has_black, "fill did not draw expected colors");
            assert!(has_midtone, "no interpolated pixels: anti-aliasing missing");
        }
    }
}
