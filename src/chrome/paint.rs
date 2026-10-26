//! Drawing the chrome (spec §4): a Direct2D DC render target bound to the
//! Window's paint DC, one pass for the bands above the editor and one for the
//! status bar, so the pixels under Scintilla are never touched. DirectWrite text.

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HINSTANCE, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_ALIASED,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS,
    DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, LoadImageW, DI_NORMAL, HICON, IMAGE_ICON, LR_DEFAULTCOLOR,
};

use super::notice::{self, Parts};
use super::{Hit, Layout, Rect, Status};

/// The spec §4 palette, plus the in-between greys of variant E.
pub(super) mod colour {
    pub const PAPER: u32 = 0xFCFCFB;
    pub const FRAME: u32 = 0xF1F1EE;
    pub const GRAPHITE: u32 = 0x1F2328;
    pub const PENCIL: u32 = 0xA4A8B0;
    pub const HAIRLINE: u32 = 0xE3E3DF;
    pub const WAX: u32 = 0xB4442C;
    pub const MENU_TEXT: u32 = 0x2B2F35;
    pub const MENU_HOVER: u32 = 0xE3E3E0;
    pub const DIVIDER: u32 = 0xD8D8D4;
    pub const TAB_TEXT: u32 = 0x4A4F57;
    pub const TAB_SEPARATOR: u32 = 0xD3D3CF;
    pub const TAB_GLYPH: u32 = 0x6B7079;
    pub const PLUS: u32 = 0x555555;
    pub const CAPTION_GLYPH: u32 = 0x333333;
    /// Windows 11's close-button red, and its pressed shade.
    pub const CLOSE_HOVER: u32 = 0xC42B1C;
    pub const CLOSE_PRESSED: u32 = 0xC83C30;
    pub const STATUS_TEXT: u32 = 0x5D626B;
    pub const STATUS_SEPARATOR: u32 = 0xDCDCD8;
    /// Notices (spec §4): amber for news, rose for trouble, and the primary button's ink.
    pub const INK: u32 = 0x2D4BA0;
    pub const AMBER_BACK: u32 = 0xFBF1D9;
    pub const AMBER_BORDER: u32 = 0xEBD9A6;
    pub const AMBER_TEXT: u32 = 0x5B4510;
    pub const ROSE_BACK: u32 = 0xFBE8E5;
    pub const ROSE_BORDER: u32 = 0xEFC6C0;
    pub const ROSE_TEXT: u32 = 0x7A2A20;
}

const UI_FONTS: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];
const ICON_FONTS: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];
const MENU_NAMES: [&str; 3] = ["File", "Edit", "View"];
const GLYPH_MIN: &str = "\u{E921}";
const GLYPH_MAX: &str = "\u{E922}";
const GLYPH_RESTORE: &str = "\u{E923}";
pub(super) const GLYPH_CLOSE: &str = "\u{E8BB}";
const GLYPH_ADD: &str = "\u{E710}";
const GLYPH_LEFT: &str = "\u{E96F}";
const GLYPH_RIGHT: &str = "\u{E970}";
const TAB_RADIUS: f32 = 8.0;
const TAB_TEXT_L: f32 = 12.0;
const TAB_TEXT_GAP: f32 = 8.0;
pub(super) const SMALL_RADIUS: f32 = 4.0;
const DOT: f32 = 8.0;
const CELL_PAD: f32 = 14.0;
const HOVER_A: f32 = 0.06;
const PRESSED_A: f32 = 0.04;

/// One Tab as the title row shows it.
pub struct TabView<'a> {
    pub name: &'a str,
    pub active: bool,
    pub unsaved: bool,
}

/// Everything one paint shows.
pub struct View<'a> {
    pub layout: &'a Layout,
    pub tabs: &'a [TabView<'a>],
    pub status: &'a Status,
    pub hover: Option<Hit>,
    pub pressed: Option<Hit>,
    /// The Window is the foreground window: caption glyphs are darker.
    pub active: bool,
    /// The menu button the keyboard highlights, or whose popup is open.
    pub menu_focus: Option<usize>,
    /// Underline the menus' access keys (Alt held, or the keyboard is on the menu bar).
    pub access_keys: bool,
    /// The active Tab's Notice, drawn in the layout's Notice band.
    pub notice: Option<&'a notice::Spec>,
    /// The find bar's contents, when it is open (12).
    pub find: Option<&'a FindView>,
}

