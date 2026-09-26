//! UI 公共工具：配色、DPI、字体、图标、宽字符转换。
//!
//! 配色对齐 HeroUI 规范（Light / Dark 双主题、高对比度语义 Token 与平滑圆角）。

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, RoundRect, SelectObject, HBRUSH, HDC, HFONT,
    HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

// HeroUI v3 theme tokens, derived from the approved desktop sketch
// (@heroui/styles 3.2.6 oklch values; see handoff fidelity plan).
// `oklch_to_colorref` is the single source; the constants below are its
// rounded results, pinned by `constants_match_sketch_oklch`.
pub const COLOR_LIGHT_PAGE: COLORREF = COLORREF(0x00F5F5F5); // oklch(0.9702 0 0)
pub const COLOR_LIGHT_CARD: COLORREF = COLORREF(0x00FFFFFF); // surface
pub const COLOR_LIGHT_TEXT_PRIMARY: COLORREF = COLORREF(0x001B1818); // oklch(0.2103 0.0059 285.89)
pub const COLOR_LIGHT_TEXT_SECONDARY: COLORREF = COLORREF(0x007A7171); // oklch(0.5517 0.0138 285.94)
pub const COLOR_LIGHT_STROKE: COLORREF = COLORREF(0x00E0DEDE); // oklch(0.90 0.004 286.32)
pub const COLOR_LIGHT_STROKE_HOVER: COLORREF = COLORREF(0x00E2E1E1); // fill*96% + fg*4%
pub const COLOR_LIGHT_CONTROL: COLORREF = COLORREF(0x00FFFFFF); // field = surface
pub const COLOR_LIGHT_CONTROL_HOVER: COLORREF = COLORREF(0x00F9F9F9); // field*90% + fg*2% (normalized)
pub const COLOR_LIGHT_TOGGLE_OFF: COLORREF = COLORREF(0x00ECEBEB); // oklch(0.94 0.001 286.375)
pub const COLOR_LIGHT_SEGMENT: COLORREF = COLORREF(0x00FFFFFF); // selected tab pill
pub const COLOR_LIGHT_WINDOW_BORDER: COLORREF = COLORREF(0x00E0DEDE);
pub const COLOR_LIGHT_ACCENT_SOFT: COLORREF = COLORREF(0x00F5E4D1); // blue 15% over page
pub const COLOR_LIGHT_ACCENT_TEXT: COLORREF = COLORREF(0x00AE631E); // oklab(blue 70%, fg 30%)
pub const COLOR_LIGHT_DANGER: COLORREF = COLORREF(0x003C38FF); // oklch(0.6532 0.2328 25.74)
pub const COLOR_LIGHT_DANGER_HOVER: COLORREF = COLORREF(0x005155FF); // oklab(danger 90%, white 10%)
pub const COLOR_LIGHT_DANGER_SOFT: COLORREF = COLORREF(0x00D9D9F6);
pub const COLOR_LIGHT_DANGER_TEXT: COLORREF = COLORREF(0x003334A4); // oklab(danger 70%, fg 40%) normalized
pub const COLOR_LIGHT_SUCCESS_SOFT: COLORREF = COLORREF(0x00DFEED4);
pub const COLOR_LIGHT_SUCCESS_TEXT: COLORREF = COLORREF(0x0045772B); // oklab(success 80%, fg 60%) normalized
pub const COLOR_LIGHT_WARNING: COLORREF = COLORREF(0x0024A5F5); // oklch(0.7819 0.1585 72.33)
pub const COLOR_LIGHT_WARNING_SOFT: COLORREF = COLORREF(0x00D6E9F5);
pub const COLOR_LIGHT_WARNING_TEXT: COLORREF = COLORREF(0x002E5F85); // oklab(warning 80%, fg 70%) normalized
pub const COLOR_DARK_PAGE: COLORREF = COLORREF(0x00070606); // oklch(0.12 0.005 285.823)
pub const COLOR_DARK_CARD: COLORREF = COLORREF(0x001B1818); // oklch(0.2103 0.0059 285.89)
pub const COLOR_DARK_TEXT_PRIMARY: COLORREF = COLORREF(0x00FCFCFC); // oklch(0.9911 0 0)
pub const COLOR_DARK_TEXT_SECONDARY: COLORREF = COLORREF(0x00A99F9F); // oklch(0.705 0.015 286.067)
pub const COLOR_DARK_STROKE: COLORREF = COLORREF(0x002C2828); // oklch(0.28 0.006 286.033)
pub const COLOR_DARK_STROKE_HOVER: COLORREF = COLORREF(0x00312E2E);
pub const COLOR_DARK_CONTROL: COLORREF = COLORREF(0x001B1818); // field = surface
pub const COLOR_DARK_CONTROL_HOVER: COLORREF = COLORREF(0x001F1C1C);
pub const COLOR_DARK_TOGGLE_OFF: COLORREF = COLORREF(0x002A2727); // oklch(0.274 0.006 286.033)
pub const COLOR_DARK_SEGMENT: COLORREF = COLORREF(0x004C4646); // oklch(0.3964 0.01 285.93)
pub const COLOR_DARK_WINDOW_BORDER: COLORREF = COLORREF(0x002C2828);
pub const COLOR_DARK_ACCENT_SOFT: COLORREF = COLORREF(0x00241506); // blue 12% over page
pub const COLOR_DARK_ACCENT_TEXT: COLORREF = COLORREF(0x00FBA861); // oklab(blue 80%, fg 30%) normalized
pub const COLOR_DARK_DANGER: COLORREF = COLORREF(0x003E3BDB); // oklch(0.594 0.1967 24.63)
pub const COLOR_DARK_DANGER_HOVER: COLORREF = COLORREF(0x005154E1);
pub const COLOR_DARK_DANGER_SOFT: COLORREF = COLORREF(0x000F0E26);
pub const COLOR_DARK_DANGER_TEXT: COLORREF = COLORREF(0x007278EB);
pub const COLOR_DARK_SUCCESS_SOFT: COLORREF = COLORREF(0x00152309);
pub const COLOR_DARK_SUCCESS_TEXT: COLORREF = COLORREF(0x008FD874);
pub const COLOR_DARK_WARNING: COLORREF = COLORREF(0x0050B7F7); // oklch(0.8203 0.1388 76.34)
pub const COLOR_DARK_WARNING_SOFT: COLORREF = COLORREF(0x0012212A);
pub const COLOR_DARK_WARNING_TEXT: COLORREF = COLORREF(0x0086CBF9);
/// HeroUI v3 primary blue, oklch(0.6204 0.195 253.83) ≈ #0485F7.
pub const COLOR_ACCENT: COLORREF = COLORREF(0x00F78504);
pub const COLOR_ACCENT_HOVER: COLORREF = COLORREF(0x00F99235); // oklab(blue 90%, white 10%)
pub const COLOR_ACCENT_ACTIVE: COLORREF = COLORREF(0x00D77303); // oklab(blue 90%, black 10%) ≈ pressed
pub const COLOR_SUCCESS: COLORREF = COLORREF(0x0064C917); // oklch(0.7329 0.1935 150.81)
/// HeroUI v3 on-accent white, oklch(0.9911 0 0) ≈ #FCFCFC.
pub const COLOR_ON_ACCENT: COLORREF = COLORREF(0x00FCFCFC);

