//! One Scintilla control with a document per Tab: styles, find, zoom, wrap (spec §5, §11).

use std::ffi::c_void;

#[allow(dead_code)]
#[link(name = "scintilla", kind = "static")]
extern "C" {
    /// Registers the `Scintilla` window class for this module.
    pub fn Scintilla_RegisterClasses(hinstance: *mut c_void) -> i32;
}

/// Signature of the function behind `SCI_GETDIRECTFUNCTION`.
#[allow(dead_code)]
pub type SciFnDirect = unsafe extern "C" fn(isize, u32, usize, isize) -> isize;

/// Scintilla constants, copied by hand from `vendor/scintilla/include/Scintilla.h`.
/// This is the one place they live; add what you need here.
#[allow(dead_code)]
pub mod sci {
    pub const SCI_ADDTEXT: u32 = 2001;
    pub const SCI_GETLENGTH: u32 = 2006;
    pub const SCI_SETUNDOCOLLECTION: u32 = 2012;
    pub const SCI_SETSAVEPOINT: u32 = 2014;
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
    pub const SCI_SETELEMENTCOLOUR: u32 = 2753;
    pub const SCI_SETSELECTIONLAYER: u32 = 2763;
    pub const SCI_SETCARETLINELAYER: u32 = 2765;
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

#[cfg(test)]
mod tests {
    /// Taking the address forces the linker to pull Scintilla in, so a broken
    /// static build fails here instead of in a later issue.
    #[test]
    fn scintilla_is_linked_statically() {
        assert_ne!(super::Scintilla_RegisterClasses as *const () as usize, 0);
    }
}