mod find;
pub use find::FindView;

pub(super) struct Fonts {
    menu: IDWriteTextFormat,
    tab: IDWriteTextFormat,
    status: IDWriteTextFormat,
    caption: IDWriteTextFormat,
    pub(super) tab_glyph: IDWriteTextFormat,
    plus: IDWriteTextFormat,
    pub(super) notice: IDWriteTextFormat,
    pub(super) notice_strong: IDWriteTextFormat,
    pub(super) notice_button: IDWriteTextFormat,
    /// The find bar's "Aa".
    mono: IDWriteTextFormat,
}

pub struct Painter {
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    rt: Option<ID2D1DCRenderTarget>,
    ui_family: HSTRING,
    icon_family: HSTRING,
    scale: f32,
    fonts: Fonts,
    hinstance: HINSTANCE,
    /// The app icon at its current pixel size.
    icon: Option<(i32, HICON)>,
}

/// The first installed family of `names`. DirectWrite substitutes a missing
/// family silently, so ask the system collection.
fn font_family(dw: &IDWriteFactory, names: &[&str]) -> HSTRING {
    let coll = unsafe {
        let mut coll = None;
        dw.GetSystemFontCollection(&mut coll, false).ok().and(coll)
    };
    for name in names {
        let h = HSTRING::from(*name);
        let found = coll.as_ref().is_some_and(|c| unsafe {
            let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
            c.FindFamilyName(&h, &mut index, &mut exists).is_ok() && exists.as_bool()
        });
        if found {
            return h;
        }
    }
    HSTRING::from(names[names.len() - 1])
}

/// A single-line, vertically centred, ellipsis-trimmed format `px` pixels tall.
fn text_format(dw: &IDWriteFactory, family: &HSTRING, px: f32, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    text_format_weight(dw, family, px, align, DWRITE_FONT_WEIGHT_NORMAL)
}

fn text_format_weight(
    dw: &IDWriteFactory,
    family: &HSTRING,
    px: f32,
    align: DWRITE_TEXT_ALIGNMENT,
    weight: DWRITE_FONT_WEIGHT,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let f = dw.CreateTextFormat(
            family,
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            px,
            w!("en-us"),
        )?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        f.SetTextAlignment(align)?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        let sign = dw.CreateEllipsisTrimmingSign(&f)?;
        let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
        f.SetTrimming(&trim, &sign)?;
        Ok(f)
    }
}

