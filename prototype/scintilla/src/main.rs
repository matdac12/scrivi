//! Throwaway spike: can Scintilla carry scrivi's Source view?
//!
//! `scrivi-proto.exe <file>` opens one window with a Scintilla child. Markdown
//! files get light styling from pulldown-cmark on a 150 ms debounce. Files over
//! 50 MB open read-only with Notepad4's large-document options.
#![windows_subsystem = "windows"]

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

// ---------------------------------------------------------------- Scintilla

#[link(name = "scintilla", kind = "static")]
extern "C" {
    fn Scintilla_RegisterClasses(hinstance: *mut c_void) -> i32;
}

type SciFnDirect = unsafe extern "C" fn(isize, u32, usize, isize) -> isize;

mod sci {
    pub const SCI_ADDTEXT: u32 = 2001;
    pub const SCI_GETLENGTH: u32 = 2006;
    pub const SCI_SETUNDOCOLLECTION: u32 = 2012;
    pub const SCI_SETSAVEPOINT: u32 = 2014;
    pub const SCI_SETEOLMODE: u32 = 2031;
    pub const SCI_STARTSTYLING: u32 = 2032;
    pub const SCI_SETBUFFEREDDRAW: u32 = 2035;
    pub const SCI_SETTABWIDTH: u32 = 2036;
    pub const SCI_SETCODEPAGE: u32 = 2037;
    pub const SCI_SETMARGINTYPEN: u32 = 2240;
    pub const SCI_SETMARGINWIDTHN: u32 = 2242;
    pub const SCI_STYLECLEARALL: u32 = 2050;
    pub const SCI_STYLESETFORE: u32 = 2051;
    pub const SCI_STYLESETBACK: u32 = 2052;
    pub const SCI_STYLESETITALIC: u32 = 2054;
    pub const SCI_STYLESETFONT: u32 = 2056;
    pub const SCI_STYLESETEOLFILLED: u32 = 2057;
    pub const SCI_STYLESETSIZEFRACTIONAL: u32 = 2061;
    pub const SCI_STYLESETWEIGHT: u32 = 2063;
    pub const SCI_SETELEMENTCOLOUR: u32 = 2753;
    pub const SCI_SETSELECTIONLAYER: u32 = 2763;
    pub const SCI_SETCARETLINELAYER: u32 = 2765;
    pub const SCI_SETSTYLINGEX: u32 = 2073;
    pub const SCI_INDICSETSTYLE: u32 = 2080;
    pub const SCI_INDICSETFORE: u32 = 2082;
    pub const SCI_SETCARETLINEVISIBLE: u32 = 2096;
    pub const SCI_SETHSCROLLBAR: u32 = 2130;
    pub const SCI_GETLINECOUNT: u32 = 2154;
    pub const SCI_SETMARGINLEFT: u32 = 2155;
    pub const SCI_SETMARGINRIGHT: u32 = 2157;
    pub const SCI_SETREADONLY: u32 = 2171;
    pub const SCI_EMPTYUNDOBUFFER: u32 = 2175;
    pub const SCI_GETDIRECTFUNCTION: u32 = 2184;
    pub const SCI_GETDIRECTPOINTER: u32 = 2185;
    pub const SCI_SETCARETWIDTH: u32 = 2188;
    pub const SCI_SETWRAPMODE: u32 = 2268;
    pub const SCI_SETWRAPINDENTMODE: u32 = 2472;
    pub const SCI_SETSCROLLWIDTH: u32 = 2274;
    pub const SCI_SETSCROLLWIDTHTRACKING: u32 = 2516;
    pub const SCI_TEXTWIDTH: u32 = 2276;
    pub const SCI_APPENDTEXT: u32 = 2282;
    pub const SCI_SETDOCPOINTER: u32 = 2358;
    pub const SCI_SETMODEVENTMASK: u32 = 2359;
    pub const SCI_CREATEDOCUMENT: u32 = 2375;
    pub const SCI_RELEASEDOCUMENT: u32 = 2377;
    pub const SCI_ALLOCATE: u32 = 2446;
    pub const SCI_SETINDICATORCURRENT: u32 = 2500;
    pub const SCI_INDICATORFILLRANGE: u32 = 2504;
    pub const SCI_INDICATORCLEARRANGE: u32 = 2505;
    pub const SCI_GETCHARACTERPOINTER: u32 = 2520;
    pub const SCI_SETEXTRAASCENT: u32 = 2525;
    pub const SCI_SETEXTRADESCENT: u32 = 2527;
    pub const SCI_SETMULTIPLESELECTION: u32 = 2563;
    pub const SCI_SETADDITIONALSELECTIONTYPING: u32 = 2565;
    pub const SCI_SETTECHNOLOGY: u32 = 2630;
    pub const SCI_SETLAYOUTTHREADS: u32 = 2775;
    pub const SCI_SETCHANGEHISTORY: u32 = 2780;

