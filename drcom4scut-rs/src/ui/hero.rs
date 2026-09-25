//! HeroUI v3 visual primitives shared by all native windows.
//! Geometry follows the approved desktop sketch; no application behavior lives here.
use super::winutil::*;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::*;

pub const CONTROL_RADIUS: i32 = 12;

pub fn rect(dpi: u32, x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: scale(x, dpi),
        top: scale(y, dpi),
        right: scale(x + w, dpi),
        bottom: scale(y + h, dpi),
    }
}

pub fn mix(a: COLORREF, b: COLORREF, percent: u32) -> COLORREF {
    let p = percent.min(100);
    let channel =
        |shift: u32| (((a.0 >> shift) & 255u32) * (100 - p) + ((b.0 >> shift) & 255u32) * p) / 100;
    COLORREF(channel(0) | channel(8) << 8 | channel(16) << 16)
}

pub fn soft(p: Palette, color: COLORREF) -> COLORREF {
    mix(p.page, color, 14)
}

pub unsafe fn text(
    dc: HDC,
    font: HFONT,
    color: COLORREF,
    mut r: RECT,
    value: &str,
    flags: DRAW_TEXT_FORMAT,
) {
    let old = SelectObject(dc, font_as_gdi(font));
    let _ = SetBkMode(dc, TRANSPARENT);
    let _ = SetTextColor(dc, color);
    let mut value: Vec<u16> = value.encode_utf16().collect();
    DrawTextW(dc, &mut value, &mut r, flags | DT_NOPREFIX);
    SelectObject(dc, old);
}

pub fn surface(dc: HDC, r: RECT, dpi: u32, fill: COLORREF, radius: i32) {
    fill_round(
        dc,
        r,
        scale(radius, dpi).min((r.bottom - r.top) / 2),
        fill,
        None,
    );
}

/// Field and primary-action fills use the same vector geometry as their rings.
pub fn control_surface(dc: HDC, r: RECT, dpi: u32, fill: COLORREF) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let radius = scale(CONTROL_RADIUS, dpi).min(h / 2);
    let rgb = ((fill.0 & 255) << 16) | (fill.0 & 0xff00) | ((fill.0 >> 16) & 255);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect width="{w}" height="{h}" rx="{radius}" fill="#{rgb:06x}"/></svg>"##
    );
    cached_bitmap(dc, r, svg.clone(), || {
        rasterize_svg(svg.as_bytes(), w.max(h) as u32)
    });
}

pub fn field(dc: HDC, r: RECT, dpi: u32, p: Palette, focused: bool) {
    if !p.is_dark {
        let shadow = RECT {
            top: r.top + scale(2, dpi),
            bottom: r.bottom + scale(2, dpi),
            ..r
        };
        control_surface(dc, shadow, dpi, mix(p.page, p.text_primary, 7));
    }
    control_surface(dc, r, dpi, p.control);
    if focused {
        // Keep one outline inside the field; an outer GDI stroke is clipped by
        // native child bounds and must not be combined with a button focus ring.
        unsafe {
            rounded_outline(
                dc,
                r,
                p.accent,
                scale(2, dpi).max(1),
                scale(CONTROL_RADIUS, dpi),
            );
        }
    }
}

pub fn switch(dc: HDC, x: i32, y: i32, dpi: u32, p: Palette, progress: f32) {
    let t = progress.clamp(0.0, 1.0);
    let track = rect(dpi, x, y, 40, 20);
    surface(
        dc,
        track,
        dpi,
        super::anim::lerp_color(p.toggle_off, p.accent, t),
        12,
    );
    let offset = (scale(14, dpi) as f32 * t).round() as i32;
    let mut thumb = rect(dpi, x + 2, y + 2, 22, 16);
    thumb.left += offset;
    thumb.right += offset;
    surface(dc, thumb, dpi, COLORREF(0xFFFFFF), 8);
}

unsafe fn rounded_outline(dc: HDC, r: RECT, color: COLORREF, width: i32, radius: i32) {
    let inset = width as f32 / 2.0;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    let radius = (radius as f32 - inset).max(0.0);
    let path = format!(
        r#"<rect x="{inset}" y="{inset}" width="{}" height="{}" rx="{radius}"/>"#,
        w - width,
        h - width
    );
    vector(dc, r, color, &format!("0 0 {w} {h}"), width as f32, &path);
}