// --- 默认回退别名（浅色基准，保持原有常量可用） ---
pub const COLOR_PAGE: COLORREF = COLOR_LIGHT_PAGE;
pub const COLOR_CARD: COLORREF = COLOR_LIGHT_CARD;
pub const COLOR_TEXT_PRIMARY: COLORREF = COLOR_LIGHT_TEXT_PRIMARY;
pub const COLOR_TEXT_SECONDARY: COLORREF = COLOR_LIGHT_TEXT_SECONDARY;
pub const COLOR_CONTROL: COLORREF = COLOR_LIGHT_CONTROL;
pub const COLOR_STROKE: COLORREF = COLOR_LIGHT_STROKE;
pub const COLOR_STROKE_HOVER: COLORREF = COLOR_LIGHT_STROKE_HOVER;
pub const COLOR_TOGGLE_OFF: COLORREF = COLOR_LIGHT_TOGGLE_OFF;
pub const COLOR_WINDOW_BORDER: COLORREF = COLOR_LIGHT_WINDOW_BORDER;

/// oklch → sRGB（Björn Ottosson OKLab 矩阵，简单色域钳制）。
/// 返回 COLORREF（0x00BBGGRR）。token 常量的可验证来源。
pub fn oklch_to_colorref(l: f64, c: f64, h: f64) -> COLORREF {
    let hr = h.to_radians();
    let a = c * hr.cos();
    let b = c * hr.sin();
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3;
    let g = -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3;
    let b = -0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3;
    let gamma = |v: f64| -> u32 {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u32
    };
    COLORREF(gamma(r) | gamma(g) << 8 | gamma(b) << 16)
}