    pub const STYLE_DEFAULT: usize = 32;
    pub const STYLE_LINENUMBER: usize = 33;
    pub const SC_CP_UTF8: usize = 65001;
    pub const SC_EOL_CRLF: usize = 0;
    pub const SC_EOL_LF: usize = 2;
    pub const SC_MARGIN_NUMBER: isize = 1;
    pub const SC_TECHNOLOGY_DIRECTWRITE: usize = 1;
    pub const SC_WRAP_NONE: usize = 0;
    pub const SC_WRAP_WORD: usize = 1;
    pub const SC_WRAPINDENT_SAME: usize = 1;
    pub const SC_LAYER_UNDER_TEXT: usize = 1;
    pub const SC_ELEMENT_SELECTION_BACK: usize = 11;
    pub const SC_ELEMENT_SELECTION_INACTIVE_BACK: usize = 17;
    pub const SC_ELEMENT_CARET: usize = 40;
    pub const SC_ELEMENT_CARET_LINE_BACK: usize = 50;
    pub const SC_DOCUMENTOPTION_STYLES_NONE: isize = 0x1;
    pub const SC_DOCUMENTOPTION_TEXT_LARGE: isize = 0x100;
    pub const SC_MOD_INSERTTEXT: i32 = 0x1;
    pub const SC_MOD_DELETETEXT: i32 = 0x2;
    pub const INDIC_STRIKE: isize = 4;
    pub const SCN_STYLENEEDED: u32 = 2000;
    pub const SCN_MODIFIED: u32 = 2008;
    pub const SCN_PAINTED: u32 = 2013;
}
use sci::*;

/// The head of `SCNotification`, up to the field we read.
#[repr(C)]
struct ScNotification {
    hwnd_from: *mut c_void,
    id_from: usize,
    code: u32,
    position: isize,
    ch: i32,
    modifiers: i32,
    modification_type: i32,
}

struct Sci {
    hwnd: HWND,
    func: SciFnDirect,
    ptr: isize,
}

impl Sci {
    fn call(&self, msg: u32, w: usize, l: isize) -> isize {
        unsafe { (self.func)(self.ptr, msg, w, l) }
    }
    fn call_str(&self, msg: u32, w: usize, s: &str) -> isize {
        let z = format!("{s}\0");
        self.call(msg, w, z.as_ptr() as isize)
    }
}

// ---------------------------------------------------------------- look

