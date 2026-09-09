//! UI 公共工具：配色、DPI、字体、图标、宽字符转换。
//!
//! 配色值对齐 .NET 版 `App.xaml` 与 `MainWindow.xaml.cs:413-422`。

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, RoundRect, SelectObject, HBRUSH, HDC, HFONT,
    HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

/// 页面背景 `#FFF3F3F3`。
pub const COLOR_PAGE: COLORREF = COLORREF(0x00F3F3F3);
/// 卡片背景 白。
pub const COLOR_CARD: COLORREF = COLORREF(0x00FFFFFF);
/// 强调色 `#0F6CBD`（COLORREF 为 0x00BBGGRR）。
pub const COLOR_ACCENT: COLORREF = COLORREF(0x00BD6C0F);
/// 危险色 `#C42B1C`。
pub const COLOR_DANGER: COLORREF = COLORREF(0x001C2BC4);
/// 次要文字 `#616161`。
pub const COLOR_TEXT_SECONDARY: COLORREF = COLORREF(0x00616161);
/// 主文字近黑 `#1A1A1A`。
pub const COLOR_TEXT_PRIMARY: COLORREF = COLORREF(0x001A1A1A);
/// 控件底 `#FBFBFB`。
pub const COLOR_CONTROL: COLORREF = COLORREF(0x00FBFBFB);
/// 浅描边（近似 `#0F000000` 叠在白底上）。
pub const COLOR_STROKE: COLORREF = COLORREF(0x00E8E8E8);
/// 悬停描边 `#72000000` 近似。
pub const COLOR_STROKE_HOVER: COLORREF = COLORREF(0x008A8A8A);
/// 开关关闭轨道 `#8A8A8A`。
pub const COLOR_TOGGLE_OFF: COLORREF = COLORREF(0x008A8A8A);
/// 强调悬停 `#115EA3`。
pub const COLOR_ACCENT_HOVER: COLORREF = COLORREF(0x00A35E11);
/// 窗口细边。
pub const COLOR_WINDOW_BORDER: COLORREF = COLORREF(0x00D0D0D0);

/// Menu geometry is the common reference: 8 DIP corners on 36 DIP controls.
/// Small controls scale proportionally; larger surfaces retain the menu radius.
pub fn component_radius(height: i32, dpi: u32) -> i32 {
    let reference_height = scale(36, dpi).max(1);
    let radius = scale(8, dpi);
    ((radius * height.min(reference_height) + reference_height / 2) / reference_height)
        .min(height / 2)
        .max(0)
}

pub fn fill_component(hdc: HDC, r: RECT, dpi: u32, fill: COLORREF, border: Option<COLORREF>) {
    fill_round(
        hdc,
        r,
        component_radius(r.bottom - r.top, dpi),
        fill,
        border,
    );
}

/// 状态色（点/环），对齐 `MainWindow.xaml.cs:413-422`。
pub fn state_color(state: crate::model::LinkState) -> COLORREF {
    use crate::model::LinkState::*;
    match state {
        Online => COLORREF(0x000F7B0F),
        Connecting => COLORREF(0x00BD6C0F),
        Waiting | Degraded => COLORREF(0x001050CA),
        Error => COLORREF(0x001C2BC4),
        Offline => COLORREF(0x008A8A8A),
    }
}

/// 创建实心画刷。调用方负责 [`delete_gdi`]。
pub fn solid_brush(color: COLORREF) -> HBRUSH {
    unsafe { CreateSolidBrush(color) }
}

/// 释放 GDI 对象（画刷/字体），容忍空句柄。
pub fn delete_gdi(obj: HGDIOBJ) {
    if obj.0 as isize != 0 {
        unsafe {
            let _ = DeleteObject(obj);
        }
    }
}

pub fn brush_as_gdi(h: HBRUSH) -> HGDIOBJ {
    HGDIOBJ(h.0)
}

pub fn font_as_gdi(h: HFONT) -> HGDIOBJ {
    HGDIOBJ(h.0)
}

