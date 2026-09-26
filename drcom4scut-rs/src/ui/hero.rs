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

/// Single rounded-rectangle rasterizer for every surface: resvg vector
/// geometry, cached per size/radius/colors. No GDI/GDI+ corner fallback.
fn rounded_fill(dc: HDC, r: RECT, radius: i32, fill: COLORREF, border: Option<(COLORREF, i32)>) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let radius = radius.clamp(0, h.min(w) / 2);
    let rgb = |c: COLORREF| ((c.0 & 255) << 16) | (c.0 & 0xff00) | ((c.0 >> 16) & 255);
    let (stroke, inset) = match border {
        Some((c, bw)) => (
            format!(r##" stroke="#{:06x}" stroke-width="{}""##, rgb(c), bw),
            bw as f32 / 2.0,
        ),
        None => (String::new(), 0.0),
    };
    let inner_r = (radius as f32 - inset).max(0.0);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect x="{inset}" y="{inset}" width="{}" height="{}" rx="{inner_r}" fill="#{:06x}"{stroke}/></svg>"##,
        (w as f32 - inset * 2.0).max(0.0),
        (h as f32 - inset * 2.0).max(0.0),
        rgb(fill),
    );
    cached_bitmap(dc, r, svg.clone(), || {
        rasterize_svg(svg.as_bytes(), w.max(h) as u32)
    });
}