fn fonts(dw: &IDWriteFactory, ui: &HSTRING, icons: &HSTRING, scale: f32) -> Result<Fonts> {
    let (lead, centre) = (DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_CENTER);
    Ok(Fonts {
        menu: text_format(dw, ui, 12.5 * scale, centre)?,
        tab: text_format(dw, ui, 12.5 * scale, lead)?,
        status: text_format(dw, ui, 12.0 * scale, lead)?,
        caption: text_format(dw, icons, 10.0 * scale, centre)?,
        tab_glyph: text_format(dw, icons, 8.0 * scale, centre)?,
        plus: text_format(dw, icons, 11.0 * scale, centre)?,
        notice: text_format(dw, ui, 12.5 * scale, lead)?,
        notice_strong: text_format_weight(dw, ui, 12.5 * scale, lead, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
        notice_button: text_format_weight(dw, ui, 12.0 * scale, centre, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
        mono: text_format(dw, &font_family(dw, &["Cascadia Mono", "Consolas"]), 11.5 * scale, centre)?,
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn rgb(c: u32, a: f32) -> D2D1_COLOR_F {
    let ch = |shift: u32| ((c >> shift) & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F { r: ch(16), g: ch(8), b: ch(0), a }
}

/// `r` in the coordinates of a pass whose target starts at `oy`.
fn rf(r: Rect, oy: i32) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left as f32, top: (r.top - oy) as f32, right: r.right as f32, bottom: (r.bottom - oy) as f32 }
}

pub(super) fn rounded(rect: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius }
}

fn intersects(a: Rect, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom && a.left < a.right && a.top < a.bottom
}

/// Draws with one target and one pass offset.
pub(super) struct Pass<'a> {
    pub(super) rt: &'a ID2D1DCRenderTarget,
    dw: &'a IDWriteFactory,
    pub(super) fonts: &'a Fonts,
    pub(super) oy: i32,
    pub(super) scale: f32,
}

impl Pass<'_> {
    pub(super) fn brush(&self, c: u32, a: f32) -> Result<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(&rgb(c, a), None) }
    }

    pub(super) fn fill(&self, r: Rect, c: u32, a: f32) -> Result<()> {
        unsafe { self.rt.FillRectangle(&rf(r, self.oy), &self.brush(c, a)?) };
        Ok(())
    }

    pub(super) fn fill_rounded(&self, r: Rect, radius: f32, c: u32, a: f32) -> Result<()> {
        unsafe { self.rt.FillRoundedRectangle(&rounded(rf(r, self.oy), radius * self.scale), &self.brush(c, a)?) };
        Ok(())
    }

    pub(super) fn text(&self, s: &str, f: &IDWriteTextFormat, r: Rect, c: u32) -> Result<()> {
        unsafe {
            self.rt.DrawText(
                &wide(s),
                f,
                &rf(r, self.oy),
                &self.brush(c, 1.0)?,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    }

    /// `text`, with its first character underlined: an access key. The
    /// line goes under that character as DirectWrite lays it out in `r`; if
    /// that can't be measured, the text shows without it.
    fn text_keyed(&self, s: &str, f: &IDWriteTextFormat, r: Rect, c: u32) -> Result<()> {
        self.text(s, f, r, c)?;
        let _ = self.underline_first(s, f, r, c);
        Ok(())
    }

    fn underline_first(&self, s: &str, f: &IDWriteTextFormat, r: Rect, c: u32) -> Result<()> {
        unsafe {
            let layout = self.dw.CreateTextLayout(&wide(s), f, (r.right - r.left) as f32, (r.bottom - r.top) as f32)?;
            let (mut x, mut y) = (0.0, 0.0);
            let mut hit = DWRITE_HIT_TEST_METRICS::default();
            layout.HitTestTextPosition(0, false, &mut x, &mut y, &mut hit)?;
            let mut line = [DWRITE_LINE_METRICS::default()];
            let mut lines = 0;
            layout.GetLineMetrics(Some(&mut line), &mut lines)?;
            let left = r.left + hit.left.round() as i32;
            let top = r.top + (hit.top + line[0].baseline).round() as i32 + self.px(1.5);
            let (right, bottom) = (left + hit.width.round() as i32, top + self.px(1.0).max(1));
            let underline = Rect { left, top, right, bottom };
            self.fill(underline, c, 1.0)
        }
    }

    /// The rounded hover (or pressed) shade of a small button.
    fn button(&self, r: Rect, hit: Hit, v: &View) -> Result<()> {
        if v.pressed == Some(hit) {
            self.fill_rounded(r, SMALL_RADIUS, 0x000000, HOVER_A + PRESSED_A)
        } else if v.hover == Some(hit) {
            self.fill_rounded(r, SMALL_RADIUS, 0x000000, HOVER_A)
        } else {
            Ok(())
        }
    }

    pub(super) fn px(&self, dip: f32) -> i32 {
        (dip * self.scale).round() as i32
    }
}

impl Painter {
    pub fn new(hinstance: HINSTANCE, scale: f32) -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let ui_family = font_family(&dw, &UI_FONTS);
            let icon_family = font_family(&dw, &ICON_FONTS);
            let fonts = fonts(&dw, &ui_family, &icon_family, scale)?;
            Ok(Painter { d2d, dw, rt: None, ui_family, icon_family, scale, fonts, hinstance, icon: None })
        }
    }

    /// Rebuild the text formats for a new DPI.
    pub fn set_scale(&mut self, scale: f32) -> Result<()> {
        if scale != self.scale {
            self.fonts = fonts(&self.dw, &self.ui_family, &self.icon_family, scale)?;
            self.scale = scale;
        }
        Ok(())
    }

    /// Text widths of "File", "Edit", "View" in DIPs, for `Frame::menu_text`.
    pub fn menu_text(&self) -> [f32; 3] {
        MENU_NAMES.map(|m| self.measure(m, &self.fonts.menu) / self.scale)
    }

    /// Where the pieces of `spec` sit in `band`.
    pub fn notice_parts(&self, band: Rect, spec: &notice::Spec) -> Parts {
        let widths: Vec<f32> =
            spec.buttons.iter().map(|b| self.measure(&b.label, &self.fonts.notice_button) / self.scale).collect();
        Parts::new(band, self.scale, &widths, spec.closable)
    }

    fn measure(&self, s: &str, f: &IDWriteTextFormat) -> f32 {
        unsafe {
            let Ok(layout) = self.dw.CreateTextLayout(&wide(s), f, 10_000.0, 1_000.0) else { return 0.0 };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_err() {
                return 0.0;
            }
            m.widthIncludingTrailingWhitespace
        }
    }

    /// Paint the chrome bands that meet `dirty` into the paint DC `hdc`.
    pub fn paint(&mut self, hdc: HDC, dirty: &RECT, v: &View) -> Result<()> {
        let l = v.layout;
        let w = l.status.right;
        let top = Rect { left: 0, top: 0, right: w, bottom: l.editor.top };
        let status_widths: Vec<i32> =
            v.status.cells.iter().map(|c| self.measure(c, &self.fonts.status).ceil() as i32).collect();
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                // Opaque: ClearType text.
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            self.rt = Some(unsafe { self.d2d.CreateDCRenderTarget(&props)? });
        }
        let rt = self.rt.clone().expect("just created");
        let mut result = Ok(());
        for (band, top_pass) in [(top, true), (l.status, false)] {
            if !intersects(band, dirty) {
                continue;
            }
            let pass = Pass { rt: &rt, dw: &self.dw, fonts: &self.fonts, oy: band.top, scale: self.scale };
            let bounds = RECT { left: band.left, top: band.top, right: band.right, bottom: band.bottom };
            result = unsafe {
                rt.BindDC(hdc, &bounds).and_then(|()| {
                    rt.BeginDraw();
                    let drawn = if top_pass { draw_top(&pass, v, self) } else { draw_status(&pass, v, &status_widths) };
                    let ended = rt.EndDraw(None, None);
                    drawn.and(ended)
                })
            };
            if result.is_err() {
                // D2DERR_RECREATE_TARGET and friends: a fresh target next time.
                self.rt = None;
                break;
            }
            if top_pass {
                self.draw_icon(hdc, l.icon);
            }
        }
        result
    }

    /// The app icon, drawn by GDI over the finished pass.
    fn draw_icon(&mut self, hdc: HDC, r: Rect) {
        let size = r.right - r.left;
        if self.icon.is_none_or(|(s, _)| s != size) {
            if let Some((_, old)) = self.icon.take() {
                unsafe {
                    let _ = DestroyIcon(old);
                }
            }
            let loaded = unsafe {
                LoadImageW(Some(self.hinstance), crate::proto::ICON, IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            };
            self.icon = loaded.ok().map(|h| (size, HICON(h.0)));
        }
        if let Some((_, icon)) = self.icon {
            unsafe {
                let _ = DrawIconEx(hdc, r.left, r.top, icon, size, size, 0, None, DI_NORMAL);
            }
        }
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        if let Some((_, icon)) = self.icon.take() {
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }
}

/// The title row, then the Notice band and the find-bar band (empty until 12).
fn draw_top(p: &Pass, v: &View, painter: &Painter) -> Result<()> {
    let l = v.layout;
    let f = p.fonts;
    let title = l.title;
    p.fill(title, colour::FRAME, 1.0)?;
    let hair = p.px(1.0).max(1);
    p.fill(Rect { top: title.bottom - hair, ..title }, colour::HAIRLINE, 1.0)?;

    for (i, (r, name)) in l.menus.iter().zip(MENU_NAMES).enumerate() {
        if v.hover == Some(Hit::Menu(i)) || v.pressed == Some(Hit::Menu(i)) || v.menu_focus == Some(i) {
            p.fill_rounded(*r, SMALL_RADIUS, colour::MENU_HOVER, 1.0)?;
        }
        if v.access_keys {
            p.text_keyed(name, &f.menu, *r, colour::MENU_TEXT)?;
        } else {
            p.text(name, &f.menu, *r, colour::MENU_TEXT)?;
        }
    }
    p.fill(l.divider, colour::DIVIDER, 1.0)?;

    unsafe { p.rt.PushAxisAlignedClip(&rf(l.strip, p.oy), D2D1_ANTIALIAS_MODE_ALIASED) };
    let tabs = draw_tabs(p, v);
    unsafe { p.rt.PopAxisAlignedClip() };
    tabs?;

    // "+", then ‹ › when the Tabs overflow.
    p.button(l.plus, Hit::NewTab, v)?;
    p.text(GLYPH_ADD, &f.plus, l.plus, colour::PLUS)?;
    if let Some(scroll) = l.scroll {
        let (left, right) = l.can_scroll();
        for (r, glyph, hit, enabled) in
            [(scroll[0], GLYPH_LEFT, Hit::ScrollLeft, left), (scroll[1], GLYPH_RIGHT, Hit::ScrollRight, right)]
        {
            if enabled {
                p.button(r, hit, v)?;
            }
            p.text(glyph, &f.caption, r, if enabled { colour::PLUS } else { colour::PENCIL })?;
        }
    }

    // Minimise, maximise (or restore), close.
    let glyphs = [GLYPH_MIN, if l.maximized { GLYPH_RESTORE } else { GLYPH_MAX }, GLYPH_CLOSE];
    for (i, (r, glyph)) in l.buttons.iter().zip(glyphs).enumerate() {
        let hit = [Hit::Minimize, Hit::Maximize, Hit::Close][i];
        let (hover, pressed) = (v.hover == Some(hit), v.pressed == Some(hit));
        let close = hit == Hit::Close;
        if pressed && close {
            p.fill(*r, colour::CLOSE_PRESSED, 1.0)?;
        } else if hover && close {
            p.fill(*r, colour::CLOSE_HOVER, 1.0)?;
        } else if pressed {
            p.fill(*r, 0x000000, PRESSED_A)?;
        } else if hover {
            p.fill(*r, 0x000000, HOVER_A)?;
        }
        let ink = if close && (hover || pressed) {
            0xFFFFFF
        } else if v.active {
            colour::CAPTION_GLYPH
        } else {
            colour::PENCIL
        };
        p.text(glyph, &f.caption, *r, ink)?;
    }

    // Bands 10 and 11 fill the Notice and band 12 the find bar below.
    p.fill(l.notice, colour::PAPER, 1.0)?;
    if let Some(spec) = v.notice {
        let parts = painter.notice_parts(l.notice, spec);
        notice::draw(p, v, spec, &parts, &|s, strong| {
            painter.measure(s, if strong { &f.notice_strong } else { &f.notice })
        })?;
    }
    p.fill(l.find, colour::FRAME, 1.0)?;
    match v.find {
        Some(find) => find::draw(p, find),
        None => Ok(()),
    }
}

/// Windows 11 connected tabs: the active one is paper with 8 px rounded top
/// corners and a hairline border, open at the bottom so it joins the editor.
fn draw_tabs(p: &Pass, v: &View) -> Result<()> {
    let l = v.layout;
    let f = p.fonts;
    let hair = p.px(1.0).max(1) as f32;
    let radius = TAB_RADIUS * p.scale;
    for (i, (tab, close)) in l.tabs.iter().zip(&l.tab_closes).enumerate() {
        let Some(t) = v.tabs.get(i) else { break };
        // Scrolled out of the strip.
        if tab.right <= l.strip.left || tab.left >= l.strip.right {
            continue;
        }
        let hovered = v.hover.and_then(Hit::tab) == Some(i);
        if t.active {
            // One pixel taller than an inactive tab, covering the row's hairline.
            let shape = Rect { top: tab.top - p.px(1.0), ..*tab };
            let r = rf(shape, p.oy);
            unsafe {
                p.rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_ALIASED);
                let below = D2D_RECT_F { bottom: r.bottom + radius, ..r };
                p.rt.FillRoundedRectangle(&rounded(below, radius), &p.brush(colour::PAPER, 1.0)?);
                let h = hair / 2.0;
                let edge = D2D_RECT_F { left: r.left + h, top: r.top + h, right: r.right - h, bottom: r.bottom + radius };
                p.rt.DrawRoundedRectangle(&rounded(edge, radius - h), &p.brush(colour::HAIRLINE, 1.0)?, hair, None);
                p.rt.PopAxisAlignedClip();
            }
        } else {
            let face = Rect { bottom: tab.bottom - p.px(1.0), ..*tab };
            if hovered {
                let r = rf(face, p.oy);
                unsafe {
                    p.rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_ALIASED);
                    let below = D2D_RECT_F { bottom: r.bottom + radius, ..r };
                    p.rt.FillRoundedRectangle(&rounded(below, radius), &p.brush(0x000000, PRESSED_A)?);
                    p.rt.PopAxisAlignedClip();
                }
            }
            let next_active = v.tabs.get(i + 1).is_some_and(|n| n.active);
            if i + 1 < v.tabs.len() && !next_active {
                let x = tab.right + p.px(1.0);
                let top = tab.top + p.px(9.0);
                p.fill(Rect { left: x, top, right: x + hair as i32, bottom: top + p.px(14.0) }, colour::TAB_SEPARATOR, 1.0)?;
            }
        }
        let name = Rect {
            left: tab.left + p.px(TAB_TEXT_L),
            right: close.left - p.px(TAB_TEXT_GAP),
            bottom: tab.bottom - p.px(1.0),
            ..*tab
        };
        p.text(t.name, &f.tab, name, if t.active { colour::GRAPHITE } else { colour::TAB_TEXT })?;

        let close_hover = v.hover == Some(Hit::TabClose(i));
        if t.unsaved && !close_hover {
            let cx = (close.left + close.right) as f32 / 2.0;
            let cy = (close.top + close.bottom) as f32 / 2.0 - p.oy as f32;
            let d = DOT * p.scale / 2.0;
            let dot = D2D_RECT_F { left: cx - d, top: cy - d, right: cx + d, bottom: cy + d };
            unsafe { p.rt.FillRoundedRectangle(&rounded(dot, d), &p.brush(colour::WAX, 1.0)?) };
        } else {
            p.button(*close, Hit::TabClose(i), v)?;
            p.text(GLYPH_CLOSE, &f.tab_glyph, *close, colour::TAB_GLYPH)?;
        }
    }
    Ok(())
}