const fn rgb(c: u32) -> isize {
    // 0xRRGGBB -> Scintilla's 0xBBGGRR
    (((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)) as isize
}
const fn rgba(c: u32, a: u32) -> isize {
    rgb(c) | ((a as isize) << 24)
}

const FONT: &str = "Cascadia Mono";
const FONT_SIZE: usize = 1100; // 11 pt, in hundredths
const BG: u32 = 0xFBFAF8;
const TEXT: u32 = 0x2A2A2E;
const LINENO: u32 = 0xC4C1BA;
const MARKER: u32 = 0xB0ACA3;
const ACCENT: u32 = 0x3D5CC0;

// Style numbers (0..=31 are free for us).
const S_DEFAULT: u8 = 0;
const S_MARKER: u8 = 1;
const S_H1: u8 = 2; // ..=7 for H1..H6
const S_STRONG: u8 = 8;
const S_EM: u8 = 9;
const S_STRONG_EM: u8 = 10;
const S_CODE: u8 = 11;
const S_CODE_MARK: u8 = 12;
const S_BLOCK: u8 = 13;
const S_BLOCK_MARK: u8 = 14;
const S_LINK: u8 = 15;
const S_QUOTE: u8 = 16;
const S_QUOTE_STRONG: u8 = 17;
const S_QUOTE_EM: u8 = 18;
const S_STRIKE: u8 = 19;
const S_HTML: u8 = 20;
const S_TASK_DONE: u8 = 21;
const INDIC_STRIKE_NO: usize = 8;

fn scale(px: f32, dpi: u32) -> isize {
    (px * dpi as f32 / 96.0).round() as isize
}

fn apply_look(s: &Sci, large: bool) {
    // SCRIVI_TECH=0..3 overrides the technology, only to compare memory (0 = GDI).
    let tech = std::env::var("SCRIVI_TECH").ok().and_then(|v| v.parse().ok());
    s.call(SCI_SETTECHNOLOGY, tech.unwrap_or(SC_TECHNOLOGY_DIRECTWRITE), 0);
    if tech != Some(0) {
        s.call(SCI_SETBUFFEREDDRAW, 0, 0);
    }
    s.call(SCI_SETCODEPAGE, SC_CP_UTF8, 0);
    s.call(SCI_SETTABWIDTH, 4, 0);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    s.call(SCI_SETLAYOUTTHREADS, threads, 0);

    s.call_str(SCI_STYLESETFONT, STYLE_DEFAULT, FONT);
    s.call(SCI_STYLESETSIZEFRACTIONAL, STYLE_DEFAULT, FONT_SIZE as isize);
    s.call(SCI_STYLESETFORE, STYLE_DEFAULT, rgb(TEXT));
    s.call(SCI_STYLESETBACK, STYLE_DEFAULT, rgb(BG));
    s.call(SCI_STYLECLEARALL, 0, 0);

    s.call(SCI_STYLESETFORE, STYLE_LINENUMBER, rgb(LINENO));
    s.call(SCI_STYLESETBACK, STYLE_LINENUMBER, rgb(BG));
    s.call(SCI_STYLESETSIZEFRACTIONAL, STYLE_LINENUMBER, 950);

    let fore = |st: u8, c: u32| s.call(SCI_STYLESETFORE, st as usize, rgb(c));
    let back = |st: u8, c: u32| s.call(SCI_STYLESETBACK, st as usize, rgb(c));
    let weight = |st: u8, w: isize| s.call(SCI_STYLESETWEIGHT, st as usize, w);
    let italic = |st: u8| s.call(SCI_STYLESETITALIC, st as usize, 1);
    let eol = |st: u8| s.call(SCI_STYLESETEOLFILLED, st as usize, 1);

    fore(S_MARKER, MARKER);
    for (i, (c, w)) in [
        (0x1B2E7A, 700),
        (0x2A45A3, 700),
        (0x3D5CC0, 600),
        (0x56689A, 600),
        (0x56689A, 600),
        (0x56689A, 600),
    ]
    .into_iter()
    .enumerate()
    {
        fore(S_H1 + i as u8, c);
        weight(S_H1 + i as u8, w);
    }
    fore(S_STRONG, 0x161618);
    weight(S_STRONG, 700);
    italic(S_EM);
    fore(S_STRONG_EM, 0x161618);
    weight(S_STRONG_EM, 700);
    italic(S_STRONG_EM);
    const CODE_BG: u32 = 0xF1EEE8;
    fore(S_CODE, 0x9E3B2C);
    back(S_CODE, CODE_BG);
    fore(S_CODE_MARK, 0xC8B8AE);
    back(S_CODE_MARK, CODE_BG);
    const BLOCK_BG: u32 = 0xF3F1EC;
    fore(S_BLOCK, 0x34353B);
    back(S_BLOCK, BLOCK_BG);
    eol(S_BLOCK);
    fore(S_BLOCK_MARK, 0xB3ADA2);
    back(S_BLOCK_MARK, BLOCK_BG);
    eol(S_BLOCK_MARK);
    fore(S_LINK, 0x2C62D6);
    fore(S_QUOTE, 0x7D7A72);
    fore(S_QUOTE_STRONG, 0x6A675F);
    weight(S_QUOTE_STRONG, 700);
    fore(S_QUOTE_EM, 0x7D7A72);
    italic(S_QUOTE_EM);
    fore(S_STRIKE, 0xA09C94);
    fore(S_HTML, 0x9A8F7E);
    fore(S_TASK_DONE, 0x3E8E5E);
    weight(S_TASK_DONE, 700);

    s.call(SCI_INDICSETSTYLE, INDIC_STRIKE_NO, INDIC_STRIKE);
    s.call(SCI_INDICSETFORE, INDIC_STRIKE_NO, rgb(0x8F8B83));

    s.call(SCI_SETMARGINTYPEN, 0, SC_MARGIN_NUMBER);
    s.call(SCI_SETMARGINWIDTHN, 1, 0);
    s.call(SCI_SETMARGINWIDTHN, 2, 0);

    s.call(SCI_SETCARETLINEVISIBLE, 1, 0);
    s.call(SCI_SETCARETLINELAYER, SC_LAYER_UNDER_TEXT, 0);
    s.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_CARET_LINE_BACK, rgba(ACCENT, 0x0E));
    s.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_CARET, rgba(ACCENT, 0xFF));
    s.call(SCI_SETSELECTIONLAYER, SC_LAYER_UNDER_TEXT, 0);
    s.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_SELECTION_BACK, rgba(ACCENT, 0x38));
    s.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_SELECTION_INACTIVE_BACK, rgba(0x000000, 0x16));
    s.call(SCI_SETMULTIPLESELECTION, 1, 0);
    s.call(SCI_SETADDITIONALSELECTIONTYPING, 1, 0);

    if large {
        s.call(SCI_SETWRAPMODE, SC_WRAP_NONE, 0);
        s.call(SCI_SETSCROLLWIDTH, 1, 0);
        s.call(SCI_SETSCROLLWIDTHTRACKING, 1, 0);
    } else {
        s.call(SCI_SETWRAPMODE, SC_WRAP_WORD, 0);
        s.call(SCI_SETWRAPINDENTMODE, SC_WRAPINDENT_SAME, 0);
        s.call(SCI_SETHSCROLLBAR, 0, 0);
    }
}

