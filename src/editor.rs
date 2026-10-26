//! One Scintilla control with a document per Tab: styles, find, zoom, wrap (spec §5, §11).

use std::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SendMessageW, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER, WINDOW_EX_STYLE, WS_CHILD,
    WS_CLIPCHILDREN, WS_VISIBLE,
};

use crate::chrome::Rect;
use crate::textfile::{self, Encoding, Eol};

#[link(name = "scintilla", kind = "static")]
extern "C" {
    /// Registers the `Scintilla` window class for this module.
    pub fn Scintilla_RegisterClasses(hinstance: *mut c_void) -> i32;
}

/// Signature of the function behind `SCI_GETDIRECTFUNCTION`.
pub type SciFnDirect = unsafe extern "C" fn(isize, u32, usize, isize) -> isize;

/// Scintilla constants, copied by hand from `vendor/scintilla/include/Scintilla.h`.
/// This is the one place they live; add what you need here.
#[allow(dead_code)]
pub mod sci {
    pub const SCI_ADDTEXT: u32 = 2001;
    pub const SCI_CLEARALL: u32 = 2004;
    pub const SCI_GETLENGTH: u32 = 2006;
    pub const SCI_GETCURRENTPOS: u32 = 2008;
    pub const SCI_SETUNDOCOLLECTION: u32 = 2012;
    pub const SCI_SETSAVEPOINT: u32 = 2014;
    pub const SCI_GOTOPOS: u32 = 2025;
    pub const SCI_SETEOLMODE: u32 = 2031;
    pub const SCI_STARTSTYLING: u32 = 2032;
    pub const SCI_SETBUFFEREDDRAW: u32 = 2035;
    pub const SCI_SETTABWIDTH: u32 = 2036;
    pub const SCI_SETCODEPAGE: u32 = 2037;
    pub const SCI_STYLECLEARALL: u32 = 2050;
    pub const SCI_STYLESETFORE: u32 = 2051;
    pub const SCI_STYLESETBACK: u32 = 2052;
    pub const SCI_STYLESETITALIC: u32 = 2054;
    pub const SCI_STYLESETFONT: u32 = 2056;
    pub const SCI_STYLESETEOLFILLED: u32 = 2057;
    pub const SCI_STYLESETSIZEFRACTIONAL: u32 = 2061;
    pub const SCI_STYLESETWEIGHT: u32 = 2063;
    pub const SCI_SETSTYLINGEX: u32 = 2073;
    pub const SCI_INDICSETSTYLE: u32 = 2080;
    pub const SCI_INDICSETFORE: u32 = 2082;
    pub const SCI_SETCARETLINEVISIBLE: u32 = 2096;
    pub const SCI_SETHSCROLLBAR: u32 = 2130;
    pub const SCI_GETLINECOUNT: u32 = 2154;
    pub const SCI_SETMARGINLEFT: u32 = 2155;
    pub const SCI_SETMARGINRIGHT: u32 = 2157;
    pub const SCI_GETMODIFY: u32 = 2159;
    pub const SCI_LINEFROMPOSITION: u32 = 2166;
    pub const SCI_POSITIONFROMLINE: u32 = 2167;
    pub const SCI_SETREADONLY: u32 = 2171;
    pub const SCI_EMPTYUNDOBUFFER: u32 = 2175;
    pub const SCI_GETDIRECTFUNCTION: u32 = 2184;
    pub const SCI_GETDIRECTPOINTER: u32 = 2185;
    pub const SCI_SETCARETWIDTH: u32 = 2188;
    pub const SCI_SETMARGINTYPEN: u32 = 2240;
    pub const SCI_SETMARGINWIDTHN: u32 = 2242;
    pub const SCI_SETWRAPMODE: u32 = 2268;
    pub const SCI_SETSCROLLWIDTH: u32 = 2274;
    pub const SCI_TEXTWIDTH: u32 = 2276;
    pub const SCI_APPENDTEXT: u32 = 2282;
    pub const SCI_SETDOCPOINTER: u32 = 2358;
    pub const SCI_SETMODEVENTMASK: u32 = 2359;
    pub const SCI_GETZOOM: u32 = 2374;
    pub const SCI_CREATEDOCUMENT: u32 = 2375;
    pub const SCI_RELEASEDOCUMENT: u32 = 2377;
    pub const SCI_ALLOCATE: u32 = 2446;
    pub const SCI_SETWRAPINDENTMODE: u32 = 2472;
    pub const SCI_SETINDICATORCURRENT: u32 = 2500;
    pub const SCI_INDICATORFILLRANGE: u32 = 2504;
    pub const SCI_INDICATORCLEARRANGE: u32 = 2505;
    pub const SCI_SETSCROLLWIDTHTRACKING: u32 = 2516;
    pub const SCI_GETCHARACTERPOINTER: u32 = 2520;
    pub const SCI_SETEXTRAASCENT: u32 = 2525;
    pub const SCI_SETEXTRADESCENT: u32 = 2527;
    pub const SCI_SETMULTIPLESELECTION: u32 = 2563;
    pub const SCI_SETADDITIONALSELECTIONTYPING: u32 = 2565;
    pub const SCI_SETTECHNOLOGY: u32 = 2630;
    pub const SCI_COUNTCHARACTERS: u32 = 2633;
    pub const SCI_SETELEMENTCOLOUR: u32 = 2753;
    pub const SCI_SETSELECTIONLAYER: u32 = 2763;
    pub const SCI_SETCARETLINELAYER: u32 = 2765;
    pub const SCI_SETLAYOUTTHREADS: u32 = 2775;
    pub const SCI_SETCHANGEHISTORY: u32 = 2780;