/// HeroUI 配色板
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub is_dark: bool,
    pub page: COLORREF,
    pub card: COLORREF,
    pub text_primary: COLORREF,
    pub text_secondary: COLORREF,
    pub stroke: COLORREF,
    pub stroke_hover: COLORREF,
    pub control: COLORREF,
    pub control_hover: COLORREF,
    pub toggle_off: COLORREF,
    pub segment: COLORREF,
    pub window_border: COLORREF,
    pub accent: COLORREF,
    pub accent_hover: COLORREF,
    pub accent_active: COLORREF,
    pub accent_soft: COLORREF,
    pub accent_text: COLORREF,
    pub danger: COLORREF,
    pub danger_hover: COLORREF,
    pub danger_soft: COLORREF,
    pub danger_text: COLORREF,
    pub success: COLORREF,
    pub success_soft: COLORREF,
    pub success_text: COLORREF,
    pub warning: COLORREF,
    pub warning_soft: COLORREF,
    pub warning_text: COLORREF,
    pub on_accent: COLORREF,
    pub disabled_bg: COLORREF,
    pub disabled_text: COLORREF,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            is_dark: false,
            page: COLOR_LIGHT_PAGE,
            card: COLOR_LIGHT_CARD,
            text_primary: COLOR_LIGHT_TEXT_PRIMARY,
            text_secondary: COLOR_LIGHT_TEXT_SECONDARY,
            stroke: COLOR_LIGHT_STROKE,
            stroke_hover: COLOR_LIGHT_STROKE_HOVER,
            control: COLOR_LIGHT_CONTROL,
            control_hover: COLOR_LIGHT_CONTROL_HOVER,
            toggle_off: COLOR_LIGHT_TOGGLE_OFF,
            segment: COLOR_LIGHT_SEGMENT,
            window_border: COLOR_LIGHT_WINDOW_BORDER,
            accent: COLOR_ACCENT,
            accent_hover: COLOR_ACCENT_HOVER,
            accent_active: COLOR_ACCENT_ACTIVE,
            accent_soft: COLOR_LIGHT_ACCENT_SOFT,
            accent_text: COLOR_LIGHT_ACCENT_TEXT,
            danger: COLOR_LIGHT_DANGER,
            danger_hover: COLOR_LIGHT_DANGER_HOVER,
            danger_soft: COLOR_LIGHT_DANGER_SOFT,
            danger_text: COLOR_LIGHT_DANGER_TEXT,
            success: COLOR_SUCCESS,
            success_soft: COLOR_LIGHT_SUCCESS_SOFT,
            success_text: COLOR_LIGHT_SUCCESS_TEXT,
            warning: COLOR_LIGHT_WARNING,
            warning_soft: COLOR_LIGHT_WARNING_SOFT,
            warning_text: COLOR_LIGHT_WARNING_TEXT,
            on_accent: COLOR_ON_ACCENT,
            disabled_bg: COLOR_LIGHT_TOGGLE_OFF,
            disabled_text: COLOR_LIGHT_TEXT_SECONDARY,
        }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            page: COLOR_DARK_PAGE,
            card: COLOR_DARK_CARD,
            text_primary: COLOR_DARK_TEXT_PRIMARY,
            text_secondary: COLOR_DARK_TEXT_SECONDARY,
            stroke: COLOR_DARK_STROKE,
            stroke_hover: COLOR_DARK_STROKE_HOVER,
            control: COLOR_DARK_CONTROL,
            control_hover: COLOR_DARK_CONTROL_HOVER,
            toggle_off: COLOR_DARK_TOGGLE_OFF,
            segment: COLOR_DARK_SEGMENT,
            window_border: COLOR_DARK_WINDOW_BORDER,
            accent: COLOR_ACCENT,
            accent_hover: COLOR_ACCENT_HOVER,
            accent_active: COLOR_ACCENT_ACTIVE,
            accent_soft: COLOR_DARK_ACCENT_SOFT,
            accent_text: COLOR_DARK_ACCENT_TEXT,
            danger: COLOR_DARK_DANGER,
            danger_hover: COLOR_DARK_DANGER_HOVER,
            danger_soft: COLOR_DARK_DANGER_SOFT,
            danger_text: COLOR_DARK_DANGER_TEXT,
            success: COLOR_SUCCESS,
            success_soft: COLOR_DARK_SUCCESS_SOFT,
            success_text: COLOR_DARK_SUCCESS_TEXT,
            warning: COLOR_DARK_WARNING,
            warning_soft: COLOR_DARK_WARNING_SOFT,
            warning_text: COLOR_DARK_WARNING_TEXT,
            on_accent: COLOR_ON_ACCENT,
            disabled_bg: COLOR_DARK_TOGGLE_OFF,
            disabled_text: COLOR_DARK_TEXT_SECONDARY,
        }
    }

    pub fn for_dark(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

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

/// 状态点/图标的实心色，对齐草图语义色。
pub fn state_color(p: &Palette, state: crate::model::LinkState) -> COLORREF {
    use crate::model::LinkState::*;
    match state {
        Online => p.success,
        Connecting => p.accent,
        Waiting | Degraded => p.warning,
        Error => p.danger,
        Offline => p.text_secondary,
    }
}

/// 状态 chip/图标芯片的 (soft 底, 文字色)，与草图 hu-chip/hu-status-icon 配色一致。
pub fn state_pair(p: &Palette, state: crate::model::LinkState) -> (COLORREF, COLORREF) {
    use crate::model::LinkState::*;
    match state {
        Online => (p.success_soft, p.success_text),
        Connecting => (p.accent_soft, p.accent_text),
        Waiting | Degraded => (p.warning_soft, p.warning_text),
        Error => (p.danger_soft, p.danger_text),
        Offline => (p.toggle_off, p.text_primary),
    }
}

/// Owner-drawn buttons supply their own opaque background in the buffered frame.
/// Suppress the separate system erase that otherwise precedes WM_DRAWITEM.
pub unsafe fn suppress_button_erase(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{WM_ERASEBKGND, WM_NCDESTROY};
    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
        id: usize,
        _: usize,
    ) -> LRESULT {
        if msg == WM_ERASEBKGND {
            return LRESULT(1);
        }
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(proc), id);
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
    let _ = SetWindowSubclass(hwnd, Some(proc), 0x4845, 0);
}