/// 圆角填充矩形，可选 1px 描边。
///
/// 不用 GDI 画笔（1px 笔是锯齿主因）：先铺描边色，再内缩 1px 铺填充色。
pub fn fill_round(hdc: HDC, r: RECT, radius: i32, fill: COLORREF, border: Option<COLORREF>) {
    unsafe {
        use windows::Win32::Graphics::Gdi::{GetStockObject, NULL_PEN};
        let null_pen = GetStockObject(NULL_PEN);
        let old_p = SelectObject(hdc, null_pen);
        let radius = radius.max(0);
        if let Some(bc) = border {
            let b = CreateSolidBrush(bc);
            let old_b = SelectObject(hdc, HGDIOBJ(b.0));
            let _ = RoundRect(
                hdc,
                r.left,
                r.top,
                r.right,
                r.bottom,
                radius * 2,
                radius * 2,
            );
            let _ = SelectObject(hdc, old_b);
            let _ = DeleteObject(HGDIOBJ(b.0));
            let inner = RECT {
                left: r.left + 1,
                top: r.top + 1,
                right: r.right - 1,
                bottom: r.bottom - 1,
            };
            let ir = (radius - 1).max(0);
            let b = CreateSolidBrush(fill);
            let old_b = SelectObject(hdc, HGDIOBJ(b.0));
            let _ = RoundRect(
                hdc,
                inner.left,
                inner.top,
                inner.right,
                inner.bottom,
                ir * 2,
                ir * 2,
            );
            let _ = SelectObject(hdc, old_b);
            let _ = DeleteObject(HGDIOBJ(b.0));
        } else {
            let b = CreateSolidBrush(fill);
            let old_b = SelectObject(hdc, HGDIOBJ(b.0));
            let _ = RoundRect(
                hdc,
                r.left,
                r.top,
                r.right,
                r.bottom,
                radius * 2,
                radius * 2,
            );
            let _ = SelectObject(hdc, old_b);
            let _ = DeleteObject(HGDIOBJ(b.0));
        }
        let _ = SelectObject(hdc, old_p);
    }
}

/// 按 DPI 缩放像素（96 DPI 基准）。
pub fn scale(px: i32, dpi: u32) -> i32 {
    ((px as i64 * dpi as i64) / 96) as i32
}

/// 创建 UI 字体。`px_size` 为 96 DPI 下的像素高度。
///
/// 字体族对齐 .NET 版 `MainWindow.xaml:12`：Segoe UI Variable Text → Segoe UI →
/// Microsoft YaHei UI，由系统 fallback 自行选择。
pub fn create_font(px_size: i32, dpi: u32, semibold: bool) -> HFONT {
    use windows::Win32::Graphics::Gdi::{
        CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
    };
    let height = -scale(px_size, dpi);
    // GDI 的 lfFaceName 只能写一个字体，不能写 CSS 式回退列表。
    let names = [
        w!("Segoe UI Variable Text"),
        w!("Segoe UI"),
        w!("Microsoft YaHei UI"),
    ];
    unsafe {
        for name in names {
            let h = CreateFontW(
                height,
                0,
                0,
                0,
                if semibold { 600 } else { 400 },
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                name,
            );
            if !h.is_invalid() {
                return h;
            }
        }
        CreateFontW(
            height,
            0,
            0,
            0,
            if semibold { 600 } else { 400 },
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("MS Shell Dlg 2"),
        )
    }
}

/// 屏幕 DPI（96 基准）。用于窗口与字体缩放。
pub fn screen_dpi() -> u32 {
    unsafe {
        let hdc = windows::Win32::Graphics::Gdi::GetDC(None);
        let dpi = windows::Win32::Graphics::Gdi::GetDeviceCaps(
            Some(hdc),
            windows::Win32::Graphics::Gdi::GET_DEVICE_CAPS_INDEX(88), // LOGPIXELSX
        );
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc);
        if dpi > 0 {
            dpi as u32
        } else {
            96
        }
    }
}

/// Query the actual monitor DPI of an existing per-monitor-aware window.
pub fn window_dpi(hwnd: windows::Win32::Foundation::HWND) -> u32 {
    #[link(name = "user32")]
    extern "system" {
        fn GetDpiForWindow(hwnd: windows::Win32::Foundation::HWND) -> u32;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        screen_dpi()
    } else {
        dpi
    }
}

/// 指定窗口所在显示器的工作区（不含任务栏）。
pub fn monitor_work_area(hwnd: windows::Win32::Foundation::HWND) -> RECT {
    #[link(name = "user32")]
    extern "system" {
        fn MonitorFromWindow(hwnd: windows::Win32::Foundation::HWND, flags: u32) -> isize;
    }
    monitor_area(unsafe { MonitorFromWindow(hwnd, 2) })
}