pub unsafe fn line(dc: HDC, r: RECT, color: COLORREF) {
    let brush = solid_brush(color);
    let _ = FillRect(dc, &r, brush);
    delete_gdi(brush_as_gdi(brush));
}

#[derive(Clone, Copy)]
pub enum Icon {
    Network,
    User,
    Lock,
    Folder,
    Download,
    Trash,
    Check,
    Info,
    Monitor,
    Close,
    Minus,
}

// Bounded UI-thread cache: vector rasterization occurs once per size/color/state,
// not once per mouse move. SvgBmp owns the DIB and releases evicted handles.
thread_local! {
    static VECTORS: std::cell::RefCell<std::collections::VecDeque<(String, SvgBmp)>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

unsafe fn vector(dc: HDC, r: RECT, color: COLORREF, view_box: &str, stroke: f32, paths: &str) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let rgb = ((color.0 & 255) << 16) | (color.0 & 0xff00) | ((color.0 >> 16) & 255);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{view_box}" fill="none" stroke="#{rgb:06x}" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"##
    );
    let key = svg.clone();
    cached_bitmap(dc, r, key, || {
        rasterize_svg(svg.as_bytes(), w.max(h) as u32)
    });
}

/// The original three-ring brand artwork, rendered at its actual display size.
pub fn logo(dc: HDC, r: RECT) {
    let size = (r.right - r.left).min(r.bottom - r.top);
    if size <= 0 {
        return;
    }
    cached_bitmap(dc, r, format!("app-logo:{size}"), || {
        rasterize_app_svg(size as u32)
    });
}

fn cached_bitmap(dc: HDC, r: RECT, key: String, render: impl FnOnce() -> Option<SvgBmp>) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    VECTORS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, bmp)) = cache.iter().find(|(stored, _)| stored == &key) {
            blit_svg(dc, bmp, r.left, r.top, w, h);
        } else if let Some(bmp) = render() {
            blit_svg(dc, &bmp, r.left, r.top, w, h);
            if cache.len() == 64 {
                cache.pop_front();
            }
            cache.push_back((key, bmp));
        }
    });
}

/// Antialiased outlines at the actual device resolution, shared by parent and children.
pub unsafe fn icon(dc: HDC, r: RECT, color: COLORREF, symbol: Icon) {
    let paths = match symbol {
        Icon::Network => r#"<path d="M9 2h6v6H9zM2 16h6v6H2zM16 16h6v6h-6zM12 8v4M5 16v-4h14v4"/>"#,
        Icon::User => r#"<circle cx="12" cy="6" r="4"/><path d="M4 22v-2a8 8 0 0 1 16 0v2"/>"#,
        Icon::Lock => {
            r#"<rect x="4" y="10" width="16" height="12" rx="2"/><path d="M7 10V7a5 5 0 0 1 10 0v3M12 14v4"/>"#
        }
        Icon::Folder => r#"<path d="M2 6h7l3 3h10v12H2z"/>"#,
        Icon::Download => r#"<path d="M12 2v13M7 10l5 5 5-5M3 15v6h18v-6"/>"#,
        Icon::Trash => r#"<path d="M6 7v15h12V7M3 6h18M9 3h6M10 10v9M14 10v9"/>"#,
        Icon::Check => r#"<path d="m4 12 5 5L20 6"/>"#,
        Icon::Info => r#"<circle cx="12" cy="12" r="10"/><path d="M12 11v7M12 6v1"/>"#,
        Icon::Monitor => r#"<path d="M2 3h20v15H2zM12 18v4M7 22h10"/>"#,
        Icon::Close => r#"<path d="m6 6 12 12M18 6 6 18"/>"#,
        Icon::Minus => r#"<path d="M5 12h14"/>"#,
    };
    vector(dc, r, color, "0 0 24 24", 1.6, paths);
}

pub unsafe fn chevron(dc: HDC, r: RECT, color: COLORREF, progress: f32) {
    let angle = progress.clamp(0.0, 1.0) * 180.0;
    vector(
        dc,
        r,
        color,
        "0 0 24 24",
        1.6,
        &format!(r#"<path d="m6 9 6 6 6-6" transform="rotate({angle:.2} 12 12)"/>"#),
    );
}