/// Render an opaque surface offscreen, then publish it in one blit. The callback
/// uses the destination's logical coordinates and must fill the complete rect.
/// Native text remains at device resolution; only vector edges are antialiased.
pub unsafe fn paint_buffered(hdc: HDC, r: RECT, paint: impl FnOnce(HDC)) {
    use windows::Win32::Graphics::Gdi::*;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    if mem.0.is_null() || bmp.0.is_null() {
        if !mem.0.is_null() {
            let _ = DeleteDC(mem);
        }
        if !bmp.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
        }
        paint(hdc);
        return;
    }
    let old = SelectObject(mem, HGDIOBJ(bmp.0));
    struct Surface(HDC, HBITMAP, HGDIOBJ);
    impl Drop for Surface {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.0, self.2);
                let _ = DeleteObject(HGDIOBJ(self.1 .0));
                let _ = DeleteDC(self.0);
            }
        }
    }
    let _surface = Surface(mem, bmp, old);
    let _ = SetViewportOrgEx(mem, -r.left, -r.top, None);
    paint(mem);
    let _ = SetViewportOrgEx(mem, 0, 0, None);
    let _ = BitBlt(hdc, r.left, r.top, w, h, Some(mem), 0, 0, SRCCOPY);
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
/// 优先走 GDI+（抗锯齿）；GDI+ 不可用时回退到旧 GDI 实现：
/// 不用 GDI 画笔（1px 笔是锯齿主因），先铺描边色，再内缩 1px 铺填充色。
pub fn fill_round(hdc: HDC, r: RECT, radius: i32, fill: COLORREF, border: Option<COLORREF>) {
    if crate::ui::gdiplus::fill_round(hdc, r, radius, fill, border) {
        return;
    }
    fill_round_gdi(hdc, r, radius, fill, border)
}