/// Blurred black rounded layers composited beneath a surface, approximating
/// the sketch's box-shadow tokens (blur ≈ 2 × feGaussianBlur stdDeviation).
/// `layers`: (dx, dy, blur, alpha 0-255) in DIP at 96.
pub fn box_shadow(dc: HDC, r: RECT, dpi: u32, radius: i32, layers: &[(i32, i32, i32, u32)]) {
    if layers.is_empty() {
        return;
    }
    let max_out = layers
        .iter()
        .map(|&(dx, dy, blur, _)| (dx.abs() + dy.abs()) / 2 + blur * 2)
        .max()
        .unwrap_or(0);
    let margin = scale(max_out.max(4), dpi);
    let outer = RECT {
        left: r.left - margin,
        top: r.top - margin,
        right: r.right + margin,
        bottom: r.bottom + margin,
    };
    let (w, h) = (outer.right - outer.left, outer.bottom - outer.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let fw = r.right - r.left;
    let fh = r.bottom - r.top;
    let mut rects = String::new();
    for &(dx, dy, blur, alpha) in layers {
        if alpha == 0 {
            continue;
        }
        let x = margin + scale(dx, dpi);
        let y = margin + scale(dy, dpi);
        let deviation = scale(blur, dpi) as f32 / 2.0;
        rects.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{fw}" height="{fh}" rx="{radius}" fill="#000" fill-opacity="{}" filter="url(#b{deviation})"/>"##,
            alpha as f32 / 255.0
        ));
        // One filter per deviation value; id embeds the deviation to dedupe.
    }
    let mut defs = String::new();
    let mut seen = Vec::new();
    for &(_, _, blur, alpha) in layers {
        if alpha == 0 {
            continue;
        }
        let deviation = scale(blur, dpi) as f32 / 2.0;
        if seen.iter().any(|d| *d == deviation) {
            continue;
        }
        seen.push(deviation);
        defs.push_str(&format!(
            r##"<filter id="b{deviation}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{deviation}"/></filter>"##
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><defs>{defs}</defs>{rects}</svg>"##
    );
    cached_bitmap(dc, outer, svg.clone(), || {
        rasterize_svg(svg.as_bytes(), w.max(h) as u32)
    });
}

/// The sketch's `--hu-field-shadow`: three hairline layers, light theme only
/// (the dark token is fully transparent).
pub const FIELD_SHADOW: [(i32, i32, i32, u32); 3] = [(0, 2, 4, 10), (0, 1, 2, 15), (0, 0, 1, 15)];
/// Switch-thumb shadow `0 1px 3px #0000001a`, used in both themes.
pub const THUMB_SHADOW: [(i32, i32, i32, u32); 1] = [(0, 1, 3, 26)];

pub fn surface(dc: HDC, r: RECT, dpi: u32, fill: COLORREF, radius: i32) {
    surface_ex(dc, r, dpi, fill, radius, None)
}

pub fn surface_ex(
    dc: HDC,
    r: RECT,
    dpi: u32,
    fill: COLORREF,
    radius: i32,
    border: Option<COLORREF>,
) {
    let radius = scale(radius, dpi).min((r.bottom - r.top) / 2);
    rounded_fill(
        dc,
        r,
        radius,
        fill,
        border.map(|c| (c, scale(1, dpi).max(1))),
    );
}

/// Field and primary-action fills use the same vector geometry as their rings.
pub fn control_surface(dc: HDC, r: RECT, dpi: u32, fill: COLORREF) {
    let radius = scale(CONTROL_RADIUS, dpi).min((r.bottom - r.top) / 2);
    rounded_fill(dc, r, radius, fill, None);
}

/// v3 action buttons are fully rounded capsules (radius = height / 2).
pub fn action_surface(dc: HDC, r: RECT, dpi: u32, fill: COLORREF) {
    let _ = dpi;
    rounded_fill(dc, r, (r.bottom - r.top) / 2, fill, None);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldState {
    Normal,
    Hover,
    Focus,
}

/// Text field: soft shadow, surface fill, and the v3 outer 2-DIP focus ring.
/// The ring lives outside the field edge so inset native EDITs never clip it.
pub fn field(dc: HDC, r: RECT, dpi: u32, p: Palette, state: FieldState) {
    if !p.is_dark {
        box_shadow(dc, r, dpi, scale(CONTROL_RADIUS, dpi), &FIELD_SHADOW);
    }
    let fill = if state == FieldState::Hover {
        p.control_hover
    } else {
        p.control
    };
    control_surface(dc, r, dpi, fill);
    if state == FieldState::Focus {
        focus_ring(dc, r, dpi, p.accent, scale(CONTROL_RADIUS, dpi));
    }
}

/// Outer 2-DIP ring hugging the surface edge (`box-shadow: 0 0 0 2px`).
pub fn focus_ring(dc: HDC, r: RECT, dpi: u32, color: COLORREF, radius: i32) {
    let ring_w = scale(2, dpi).max(1);
    let margin = ring_w + scale(2, dpi).max(1);
    let outer = RECT {
        left: r.left - margin,
        top: r.top - margin,
        right: r.right + margin,
        bottom: r.bottom + margin,
    };
    let (w, h) = (outer.right - outer.left, outer.bottom - outer.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let rgb = ((color.0 & 255) << 16) | (color.0 & 0xff00) | ((color.0 >> 16) & 255);
    // Stroke the path whose edge sits ring_w/2 inside the field boundary, so
    // the band lands exactly on [edge, edge + ring_w] in device pixels.
    let offset = margin as f32 - ring_w as f32 / 2.0;
    let fw = r.right - r.left + ring_w;
    let fh = r.bottom - r.top + ring_w;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect x="{offset}" y="{offset}" width="{fw}" height="{fh}" rx="{}" fill="none" stroke="#{rgb:06x}" stroke-width="{ring_w}"/></svg>"##,
        radius as f32 + ring_w as f32 / 2.0,
    );
    cached_bitmap(dc, outer, svg.clone(), || {
        rasterize_svg(svg.as_bytes(), w.max(h) as u32)
    });
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
    box_shadow(dc, thumb, dpi, scale(8, dpi), &THUMB_SHADOW);
    surface(dc, thumb, dpi, COLORREF(0xFFFFFF), 8);
}

/// v3 checkbox: 18 DIP box, 2 DIP line border, radius 5; checked = blue fill
/// with a white check. `progress` drives the fill/border interpolation.
pub fn checkbox(dc: HDC, x: i32, y: i32, dpi: u32, p: Palette, progress: f32) {
    let t = progress.clamp(0.0, 1.0);
    let box_r = rect(dpi, x, y, 18, 18);
    let fill = super::anim::lerp_color(p.page, p.accent, t);
    let border = super::anim::lerp_color(p.stroke, p.accent, t);
    rounded_fill(
        dc,
        box_r,
        scale(5, dpi),
        fill,
        Some((border, scale(2, dpi).max(1))),
    );
    if t > 0.5 {
        unsafe {
            icon(
                dc,
                rect(dpi, x + 2, y + 2, 14, 14),
                p.on_accent,
                Icon::Check,
            );
        }
    }
}

/// 32 DIP icon-button background (rounded 8, hover uses the hover token).
pub fn icon_button(dc: HDC, r: RECT, dpi: u32, p: Palette, hovered: bool) {
    if hovered {
        surface(dc, r, dpi, p.stroke_hover, 8);
    }
}

/// v3 chip: soft background, optional 6 DIP dot, 12 DIP label, fully rounded.
/// Draws right-aligned at `right`; returns the consumed width in device px.
pub unsafe fn chip(
    dc: HDC,
    font: HFONT,
    right: i32,
    top: i32,
    dpi: u32,
    bg: COLORREF,
    fg: COLORREF,
    dot: Option<COLORREF>,
    label: &str,
) -> i32 {
    let mut measure = RECT::default();
    let old = SelectObject(dc, font_as_gdi(font));
    let mut value: Vec<u16> = label.encode_utf16().collect();
    let _ = DrawTextW(
        dc,
        &mut value,
        &mut measure,
        DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(dc, old);
    let text_w = measure.right - measure.left;
    let h = scale(26, dpi);
    let pad = scale(10, dpi);
    let (dot_d, gap) = if dot.is_some() {
        (scale(6, dpi), scale(6, dpi))
    } else {
        (0, 0)
    };
    let w = pad + dot_d + gap + text_w + pad;
    let r = RECT {
        left: right - w,
        top,
        right,
        bottom: top + h,
    };
    surface(dc, r, dpi, bg, 999);
    let mut text_left = r.left + pad;
    if let Some(dot) = dot {
        let dot_r = RECT {
            left: text_left,
            top: top + (h - dot_d) / 2,
            right: text_left + dot_d,
            bottom: top + (h - dot_d) / 2 + dot_d,
        };
        surface(dc, dot_r, dpi, dot, 999);
        text_left = dot_r.right + gap;
    }
    text(
        dc,
        font,
        fg,
        RECT {
            left: text_left,
            top,
            right: r.right - pad / 2,
            bottom: top + h,
        },
        label,
        DT_VCENTER | DT_SINGLELINE,
    );
    w
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
    ArrowRight,
    PlugZap,
    SlidersHorizontal,
    EthernetPort,
    ShieldCheck,
    TriangleAlert,
    AppWindow,
    PackageOpen,
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
        Icon::ArrowRight => r#"<path d="M5 12h14M13 6l6 6-6 6"/>"#,
        Icon::PlugZap => {
            r#"<path d="M6.3 20.3a2.4 2.4 0 0 1-2.3-3.1l1.8-5.7a2.4 2.4 0 0 1 2.3-1.8h7.8a2.4 2.4 0 0 1 2.3 1.8l1.8 5.7a2.4 2.4 0 0 1-2.3 3.1Z"/><path d="m12 9-2 3h4l-2 3"/><path d="M9 2v3M15 2v3"/>"#
        }
        Icon::SlidersHorizontal => {
            r#"<path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4"/>"#
        }
        Icon::EthernetPort => {
            r#"<path d="m15 20 3-3h2a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h2l3 3Z"/><path d="M6 8v1M10 8v1M14 8v1M18 8v1"/>"#
        }
        Icon::ShieldCheck => {
            r#"<path d="M20 13c0 5-3.5 7.5-7.7 9a.6.6 0 0 1-.6 0C7.5 20.5 4 18 4 13V6a1 1 0 0 1 .7-1c2.3-.8 4.7-2 6.6-3.2a1 1 0 0 1 1.4 0C14.6 3 17 4.2 19.3 5a1 1 0 0 1 .7 1Z"/><path d="m9 12 2 2 4-4"/>"#
        }
        Icon::TriangleAlert => {
            r#"<path d="m21.7 18-8-14a2 2 0 0 0-3.4 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.7-3ZM12 9v4M12 17h.01"/>"#
        }
        Icon::AppWindow => {
            r#"<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M10 8h4M6 12h.01M18 12h.01"/>"#
        }
        Icon::PackageOpen => {
            r#"<path d="M12 22v-9M2 10v8a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-8M7.5 2.7 2 6l7.5 4.2L12 8l2.5 2.2L22 6l-5.5-3.3L12 5.4 7.5 2.7Z"/>"#
        }
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