/// WM_DPICHANGED's suggested rectangle may already be on a different monitor.
pub fn rect_work_area(rect: RECT) -> RECT {
    #[link(name = "user32")]
    extern "system" {
        fn MonitorFromRect(rect: *const RECT, flags: u32) -> isize;
    }
    monitor_area(unsafe { MonitorFromRect(&rect, 2) })
}

fn monitor_area(monitor: isize) -> RECT {
    #[repr(C)]
    struct MonitorInfo {
        cb_size: u32,
        rc_monitor: RECT,
        rc_work: RECT,
        dw_flags: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetMonitorInfoW(monitor: isize, mi: *mut MonitorInfo) -> i32;
    }
    let mut mi = MonitorInfo {
        cb_size: std::mem::size_of::<MonitorInfo>() as u32,
        rc_monitor: RECT::default(),
        rc_work: RECT::default(),
        dw_flags: 0,
    };
    if unsafe { GetMonitorInfoW(monitor, &mut mi) } != 0 {
        mi.rc_work
    } else {
        work_area()
    }
}

/// 工作区（不含任务栏），用于窗口居中，避免整屏居中导致偏下。
pub fn work_area() -> RECT {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    unsafe {
        #[link(name = "user32")]
        extern "system" {
            fn SystemParametersInfoW(action: u32, param: u32, pv: *mut RECT, winini: u32) -> i32;
        }
        let _ = SystemParametersInfoW(0x0030, 0, &mut r, 0);
    }
    r
}

/// UTF-8 → UTF-16（NUL 结尾）。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 确保图标文件存在于数据目录（从内嵌字节释放），返回文件路径。
///
/// .NET 版直接从嵌入资源加载；Win32 LoadImageW 需要文件或资源，故落盘一次。
pub fn ensure_icon_file() -> Option<std::path::PathBuf> {
    const ICON_BYTES: &[u8] = include_bytes!("../../resources/app.ico");
    let dir = crate::paths::root()?;
    let path = dir.join("app.ico");
    let needs_write = match std::fs::metadata(&path) {
        Ok(m) => m.len() != ICON_BYTES.len() as u64,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&path, ICON_BYTES).ok()?;
    }
    Some(path)
}

/// 从图标文件加载指定尺寸的 HICON。
pub fn load_icon(path: &std::path::Path, cx: i32, cy: i32) -> Option<HICON> {
    use windows::Win32::UI::WindowsAndMessaging::{LoadImageW, IMAGE_ICON, LR_LOADFROMFILE};
    let wide = wide(&path.to_string_lossy());
    unsafe {
        let handle = LoadImageW(
            None,
            windows::core::PCWSTR(wide.as_ptr()),
            IMAGE_ICON,
            cx,
            cy,
            LR_LOADFROMFILE,
        );
        match handle {
            Ok(h) => {
                if h.is_invalid() {
                    None
                } else {
                    Some(HICON(h.0))
                }
            }
            Err(_) => None,
        }
    }
}

/// 销毁 HICON。
pub fn destroy_icon(icon: HICON) {
    unsafe {
        let _ = DestroyIcon(icon);
    }
}

/// 光栅化后的预乘 alpha 位图（BGRA）。
pub struct SvgBmp {
    pub w: i32,
    pub h: i32,
    pub hbmp: windows::Win32::Graphics::Gdi::HBITMAP,
    pub bits: *mut u8,
}

impl Drop for SvgBmp {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.hbmp.0));
        }
    }
}

/// 将 `resources/app.svg` 渲成 `px` 见方的位图（按 viewBox 等比）。
pub fn rasterize_app_svg(px: u32) -> Option<SvgBmp> {
    rasterize_svg(include_bytes!("../../resources/app.svg"), px)
}

pub fn rasterize_eye_on(px: u32) -> Option<SvgBmp> {
    rasterize_svg(include_bytes!("../../resources/eye-on.svg"), px)
}

pub fn rasterize_eye_off(px: u32) -> Option<SvgBmp> {
    rasterize_svg(include_bytes!("../../resources/eye-off.svg"), px)
}