/// Everything measured in pixels, which Scintilla does not scale for us.
fn apply_dpi(s: &Sci, dpi: u32) {
    s.call(SCI_SETEXTRAASCENT, scale(3.0, dpi) as usize, 0);
    s.call(SCI_SETEXTRADESCENT, scale(3.0, dpi) as usize, 0);
    s.call(SCI_SETCARETWIDTH, scale(2.0, dpi) as usize, 0);
    s.call(SCI_SETMARGINLEFT, 0, scale(14.0, dpi));
    s.call(SCI_SETMARGINRIGHT, 0, scale(14.0, dpi));
    let lines = s.call(SCI_GETLINECOUNT, 0, 0).max(1);
    let digits = (lines.ilog10() + 1).max(3) as usize;
    let w = s.call_str(SCI_TEXTWIDTH, STYLE_LINENUMBER, &"9".repeat(digits));
    s.call(SCI_SETMARGINWIDTHN, 0, w + scale(24.0, dpi));
}

// ---------------------------------------------------------------- markdown

/// Re-parses the whole text and writes one style byte per text byte.
/// Bytes that no leaf event covers are markdown syntax, so they start as markers.
fn markdown_styles(src: &str, styles: &mut Vec<u8>, strikes: &mut Vec<Range<usize>>) {
    styles.clear();
    styles.resize(src.len(), S_MARKER);
    strikes.clear();
    let opts = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let (mut heading, mut strong, mut em, mut strike, mut link, mut quote) = (0u8, 0, 0, 0, 0, 0);
    let mut block = false;
    let fill = |styles: &mut Vec<u8>, r: Range<usize>, st: u8| styles[r].fill(st);
    for (ev, r) in Parser::new_ext(src, opts).into_offset_iter() {
        let text_style = || {
            if block {
                S_BLOCK
            } else if heading > 0 {
                S_H1 + heading - 1
            } else if link > 0 {
                S_LINK
            } else if strike > 0 {
                S_STRIKE
            } else if quote > 0 {
                match (strong > 0, em > 0) {
                    (true, _) => S_QUOTE_STRONG,
                    (false, true) => S_QUOTE_EM,
                    _ => S_QUOTE,
                }
            } else {
                match (strong > 0, em > 0) {
                    (true, true) => S_STRONG_EM,
                    (true, false) => S_STRONG,
                    (false, true) => S_EM,
                    _ => S_DEFAULT,
                }
            }
        };
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => heading = level as u8,
                Tag::Strong | Tag::TableHead => strong += 1,
                Tag::Emphasis => em += 1,
                Tag::Strikethrough => {
                    strike += 1;
                    strikes.push(r);
                }
                Tag::Link { .. } | Tag::Image { .. } => link += 1,
                Tag::BlockQuote(_) => quote += 1,
                Tag::CodeBlock(_) => {
                    block = true;
                    // Take the closing fence's line end too, so its tint fills the row.
                    let b = src.as_bytes();
                    let mut end = r.end;
                    if b.get(end) == Some(&b'\r') {
                        end += 1;
                    }
                    if b.get(end) == Some(&b'\n') {
                        end += 1;
                    }
                    fill(styles, r.start..end, S_BLOCK_MARK);
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => heading = 0,
                TagEnd::Strong | TagEnd::TableHead => strong -= 1,
                TagEnd::Emphasis => em -= 1,
                TagEnd::Strikethrough => strike -= 1,
                TagEnd::Link | TagEnd::Image => link -= 1,
                TagEnd::BlockQuote(_) => quote -= 1,
                TagEnd::CodeBlock => block = false,
                _ => {}
            },
            Event::Text(_) | Event::SoftBreak => {
                let st = text_style();
                fill(styles, r, st);
            }
            Event::Code(_) => {
                let b = src.as_bytes();
                let ticks = b[r.clone()].iter().take_while(|&&c| c == b'`').count();
                fill(styles, r.clone(), S_CODE);
                fill(styles, r.start..r.start + ticks, S_CODE_MARK);
                fill(styles, r.end.saturating_sub(ticks).max(r.start)..r.end, S_CODE_MARK);
            }
            Event::Html(_) | Event::InlineHtml(_) => fill(styles, r, S_HTML),
            Event::TaskListMarker(true) => fill(styles, r, S_TASK_DONE),
            _ => {} // markers stay markers: rules, breaks, unchecked boxes
        }
    }
}