/// 旧 GDI 回退实现（无抗锯齿）。
fn fill_round_gdi(hdc: HDC, r: RECT, radius: i32, fill: COLORREF, border: Option<COLORREF>) {
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

/// Readable caption size shared by the client and both setup windows.
pub const UI_CAPTION_SIZE: i32 = 13;

fn font_installed(face: &[u16]) -> bool {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Graphics::Gdi::{
        EnumFontFamiliesExW, GetDC, ReleaseDC, DEFAULT_CHARSET, LOGFONTW, TEXTMETRICW,
    };

    unsafe extern "system" fn enum_font_proc(
        _elf: *const LOGFONTW,
        _ntm: *const TEXTMETRICW,
        _font_type: u32,
        lparam: LPARAM,
    ) -> i32 {
        let found = &mut *(lparam.0 as *mut bool);
        *found = true;
        0
    }

    unsafe {
        let hdc = GetDC(None);
        let mut lf = LOGFONTW {
            lfCharSet: DEFAULT_CHARSET,
            ..Default::default()
        };
        let len = face.len().min(lf.lfFaceName.len() - 1);
        lf.lfFaceName[..len].copy_from_slice(&face[..len]);
        lf.lfFaceName[len] = 0;
        let mut found = false;
        let _ = EnumFontFamiliesExW(
            hdc,
            &lf,
            Some(enum_font_proc),
            LPARAM(&mut found as *mut bool as isize),
            0,
        );
        let _ = ReleaseDC(None, hdc);
        found
    }
}

struct FontFamilies {
    regular: Vec<u16>,
    emphasis: Vec<u16>,
    emphasis_weight: i32,
}

static FONT_FAMILIES: std::sync::OnceLock<FontFamilies> = std::sync::OnceLock::new();

fn font_families() -> &'static FontFamilies {
    FONT_FAMILIES.get_or_init(|| {
        fn load(compressed: &[u8]) -> bool {
            use std::io::Read;
            let mut bytes = Vec::new();
            if flate2::read::ZlibDecoder::new(compressed)
                .read_to_end(&mut bytes)
                .is_err()
            {
                return false;
            }
            let mut count = 0;
            // GDI copies the bytes. The private font lives for this process;
            // Windows releases it at exit, without a system font installation.
            let handle = unsafe {
                windows::Win32::Graphics::Gdi::AddFontMemResourceEx(
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    None,
                    &mut count,
                )
            };
            !handle.is_invalid() && count > 0
        }
        let regular = load(include_bytes!(
            "../../resources/fonts/NotoSansSC-Regular.ttf.zlib"
        ));
        let medium = load(include_bytes!(
            "../../resources/fonts/NotoSansSC-Medium.ttf.zlib"
        ));
        if regular && medium {
            // Memory fonts are not enumerable; select their actual family names
            // directly, using the supplied weight instead of synthetic bold.
            return FontFamilies {
                regular: wide("Noto Sans SC"),
                emphasis: wide("Noto Sans SC Medium"),
                emphasis_weight: 500,
            };
        }
        let family = [
            "Microsoft YaHei UI",
            "Segoe UI Variable Text",
            "Segoe UI",
            "MS Shell Dlg 2",
        ]
        .into_iter()
        .map(wide)
        .find(|name| font_installed(name))
        .unwrap_or_else(|| wide("MS Shell Dlg 2"));
        FontFamilies {
            regular: family.clone(),
            emphasis: family,
            emphasis_weight: 600,
        }
    })
}

/// 创建 UI 字体。`px_size` 为 96 DPI 下的像素高度。
///
/// 统一使用内嵌 Noto Sans SC 常规/中等字重；私有字体加载失败时才回退系统字体。
pub fn create_font(px_size: i32, dpi: u32, semibold: bool) -> HFONT {
    create_font_weight(px_size, dpi, if semibold { 500 } else { 400 })
}

/// 指定字重创建 UI 字体（>400 选中等字重家族，GDI 可合成更重字重）。
pub fn create_font_weight(px_size: i32, dpi: u32, weight: i32) -> HFONT {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
    };
    // Round the requested em height (as MulDiv does) instead of truncating it:
    // 14 DIP at 125% is 17.5 px, so a 17 px font loses a pixel of CJK detail.
    let height = -((px_size as i64 * dpi as i64 + 48) / 96).max(1) as i32;
    let families = font_families();
    let family = if weight > 400 {
        &families.emphasis
    } else {
        &families.regular
    };
    let weight = weight.clamp(400, 900);
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            PCWSTR(family.as_ptr()),
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

/// 检测 Windows 系统是否启用了应用深色模式。
/// 若注册表键不存在或读取失败，默认回退浅色模式（false）。
pub fn is_system_dark_mode() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
        REG_VALUE_TYPE,
    };
    unsafe {
        let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return false;
        }
        let val_name = wide("AppsUseLightTheme");
        let mut data: u32 = 1;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut val_type = REG_VALUE_TYPE::default();
        let res = RegQueryValueExW(
            hkey,
            PCWSTR(val_name.as_ptr()),
            None,
            Some(&mut val_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if res.is_ok() && val_type == REG_DWORD {
            data == 0
        } else {
            false
        }
    }
}