    pub const STYLE_DEFAULT: usize = 32;
    pub const STYLE_LINENUMBER: usize = 33;
    pub const SC_CP_UTF8: usize = 65001;
    pub const SC_EOL_CRLF: usize = 0;
    pub const SC_EOL_CR: usize = 1;
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
    pub const SCN_UPDATEUI: u32 = 2007;
    pub const SCN_MODIFIED: u32 = 2008;
    pub const SCN_PAINTED: u32 = 2013;
    pub const SCN_ZOOM: u32 = 2018;
}

use sci::*;

/// The head of `SCNotification` (an `NMHDR`): all `WM_NOTIFY` needs to route it.
#[repr(C)]
pub struct NotifyHeader {
    pub hwnd_from: *mut c_void,
    pub id_from: usize,
    pub code: u32,
}

// The Source view's look: the spec §4 palette, sizes from variant E.
const FONT: &str = "Cascadia Mono";
/// 11 pt, in hundredths.
const FONT_SIZE: isize = 1100;
const LINE_NUMBER_SIZE: isize = 950;
const PAPER: u32 = 0xFCFCFB;
const GRAPHITE: u32 = 0x1F2328;
const PENCIL: u32 = 0xA4A8B0;
const CARET_LINE: u32 = 0xF4F6FB;
const SELECTION: u32 = 0xCFE0FF;

/// 0xRRGGBB to Scintilla's 0xBBGGRR.
const fn rgb(c: u32) -> isize {
    (((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)) as isize
}

/// With an alpha byte, for the element colours.
const fn rgba(c: u32, a: u8) -> isize {
    rgb(c) | ((a as isize) << 24)
}

fn scale(px: f32, dpi: u32) -> isize {
    (px * dpi as f32 / 96.0).round() as isize
}

/// The Window's one Scintilla control. A copyable handle; calls go through
/// Scintilla's direct function. Some of them send `WM_NOTIFY` to the Window
/// synchronously, so never call one while holding a borrow of shared state.
#[derive(Clone, Copy)]
pub struct Editor {
    hwnd: HWND,
    func: SciFnDirect,
    ptr: isize,
}

/// Where the caret is, for the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    /// 1-based.
    pub line: usize,
    /// 1-based, in characters.
    pub col: usize,
}

impl Editor {
    /// Register the `Scintilla` window class; once per process.
    pub fn register_class(hinstance: HINSTANCE) -> bool {
        unsafe { Scintilla_RegisterClasses(hinstance.0) != 0 }
    }