/// The status bar: the note on the left, cells packed to the right with
/// hairline separators.
fn draw_status(p: &Pass, v: &View, widths: &[i32]) -> Result<()> {
    let l = v.layout;
    let s = l.status;
    p.fill(s, colour::FRAME, 1.0)?;
    let hair = p.px(1.0).max(1);
    p.fill(Rect { bottom: s.top + hair, ..s }, colour::HAIRLINE, 1.0)?;
    let body = Rect { top: s.top + hair, ..s };
    let cells = l.status_cells(widths);
    let pad = p.px(CELL_PAD);
    for (i, (cell, text)) in cells.iter().zip(&v.status.cells).enumerate() {
        if i > 0 {
            p.fill(Rect { right: cell.left + hair, ..Rect { top: body.top, ..*cell } }, colour::STATUS_SEPARATOR, 1.0)?;
        }
        let r = Rect { left: cell.left + pad, right: cell.right - pad + 1, top: body.top, ..*cell };
        p.text(text, &p.fonts.status, r, colour::STATUS_TEXT)?;
    }
    if !v.status.note.is_empty() {
        let note = Rect { top: body.top, ..l.status_note(&cells) };
        p.text(&v.status.note, &p.fonts.status, note, colour::STATUS_TEXT)?;
    }
    Ok(())
}