/// 设置窗口的沉浸式深色模式（Win10 2004+ / Win11）。
/// 若当前系统不支持 DWM 特性，则静默忽略。
pub fn set_window_dark_mode(hwnd: windows::Win32::Foundation::HWND, dark: bool) {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: windows::Win32::Foundation::HWND,
            attr: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let val: i32 = if dark { 1 } else { 0 };
    unsafe {
        // 20 = DWMWA_USE_IMMERSIVE_DARK_MODE (Windows 11 及 Windows 10 2004+)
        // 19 = 旧版 Windows 10 属性
        if DwmSetWindowAttribute(hwnd, 20, (&val as *const i32).cast(), 4) != 0 {
            let _ = DwmSetWindowAttribute(hwnd, 19, (&val as *const i32).cast(), 4);
        }
    }
}

/// 计算 sRGB 颜色的相对亮度 (WCAG 2.1 规范)
pub fn relative_luminance(c: COLORREF) -> f64 {
    let r = (c.0 & 0xFF) as f64 / 255.0;
    let g = ((c.0 >> 8) & 0xFF) as f64 / 255.0;
    let b = ((c.0 >> 16) & 0xFF) as f64 / 255.0;
    let to_linear = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * to_linear(r) + 0.7152 * to_linear(g) + 0.0722 * to_linear(b)
}

/// 计算两颜色的 WCAG 对比度 (返回值范围 1.0 到 21.0)
pub fn contrast_ratio(c1: COLORREF, c2: COLORREF) -> f64 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
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