    /// Create the control as a child of `parent`, with the Source view look.
    pub fn create(parent: HWND, hinstance: HINSTANCE, dpi: u32) -> Option<Editor> {
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("Scintilla"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                0,
                0,
                Some(parent),
                None,
                Some(hinstance),
                None,
            )
            .ok()?;
            let func = SendMessageW(hwnd, SCI_GETDIRECTFUNCTION, None, None).0;
            let ptr = SendMessageW(hwnd, SCI_GETDIRECTPOINTER, None, None).0;
            if func == 0 || ptr == 0 {
                return None;
            }
            let func = std::mem::transmute::<isize, SciFnDirect>(func);
            let e = Editor { hwnd, func, ptr };
            e.apply_look();
            e.apply_dpi(dpi);
            Some(e)
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    fn call(&self, msg: u32, w: usize, l: isize) -> isize {
        unsafe { (self.func)(self.ptr, msg, w, l) }
    }

    fn call_str(&self, msg: u32, w: usize, s: &str) -> isize {
        let z = format!("{s}\0");
        self.call(msg, w, z.as_ptr() as isize)
    }

    fn apply_look(&self) {
        self.call(SCI_SETTECHNOLOGY, SC_TECHNOLOGY_DIRECTWRITE, 0);
        self.call(SCI_SETBUFFEREDDRAW, 0, 0);
        self.call(SCI_SETCODEPAGE, SC_CP_UTF8, 0);
        self.call(SCI_SETTABWIDTH, 4, 0);
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        self.call(SCI_SETLAYOUTTHREADS, threads, 0);
        self.call(SCI_SETMODEVENTMASK, 0, 0);

        self.call_str(SCI_STYLESETFONT, STYLE_DEFAULT, FONT);
        self.call(SCI_STYLESETSIZEFRACTIONAL, STYLE_DEFAULT, FONT_SIZE);
        self.call(SCI_STYLESETFORE, STYLE_DEFAULT, rgb(GRAPHITE));
        self.call(SCI_STYLESETBACK, STYLE_DEFAULT, rgb(PAPER));
        self.call(SCI_STYLECLEARALL, 0, 0);
        self.call(SCI_STYLESETFORE, STYLE_LINENUMBER, rgb(PENCIL));
        self.call(SCI_STYLESETBACK, STYLE_LINENUMBER, rgb(PAPER));
        self.call(SCI_STYLESETSIZEFRACTIONAL, STYLE_LINENUMBER, LINE_NUMBER_SIZE);

        // Line numbers in every file; no symbol or fold margin.
        self.call(SCI_SETMARGINTYPEN, 0, SC_MARGIN_NUMBER);
        self.call(SCI_SETMARGINWIDTHN, 1, 0);
        self.call(SCI_SETMARGINWIDTHN, 2, 0);

        self.call(SCI_SETCARETLINEVISIBLE, 1, 0);
        self.call(SCI_SETCARETLINELAYER, SC_LAYER_UNDER_TEXT, 0);
        self.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_CARET_LINE_BACK, rgba(CARET_LINE, 0xFF));
        self.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_CARET, rgba(GRAPHITE, 0xFF));
        self.call(SCI_SETSELECTIONLAYER, SC_LAYER_UNDER_TEXT, 0);
        self.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_SELECTION_BACK, rgba(SELECTION, 0xFF));
        self.call(SCI_SETELEMENTCOLOUR, SC_ELEMENT_SELECTION_INACTIVE_BACK, rgba(0x000000, 0x16));
        self.call(SCI_SETMULTIPLESELECTION, 1, 0);
        self.call(SCI_SETADDITIONALSELECTIONTYPING, 1, 0);

        // Word wrap is on by default; prefs (09) will remember it.
        self.call(SCI_SETWRAPMODE, SC_WRAP_WORD, 0);
        self.call(SCI_SETWRAPINDENTMODE, SC_WRAPINDENT_SAME, 0);
        self.call(SCI_SETHSCROLLBAR, 0, 0);
    }

    /// Everything Scintilla takes in raw pixels. Call once the control has seen
    /// `WM_DPICHANGED_AFTERPARENT`, so `SCI_TEXTWIDTH` measures at the new DPI.
    pub fn apply_dpi(&self, dpi: u32) {
        self.call(SCI_SETEXTRAASCENT, scale(3.0, dpi) as usize, 0);
        self.call(SCI_SETEXTRADESCENT, scale(3.0, dpi) as usize, 0);
        self.call(SCI_SETCARETWIDTH, scale(2.0, dpi) as usize, 0);
        self.call(SCI_SETMARGINLEFT, 0, scale(14.0, dpi));
        self.call(SCI_SETMARGINRIGHT, 0, scale(14.0, dpi));
        self.fit_line_numbers(dpi);
    }

    /// Digits the number margin is sized for: the line count's, three at least.
    pub fn line_digits(&self) -> u32 {
        let lines = self.call(SCI_GETLINECOUNT, 0, 0).max(1);
        (lines.ilog10() + 1).max(3)
    }

    /// Size the number margin to `line_digits`.
    pub fn fit_line_numbers(&self, dpi: u32) {
        let digits = self.line_digits() as usize;
        let w = self.call_str(SCI_TEXTWIDTH, STYLE_LINENUMBER, &"9".repeat(digits));
        self.call(SCI_SETMARGINWIDTHN, 0, w + scale(24.0, dpi));
    }

    /// Replace the text with `bytes`, unmodified, caret at the start. Until 08
    /// every file is read as UTF-8; returns what was found.
    pub fn load_utf8(&self, bytes: &[u8]) -> (Encoding, Eol) {
        let (bytes, encoding) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
            Some(rest) => (rest, Encoding::Utf8Bom),
            None => (bytes, Encoding::Utf8),
        };
        let text = String::from_utf8_lossy(bytes);
        let eol = textfile::detect_eol(&text);
        let mode = match eol {
            Eol::Lf => SC_EOL_LF,
            Eol::Crlf => SC_EOL_CRLF,
            Eol::Cr => SC_EOL_CR,
        };
        self.call(SCI_SETEOLMODE, mode, 0);
        self.call(SCI_SETUNDOCOLLECTION, 0, 0);
        self.call(SCI_CLEARALL, 0, 0);
        self.call(SCI_ALLOCATE, text.len() + 1, 0);
        self.call(SCI_ADDTEXT, text.len(), text.as_ptr() as isize);
        self.call(SCI_SETUNDOCOLLECTION, 1, 0);
        self.call(SCI_EMPTYUNDOBUFFER, 0, 0);
        self.call(SCI_SETSAVEPOINT, 0, 0);
        self.call(SCI_GOTOPOS, 0, 0);
        (encoding, eol)
    }

    /// Edited since the last load (Scintilla's save point).
    pub fn is_modified(&self) -> bool {
        self.call(SCI_GETMODIFY, 0, 0) != 0
    }

    pub fn caret(&self) -> Caret {
        let pos = self.call(SCI_GETCURRENTPOS, 0, 0);
        let line = self.call(SCI_LINEFROMPOSITION, pos as usize, 0);
        let start = self.call(SCI_POSITIONFROMLINE, line as usize, 0);
        let col = self.call(SCI_COUNTCHARACTERS, start as usize, pos);
        Caret { line: line as usize + 1, col: col as usize + 1 }
    }

    /// Zoom as a percentage of the base font size: 100 when not zoomed.
    pub fn zoom_percent(&self) -> i32 {
        // SCI_GETZOOM is the number of points added to every style.
        let points = self.call(SCI_GETZOOM, 0, 0) as i32;
        (FONT_SIZE as i32 + points * 100) * 100 / FONT_SIZE as i32
    }

    pub fn move_to(&self, r: Rect) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    /// Taking the address forces the linker to pull Scintilla in, so a broken
    /// static build fails here instead of in a later issue.
    #[test]
    fn scintilla_is_linked_statically() {
        assert_ne!(super::Scintilla_RegisterClasses as *const () as usize, 0);
    }
}