// ---------------------------------------------------------------- app

struct App {
    main: HWND,
    sci: Sci,
    file: String,
    markdown: bool,
    large: bool,
    log: PathBuf,
    paint_since: Cell<Option<(Instant, &'static str)>>,
    styles: std::cell::RefCell<(Vec<u8>, Vec<Range<usize>>)>,
}

thread_local! {
    static APP: OnceCell<&'static App> = const { OnceCell::new() };
}

fn app() -> Option<&'static App> {
    APP.with(|a| a.get().copied())
}

const TIMER_RESTYLE: usize = 1;
const WM_APP_SIM_EDIT: u32 = WM_APP + 1; // measure.ps1 posts this to time an edit pass
const WM_APP_DPI: u32 = WM_APP + 2;

fn log_line(path: &Path, line: &str) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{ms} {line}");
    }
}

fn restyle(a: &App, reason: &'static str) {
    let t0 = Instant::now();
    let len = a.sci.call(SCI_GETLENGTH, 0, 0) as usize;
    let ptr = a.sci.call(SCI_GETCHARACTERPOINTER, 0, 0) as *const u8;
    if ptr.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(src) = std::str::from_utf8(bytes) else {
        log_line(&a.log, &format!("{} {reason} skipped: not UTF-8", a.file));
        return;
    };
    let mut guard = a.styles.borrow_mut();
    let (styles, strikes) = &mut *guard;
    markdown_styles(src, styles, strikes);
    let t1 = Instant::now();
    a.sci.call(SCI_STARTSTYLING, 0, 0);
    a.sci.call(SCI_SETSTYLINGEX, len, styles.as_ptr() as isize);
    a.sci.call(SCI_SETINDICATORCURRENT, INDIC_STRIKE_NO, 0);
    a.sci.call(SCI_INDICATORCLEARRANGE, 0, len as isize);
    for r in strikes.iter() {
        a.sci.call(SCI_INDICATORFILLRANGE, r.start, r.len() as isize);
    }
    let t2 = Instant::now();
    log_line(
        &a.log,
        &format!(
            "{} style-pass reason={reason} bytes={len} parse_ms={:.2} apply_ms={:.2} total_ms={:.2}",
            a.file,
            (t1 - t0).as_secs_f64() * 1e3,
            (t2 - t1).as_secs_f64() * 1e3,
            (t2 - t0).as_secs_f64() * 1e3
        ),
    );
    a.paint_since.set(Some((t2, reason)));
}