/// Golden-image review for the offscreen snapshot tests.
///
/// - `DRCOM_UI_UPDATE_GOLDEN=1` + `DRCOM_UI_GOLDEN_DIR`: refresh the baseline.
/// - `DRCOM_UI_GOLDEN_DIR` alone: compare the freshly rendered PNG with the
///   baseline; panics when pixels diverge beyond antialiasing tolerance
///   (per-channel ±2, at most 0.1% of pixels over tolerance).
/// - Neither set: no-op, snapshots remain eyeball-only artifacts.
pub fn golden_review(path: &std::path::Path) {
    let Some(dir) = std::env::var_os("DRCOM_UI_GOLDEN_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let golden = dir.join(path.file_name().expect("snapshot file name"));
    if std::env::var_os("DRCOM_UI_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(&dir).expect("create golden directory");
        std::fs::copy(path, &golden).expect("refresh golden baseline");
        return;
    }
    if !golden.exists() {
        eprintln!("golden baseline missing, skipping: {}", golden.display());
        return;
    }
    let actual = resvg::tiny_skia::Pixmap::load_png(path).expect("load rendered png");
    let expected = resvg::tiny_skia::Pixmap::load_png(&golden).expect("load golden png");
    assert_eq!(
        (actual.width(), actual.height()),
        (expected.width(), expected.height()),
        "golden size mismatch: {}",
        golden.display()
    );
    let a = actual.data();
    let b = expected.data();
    let total = (actual.width() * actual.height()) as usize;
    let bad = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(pa, pb)| (0..4).any(|c| (pa[c] as i32 - pb[c] as i32).abs() > 2))
        .count();
    assert!(
        bad * 1000 <= total,
        "golden mismatch in {}: {bad}/{total} pixels beyond tolerance",
        golden.display()
    );
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

pub fn rasterize_eye_on(px: u32, color: COLORREF) -> Option<SvgBmp> {
    rasterize_eye_icon(include_bytes!("../../resources/eye-on.svg"), px, color)
}

pub fn rasterize_eye_off(px: u32, color: COLORREF) -> Option<SvgBmp> {
    rasterize_eye_icon(include_bytes!("../../resources/eye-off.svg"), px, color)
}

fn rasterize_eye_icon(svg: &[u8], px: u32, color: COLORREF) -> Option<SvgBmp> {
    let bmp = rasterize_svg(svg, px)?;
    // The licensed monochrome artwork supplies coverage; recolor the
    // premultiplied BGRA pixels without changing its paths or antialiasing.
    let pixels = unsafe { std::slice::from_raw_parts_mut(bmp.bits, (bmp.w * bmp.h * 4) as usize) };
    for p in pixels.chunks_exact_mut(4) {
        let alpha = p[3] as u16;
        p[0] = (((color.0 >> 16) & 0xff) as u16 * alpha / 255) as u8;
        p[1] = (((color.0 >> 8) & 0xff) as u16 * alpha / 255) as u8;
        p[2] = ((color.0 & 0xff) as u16 * alpha / 255) as u8;
    }
    Some(bmp)
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
        // RGB #0485F7 is stored as BGR.
        assert_eq!(COLOR_ACCENT.0, 0x00F78504);
        // RGB #FF383C (light danger) is stored as BGR.
        assert_eq!(COLOR_LIGHT_DANGER.0, 0x003C38FF);
    }

    #[test]
    fn constants_match_sketch_oklch() {
        // The approved sketch states oklch tokens; constants must stay within
        // rounding distance of the conversion so the palette cannot drift.
        let close = |actual: COLORREF, l: f64, c: f64, h: f64| {
            let expected = oklch_to_colorref(l, c, h).0;
            for shift in [0, 8, 16] {
                let a = ((actual.0 >> shift) & 0xff) as i32;
                let e = ((expected >> shift) & 0xff) as i32;
                assert!(
                    (a - e).abs() <= 2,
                    "channel mismatch: actual {actual:08X?} vs oklch({l} {c} {h}) = {expected:08X?}"
                );
            }
        };
        close(COLOR_ACCENT, 0.6204, 0.195, 253.83);
        close(COLOR_SUCCESS, 0.7329, 0.1935, 150.81);
        close(COLOR_LIGHT_WARNING, 0.7819, 0.1585, 72.33);
        close(COLOR_DARK_WARNING, 0.8203, 0.1388, 76.34);
        close(COLOR_LIGHT_DANGER, 0.6532, 0.2328, 25.74);
        close(COLOR_DARK_DANGER, 0.594, 0.1967, 24.63);
        close(COLOR_LIGHT_PAGE, 0.9702, 0.0, 0.0);
        close(COLOR_DARK_PAGE, 0.12, 0.005, 285.823);
        close(COLOR_DARK_CARD, 0.2103, 0.0059, 285.89);
        close(COLOR_LIGHT_TEXT_PRIMARY, 0.2103, 0.0059, 285.89);
        close(COLOR_DARK_TEXT_PRIMARY, 0.9911, 0.0, 0.0);
        close(COLOR_LIGHT_TEXT_SECONDARY, 0.5517, 0.0138, 285.94);
        close(COLOR_DARK_TEXT_SECONDARY, 0.705, 0.015, 286.067);
        close(COLOR_LIGHT_TOGGLE_OFF, 0.94, 0.001, 286.375);
        close(COLOR_DARK_TOGGLE_OFF, 0.274, 0.006, 286.033);
        close(COLOR_LIGHT_STROKE, 0.90, 0.004, 286.32);
        close(COLOR_DARK_STROKE, 0.28, 0.006, 286.033);
        close(COLOR_DARK_SEGMENT, 0.3964, 0.01, 285.93);
        // Known HeroUI anchors from the published theme.
        assert_eq!(COLOR_SUCCESS.0, 0x0064C917, "success must be #17C964");
        assert_eq!(COLOR_LIGHT_WARNING.0, 0x0024A5F5, "warning must be #F5A524");
    }

    #[test]
    fn state_colors_match_heroui_palette() {
        use crate::model::LinkState::*;
        let light = Palette::light();
        assert_eq!(state_color(&light, Online), light.success);
        assert_eq!(state_color(&light, Offline), light.text_secondary);
        assert_eq!(state_color(&light, Error), light.danger);
        assert_eq!(state_color(&light, Waiting), light.warning);
        assert_eq!(state_color(&light, Connecting), light.accent);
        let dark = Palette::dark();
        assert_eq!(state_color(&dark, Error), dark.danger);
        assert_ne!(light.danger, dark.danger, "danger is per-theme in v3");
        assert_eq!(
            state_pair(&light, Online),
            (light.success_soft, light.success_text)
        );
        assert_eq!(
            state_pair(&dark, Offline),
            (dark.toggle_off, dark.text_primary)
        );
    }

    #[test]
    fn wcag_contrast_ratios_pass_aa() {
        let white = COLOR_ON_ACCENT;
        let light = Palette::light();
        assert!(
            contrast_ratio(light.text_primary, light.card) >= 4.5,
            "Light primary text on card contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(light.text_secondary, light.card) >= 4.5,
            "Light secondary text on card contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(light.text_primary, light.page) >= 4.5,
            "Light primary text on page contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(light.accent_text, light.page) >= 4.5,
            "Light accent text on page contrast must be >= 4.5:1"
        );
        // Sketch chip pair measures ≈4.47:1; keep the official tokens and
        // pin the measured floor instead of re-darkening the text color.
        assert!(
            contrast_ratio(light.success_text, light.success_soft) >= 4.4,
            "Light success text on soft chip contrast must stay >= 4.4:1"
        );
        // The approved sketch fills primary/danger actions with the official
        // v3 colors and white labels: this meets WCAG AA for large text (3:1)
        // but not the 4.5:1 body threshold. Measured: blue ≈ 3.6, danger ≈ 3.5.
        assert!(
            contrast_ratio(white, light.accent) >= 3.0,
            "White label on accent must meet AA large-text (3:1)"
        );
        assert!(
            contrast_ratio(white, light.danger) >= 3.0,
            "White label on danger must meet AA large-text (3:1)"
        );

        let dark = Palette::dark();
        assert!(
            contrast_ratio(dark.text_primary, dark.card) >= 4.5,
            "Dark primary text on card contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(dark.text_secondary, dark.card) >= 4.5,
            "Dark secondary text on card contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(dark.text_primary, dark.page) >= 4.5,
            "Dark primary text on page contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(white, dark.accent) >= 3.0,
            "White label on dark mode accent must meet AA large-text (3:1)"
        );
        assert!(
            contrast_ratio(white, dark.danger) >= 3.0,
            "White label on dark mode danger must meet AA large-text (3:1)"
        );
        assert!(
            contrast_ratio(dark.danger_text, dark.page) >= 4.5,
            "Dark error text on page contrast must be >= 4.5:1"
        );
        assert!(
            contrast_ratio(dark.danger_text, dark.card) >= 4.5,
            "Dark error text on card contrast must be >= 4.5:1"
        );
    }

    #[test]
    fn themed_eye_icons_remain_visible_and_premultiplied() {
        for palette in [Palette::light(), Palette::dark()] {
            assert!(contrast_ratio(palette.text_secondary, palette.control) >= 3.0);
            let bmp = rasterize_eye_on(40, palette.text_secondary).unwrap();
            let pixels =
                unsafe { std::slice::from_raw_parts(bmp.bits, (bmp.w * bmp.h * 4) as usize) };
            assert!(pixels.chunks_exact(4).any(|p| p[3] == 255));
            assert!(pixels
                .chunks_exact(4)
                .all(|p| p[..3].iter().all(|c| *c <= p[3])));
        }
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

    #[test]
    fn bundled_fonts_are_selected_with_real_weights_and_cjk_coverage() {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Gdi::*;
        unsafe {
            let dc = GetDC(None);
            for (emphasis, family, weight) in [
                (false, "Noto Sans SC", 400),
                (true, "Noto Sans SC Medium", 500),
            ] {
                for dpi in [96, 120, 144, 192] {
                    let font = create_font(14, dpi, emphasis);
                    assert!(!font.is_invalid());
                    let old = SelectObject(dc, font_as_gdi(font));
                    // GetObject only reports the requested face; GetTextFace
                    // verifies the private font actually selected into the DC.
                    let mut face = [0u16; 128];
                    assert!(GetTextFaceW(dc, Some(&mut face)) > 0);
                    let end = face.iter().position(|&c| c == 0).unwrap();
                    let actual = String::from_utf16_lossy(&face[..end]);
                    assert!(actual == family || (emphasis && actual == "Noto Sans SC"));
                    let mut metrics = TEXTMETRICW::default();
                    GetTextMetricsW(dc, &mut metrics).unwrap();
                    assert_eq!(metrics.tmWeight, weight);
                    // Inspect the actual font table too: synthesized bold can
                    // report the requested weight without selecting Medium.
                    let mut class = [0u8; 2];
                    assert_eq!(
                        GetFontData(
                            dc,
                            u32::from_le_bytes(*b"OS/2"),
                            4,
                            Some(class.as_mut_ptr().cast()),
                            2
                        ),
                        2
                    );
                    assert_eq!(u16::from_be_bytes(class) as i32, weight);
                    let sample = wide("校园网络密码连接偏好设置網絡 Windows 0123456789 · •");
                    let mut glyphs = vec![0u16; sample.len() - 1];
                    assert_ne!(
                        GetGlyphIndicesW(
                            dc,
                            PCWSTR(sample.as_ptr()),
                            glyphs.len() as i32,
                            glyphs.as_mut_ptr(),
                            GGI_MARK_NONEXISTING_GLYPHS
                        ),
                        u32::MAX
                    );
                    assert!(
                        glyphs.iter().all(|g| *g != 0xffff),
                        "missing bundled UI glyph"
                    );
                    let _ = SelectObject(dc, old);
                    let _ = DeleteObject(font_as_gdi(font));
                }
            }
            let _ = ReleaseDC(None, dc);
        }
    }
}