pub fn rasterize_svg(svg: &[u8], px: u32) -> Option<SvgBmp> {
    let tree = resvg::usvg::Tree::from_data(svg, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let dim = size.width().max(size.height()).max(1.0);
    let scale = px as f32 / dim;
    let w = ((size.width() * scale).round() as u32).max(1);
    let h = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    rgba_to_dib(pixmap.data(), w as i32, h as i32)
}

fn rgba_to_dib(rgba: &[u8], w: i32, h: i32) -> Option<SvgBmp> {
    use windows::Win32::Graphics::Gdi::{
        CreateDIBSection, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hbmp = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()? };
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
        }
        return None;
    }
    let n = (w * h) as usize;
    let dst = unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, n * 4) };
    for i in 0..n {
        let r = rgba[i * 4] as u16;
        let g = rgba[i * 4 + 1] as u16;
        let b = rgba[i * 4 + 2] as u16;
        let a = rgba[i * 4 + 3] as u16;
        // tiny-skia already supplies premultiplied channels.
        dst[i * 4] = b as u8;
        dst[i * 4 + 1] = g as u8;
        dst[i * 4 + 2] = r as u8;
        dst[i * 4 + 3] = a as u8;
    }
    Some(SvgBmp {
        w,
        h,
        hbmp: HBITMAP(hbmp.0),
        bits: bits.cast(),
    })
}

/// 带 alpha 绘制 SVG 位图。
pub fn blit_svg(hdc: HDC, bmp: &SvgBmp, x: i32, y: i32, dw: i32, dh: i32) {
    use windows::Win32::Graphics::Gdi::{
        AlphaBlend, CreateCompatibleDC, DeleteDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
        BLENDFUNCTION,
    };
    unsafe {
        let mem = CreateCompatibleDC(Some(hdc));
        let old = SelectObject(mem, HGDIOBJ(bmp.hbmp.0));
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = AlphaBlend(hdc, x, y, dw, dh, mem, 0, 0, bmp.w, bmp.h, blend);
        let _ = SelectObject(mem, old);
        let _ = DeleteDC(mem);
    }
}

/// 字体度量：`(ascent, descent, height)`，height = ascent + descent。
pub fn font_metrics(font: HFONT) -> (i32, i32, i32) {
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetTextMetricsW, ReleaseDC, SelectObject, TEXTMETRICW,
    };
    unsafe {
        let hdc = GetDC(None);
        let old = SelectObject(hdc, font_as_gdi(font));
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        let _ = SelectObject(hdc, old);
        let _ = ReleaseDC(None, hdc);
        (tm.tmAscent, tm.tmDescent, tm.tmHeight)
    }
}

/// 单行文字的绘制高度（DrawText 计算），用于把原生 EDIT 窗口对准视觉中线。
pub fn measure_line_height(font: HFONT) -> i32 {
    use windows::Win32::Graphics::Gdi::{
        DrawTextW, GetDC, ReleaseDC, SelectObject, DT_CALCRECT, DT_NOPREFIX, DT_SINGLELINE,
    };
    unsafe {
        let hdc = GetDC(None);
        let old = SelectObject(hdc, font_as_gdi(font));
        let mut sample = wide("好Ag");
        let n = sample.len().saturating_sub(1);
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 400,
            bottom: 0,
        };
        let _ = DrawTextW(
            hdc,
            &mut sample[..n],
            &mut rc,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
        let _ = SelectObject(hdc, old);
        let _ = ReleaseDC(None, hdc);
        (rc.bottom - rc.top).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_layout_is_bbggrr() {
        // #0F6CBD → R=0x0F G=0x6C B=0xBD → COLORREF = 0x00BD6C0F
        assert_eq!(COLOR_ACCENT.0, 0x00BD6C0F);
    }

    #[test]
    fn state_colors_match_dot_palette() {
        use crate::model::LinkState::*;
        assert_eq!(state_color(Online).0, 0x000F7B0F);
        assert_eq!(state_color(Offline).0, 0x008A8A8A);
        assert_eq!(state_color(Error).0, 0x001C2BC4);
        assert_eq!(state_color(Waiting).0, 0x001050CA);
    }

    #[test]
    fn scale_at_standard_dpi_is_identity() {
        assert_eq!(scale(40, 96), 40);
        assert_eq!(scale(40, 192), 80);
        assert_eq!(scale(46, 144), 69);
    }

    #[test]
    fn svg_alpha_is_premultiplied_exactly_once() {
        let bmp = rasterize_svg(br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#ff0000" opacity="0.5"/></svg>"##, 8).unwrap();
        let pixel = unsafe { std::slice::from_raw_parts(bmp.bits, 4) };
        assert_eq!(pixel[0], 0);
        assert_eq!(pixel[1], 0);
        assert!((127..=128).contains(&pixel[3]));
        assert_eq!(
            pixel[2], pixel[3],
            "already-premultiplied red must not be multiplied again"
        );
    }
}