fn load(s: &Sci, path: &Path, large: bool) -> std::io::Result<usize> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len() as usize;
    s.call(SCI_SETUNDOCOLLECTION, 0, 0);
    s.call(SCI_SETCHANGEHISTORY, 0, 0);
    s.call(SCI_ALLOCATE, len + 1, 0);
    let mut buf = vec![0u8; 4 << 20];
    let mut first = true;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let mut chunk = &buf[..n];
        if first {
            first = false;
            if chunk.starts_with(b"\xEF\xBB\xBF") {
                chunk = &chunk[3..];
            }
            let eol = match chunk.iter().position(|&c| c == b'\n') {
                Some(i) if i > 0 && chunk[i - 1] == b'\r' => SC_EOL_CRLF,
                Some(_) => SC_EOL_LF,
                None => SC_EOL_CRLF,
            };
            s.call(SCI_SETEOLMODE, eol, 0);
        }
        s.call(SCI_APPENDTEXT, chunk.len(), chunk.as_ptr() as isize);
    }
    if !large {
        s.call(SCI_SETUNDOCOLLECTION, 1, 0);
    }
    s.call(SCI_EMPTYUNDOBUFFER, 0, 0);
    s.call(SCI_SETSAVEPOINT, 0, 0);
    if large {
        s.call(SCI_SETREADONLY, 1, 0);
    }
    Ok(len)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(a) = app() else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_SIZE => {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let _ = MoveWindow(a.sci.hwnd, 0, 0, rc.right, rc.bottom, true);
            LRESULT(0)
        }
        WM_SETFOCUS => {
            let _ = SetFocus(Some(a.sci.hwnd));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DPICHANGED => {
            let rc = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rc.left,
                rc.top,
                rc.right - rc.left,
                rc.bottom - rc.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            // Scintilla learns the new DPI from WM_DPICHANGED_AFTERPARENT; resize after it.
            let _ = PostMessageW(Some(hwnd), WM_APP_DPI, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_APP_DPI => {
            apply_dpi(&a.sci, GetDpiForWindow(hwnd));
            LRESULT(0)
        }
        WM_APP_SIM_EDIT => {
            if !a.large {
                a.sci.call(SCI_ADDTEXT, 1, b"x".as_ptr() as isize);
            }
            LRESULT(0)
        }
        WM_NOTIFY => {
            let n = &*(lp.0 as *const ScNotification);
            if n.hwnd_from != a.sci.hwnd.0 {
                return LRESULT(0);
            }
            match n.code {
                SCN_MODIFIED if a.markdown => {
                    if n.modification_type & (SC_MOD_INSERTTEXT | SC_MOD_DELETETEXT) != 0 {
                        SetTimer(Some(hwnd), TIMER_RESTYLE, 150, None);
                    }
                }
                SCN_STYLENEEDED => {
                    // We style the whole text ourselves; mark it done so Scintilla stops asking.
                    let len = a.sci.call(SCI_GETLENGTH, 0, 0);
                    a.sci.call(SCI_STARTSTYLING, len as usize, 0);
                }
                SCN_PAINTED => {
                    if let Some((t, reason)) = a.paint_since.take() {
                        log_line(
                            &a.log,
                            &format!(
                                "{} painted-after-style reason={reason} ms={:.2}",
                                a.file,
                                t.elapsed().as_secs_f64() * 1e3
                            ),
                        );
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_RESTYLE => {
            let _ = KillTimer(Some(hwnd), TIMER_RESTYLE);
            restyle(a, "edit");
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() {
    let Some(arg) = std::env::args_os().nth(1) else {
        unsafe {
            MessageBoxW(None, w!("usage: scrivi-proto.exe <file>"), w!("scrivi prototype"), MB_OK);
        }
        return;
    };
    let path = PathBuf::from(arg);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let large = size > 50 * 1024 * 1024;
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    let markdown = !large && matches!(ext.as_deref(), Some("md" | "markdown"));
    let log = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("scrivi-proto.log")))
        .unwrap_or_else(|| PathBuf::from("scrivi-proto.log"));

    unsafe {
        let hinst = GetModuleHandleW(None).unwrap();
        Scintilla_RegisterClasses(hinst.0);
        let class = w!("ScriviProto");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let main = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("scrivi prototype"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap();
        let dpi = GetDpiForWindow(main);
        let _ = SetWindowPos(
            main,
            None,
            0,
            0,
            scale(980.0, dpi) as i32,
            scale(760.0, dpi) as i32,
            SWP_NOMOVE | SWP_NOZORDER,
        );
        // Blend the title bar into the page (Windows 11 only; ignored elsewhere).
        let cap = COLORREF(rgb(BG) as u32);
        let border = COLORREF(rgb(0xE4E1DA) as u32);
        let _ = DwmSetWindowAttribute(main, DWMWA_CAPTION_COLOR, &cap as *const _ as *const c_void, 4);
        let _ = DwmSetWindowAttribute(main, DWMWA_BORDER_COLOR, &border as *const _ as *const c_void, 4);

        let child = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("Scintilla"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
            0,
            0,
            100,
            100,
            Some(main),
            None,
            Some(hinst.into()),
            None,
        )
        .unwrap();
        let func: SciFnDirect =
            std::mem::transmute(SendMessageW(child, SCI_GETDIRECTFUNCTION, None, None).0);
        let ptr = SendMessageW(child, SCI_GETDIRECTPOINTER, None, None).0;
        let s = Sci { hwnd: child, func, ptr };

        if large {
            let doc = s.call(
                SCI_CREATEDOCUMENT,
                0,
                SC_DOCUMENTOPTION_STYLES_NONE | SC_DOCUMENTOPTION_TEXT_LARGE,
            );
            s.call(SCI_SETDOCPOINTER, 0, doc);
            s.call(SCI_RELEASEDOCUMENT, 0, doc);
        }
        s.call(SCI_SETMODEVENTMASK, 0, 0);
        apply_look(&s, large);

        let a: &'static App = Box::leak(Box::new(App {
            main,
            sci: s,
            file: file.clone(),
            markdown,
            large,
            log,
            paint_since: Cell::new(None),
            styles: Default::default(),
        }));
        APP.with(|c| {
            let _ = c.set(a);
        });

        let mut rc = RECT::default();
        let _ = GetClientRect(main, &mut rc);
        let _ = MoveWindow(child, 0, 0, rc.right, rc.bottom, false);
        let _ = ShowWindow(main, SW_SHOW);

        let t0 = Instant::now();
        match load(&a.sci, &path, large) {
            Ok(n) => {
                let title = if large {
                    format!("{file} \u{2014} read-only (large file) \u{2014} scrivi prototype")
                } else {
                    format!("{file} \u{2014} scrivi prototype")
                };
                let _ = SetWindowTextW(a.main, PCWSTR(wide(&title).as_ptr()));
                log_line(
                    &a.log,
                    &format!(
                        "{file} load bytes={n} large={large} ms={:.2}",
                        t0.elapsed().as_secs_f64() * 1e3
                    ),
                );
            }
            Err(e) => {
                let _ = SetWindowTextW(a.main, PCWSTR(wide(&format!("{file}: {e}")).as_ptr()));
            }
        }
        apply_dpi(&a.sci, dpi);
        if markdown {
            restyle(a, "load");
            a.sci.call(SCI_SETMODEVENTMASK, (SC_MOD_INSERTTEXT | SC_MOD_DELETETEXT) as usize, 0);
        }
        let _ = SetFocus(Some(child));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
