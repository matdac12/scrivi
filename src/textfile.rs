//! Pure decode and encode of Text files: encodings, line endings, Large files (spec §7).
//!
//! No I/O and no Win32: the caller reads and writes the bytes and passes the
//! system ANSI code page (from `GetACP()`) to [`decode`].
//!
//! The detection order and the NUL-pattern test for BOM-less UTF-16 follow the
//! approach of Notepad4 (BSD-3, see `THIRD_PARTY_NOTICES.md`); the code is
//! written for this crate.

use std::fmt;

use encoding_rs::{EncoderResult, UTF_16BE, UTF_16LE, UTF_8, WINDOWS_1252};

/// A file over this many bytes is a Large file: read-only, no styling.
pub const LARGE_FILE_BYTES: u64 = 50 * 1024 * 1024;

/// True when a file of `len` bytes opens as a Large file.
pub fn is_large(len: u64) -> bool {
    len > LARGE_FILE_BYTES
}

/// How a Text file is stored on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    /// Written with a BOM. A BOM-less UTF-16 file decodes as this too.
    Utf16Le,
    /// Written with a BOM.
    Utf16Be,
    Windows1252,
    /// A legacy code page found by detection or the system ANSI code page
    /// (Shift_JIS, windows-1251, ...). Not in the Encoding menu.
    Other(&'static encoding_rs::Encoding),
}

impl Encoding {
    /// The five encodings the Encoding menu offers.
    pub const MENU: [Encoding; 5] = [
        Encoding::Utf8,
        Encoding::Utf8Bom,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
        Encoding::Windows1252,
    ];

    /// The label shown in the menu and the status bar.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252 (ANSI)",
            Encoding::Other(e) => e.name(),
        }
    }

    fn from_legacy(e: &'static encoding_rs::Encoding) -> Encoding {
        if e == WINDOWS_1252 {
            Encoding::Windows1252
        } else {
            Encoding::Other(e)
        }
    }
}

impl Default for Encoding {
    /// An Untitled tab starts as UTF-8 without BOM.
    fn default() -> Self {
        Encoding::Utf8
    }
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Line ending of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    Cr,
}

impl Eol {
    pub const MENU: [Eol; 3] = [Eol::Lf, Eol::Crlf, Eol::Cr];

    /// The label shown in the menu and the status bar.
    pub fn name(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::Crlf => "CRLF",
            Eol::Cr => "CR",
        }
    }

    /// The characters themselves.
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
            Eol::Cr => "\r",
        }
    }
}

impl Default for Eol {
    /// An Untitled tab starts with LF.
    fn default() -> Self {
        Eol::Lf
    }
}

impl fmt::Display for Eol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What an Untitled tab starts with.
pub const NEW_FILE: (Encoding, Eol) = (Encoding::Utf8, Eol::Lf);

/// The result of [`decode`]. `text` has no BOM and keeps its original line
/// endings; use [`convert_eol`] to normalise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    pub encoding: Encoding,
    pub eol: Eol,
    /// Some bytes were not valid in `encoding` and became U+FFFD: saving the
    /// text back would change bytes the user never touched.
    pub lossy: bool,
}

/// Characters the chosen encoding cannot represent, each listed once, in
/// order of first appearance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unrepresentable {
    pub encoding: Encoding,
    pub chars: Vec<char>,
}

impl fmt::Display for Unrepresentable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} cannot represent {} character(s)",
            self.encoding,
            self.chars.len()
        )
    }
}

impl std::error::Error for Unrepresentable {}

/// Turn file bytes into text, detecting the encoding and the line ending.
///
/// Order: BOM, NUL pattern of BOM-less UTF-16, strict UTF-8, chardetng, then
/// the system ANSI code page `ansi_code_page` (what chardetng falls back to
/// when it only guesses Western European).
pub fn decode(bytes: &[u8], ansi_code_page: u16) -> Decoded {
    let (text, encoding, lossy) = decode_text(bytes, ansi_code_page);
    let eol = detect_eol(&text);
    Decoded {
        text,
        encoding,
        eol,
        lossy,
    }
}

/// [`decode`] for bytes the caller no longer needs: valid UTF-8 without a BOM
/// (most files, and every Large file worth the name) becomes the text without
/// a copy.
pub fn decode_owned(bytes: Vec<u8>, ansi_code_page: u16) -> Decoded {
    let plain = !bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        && !bytes.starts_with(&[0xFF, 0xFE])
        && !bytes.starts_with(&[0xFE, 0xFF])
        && sniff_utf16_without_bom(&bytes).is_none();
    if plain {
        match String::from_utf8(bytes) {
            Ok(text) => {
                let eol = detect_eol(&text);
                return Decoded { text, encoding: Encoding::Utf8, eol, lossy: false };
            }
            Err(e) => return decode(e.as_bytes(), ansi_code_page),
        }
    }
    decode(&bytes, ansi_code_page)
}

/// Re-read `bytes` in a chosen encoding (Reopen with encoding…). Malformed
/// sequences become U+FFFD. A matching BOM is stripped.
pub fn decode_as(bytes: &[u8], encoding: Encoding) -> Decoded {
    let (text, lossy) = match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes))
        }
        Encoding::Utf16Le => {
            let b = bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes);
            with_replacement(UTF_16LE, b)
        }
        Encoding::Utf16Be => {
            let b = bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes);
            with_replacement(UTF_16BE, b)
        }
        Encoding::Windows1252 => with_replacement(WINDOWS_1252, bytes),
        Encoding::Other(e) => with_replacement(e, bytes),
    };
    let eol = detect_eol(&text);
    Decoded {
        text,
        encoding,
        eol,
        lossy,
    }
}

/// `bytes` as UTF-8, malformed sequences replaced; and whether any were.
fn utf8_lossy(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), false),
        Err(_) => (String::from_utf8_lossy(bytes).into_owned(), true),
    }
}

/// `bytes` in `codec`, malformed sequences replaced; and whether any were.
fn with_replacement(codec: &'static encoding_rs::Encoding, bytes: &[u8]) -> (String, bool) {
    let (s, lossy) = codec.decode_without_bom_handling(bytes);
    (s.into_owned(), lossy)
}

fn decode_text(bytes: &[u8], ansi_code_page: u16) -> (String, Encoding, bool) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let (s, lossy) = utf8_lossy(rest);
        return (s, Encoding::Utf8Bom, lossy);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (s, lossy) = with_replacement(UTF_16LE, rest);
        return (s, Encoding::Utf16Le, lossy);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (s, lossy) = with_replacement(UTF_16BE, rest);
        return (s, Encoding::Utf16Be, lossy);
    }
    if let Some(enc) = sniff_utf16_without_bom(bytes) {
        let codec = if enc == Encoding::Utf16Le {
            UTF_16LE
        } else {
            UTF_16BE
        };
        if let Some(s) = codec.decode_without_bom_handling_and_without_replacement(bytes) {
            return (s.into_owned(), enc, false);
        }
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_owned(), Encoding::Utf8, false);
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let guess = detector.guess(None, chardetng::Utf8Detection::Deny);
    let legacy = if guess == WINDOWS_1252 {
        // chardetng's "no idea, Western" answer: trust the user's code page.
        codepage::to_encoding(ansi_code_page)
            // A UTF-8 or UTF-16 "ANSI" page (65001, 1200, 1201) is no legacy page.
            .filter(|e| ![UTF_8, UTF_16LE, UTF_16BE].contains(e))
            .unwrap_or(WINDOWS_1252)
    } else {
        guess
    };
    let (s, lossy) = with_replacement(legacy, bytes);
    (s, Encoding::from_legacy(legacy), lossy)
}

/// BOM-less UTF-16 shows as NUL bytes at every other position. Latin text has
/// a zero high byte on (almost) every character; require no NUL on the other
/// parity and at least a third of the units to be NUL-high.
fn sniff_utf16_without_bom(bytes: &[u8]) -> Option<Encoding> {
    let pairs = bytes.len() / 2;
    if pairs == 0 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    // Text without a single NUL is not UTF-16 (a fast memchr, not a count).
    if !bytes.contains(&0) {
        return None;
    }
    let (mut even_nul, mut odd_nul) = (0usize, 0usize);
    for (i, &b) in bytes.iter().enumerate() {
        if b == 0 {
            if i % 2 == 0 {
                even_nul += 1;
            } else {
                odd_nul += 1;
            }
        }
    }
    if even_nul == 0 && odd_nul * 3 >= pairs {
        Some(Encoding::Utf16Le)
    } else if odd_nul == 0 && even_nul * 3 >= pairs {
        Some(Encoding::Utf16Be)
    } else {
        None
    }
}

/// The line ending used most; ties go to LF, then CRLF, then CR. A text with
/// no line break is LF.
pub fn detect_eol(text: &str) -> Eol {
    let b = text.as_bytes();
    // No CR at all (a fast memchr): every break is an LF.
    if !b.contains(&b'\r') {
        return Eol::Lf;
    }
    let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    if lf >= crlf && lf >= cr {
        Eol::Lf
    } else if crlf >= cr {
        Eol::Crlf
    } else {
        Eol::Cr
    }
}

/// Rewrite every line ending (LF, CRLF or CR) to `eol`.
pub fn convert_eol(text: &str, eol: Eol) -> String {
    let target = eol.as_str();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(target);
            }
            '\n' => out.push_str(target),
            _ => out.push(c),
        }
    }
    out
}

/// Turn text into file bytes: line endings converted to `eol`, then written in
/// `encoding` (with a BOM for UTF-8 with BOM and UTF-16). Fails, writing
/// nothing, if the encoding cannot represent some characters.
pub fn encode(text: &str, encoding: Encoding, eol: Eol) -> Result<Vec<u8>, Unrepresentable> {
    let text = convert_eol(text, eol);
    match encoding {
        Encoding::Utf8 => Ok(text.into_bytes()),
        Encoding::Utf8Bom => {
            let mut out = Vec::with_capacity(text.len() + 3);
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            out.extend_from_slice(text.as_bytes());
            Ok(out)
        }
        Encoding::Utf16Le => Ok(encode_utf16(&text, &[0xFF, 0xFE], u16::to_le_bytes)),
        Encoding::Utf16Be => Ok(encode_utf16(&text, &[0xFE, 0xFF], u16::to_be_bytes)),
        Encoding::Windows1252 => encode_legacy(&text, WINDOWS_1252, encoding),
        Encoding::Other(e) => encode_legacy(&text, e, encoding),
    }
}

fn encode_utf16(text: &str, bom: &[u8; 2], unit: fn(u16) -> [u8; 2]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + text.len() * 2);
    out.extend_from_slice(bom);
    for u in text.encode_utf16() {
        out.extend_from_slice(&unit(u));
    }
    out
}

fn encode_legacy(
    text: &str,
    codec: &'static encoding_rs::Encoding,
    encoding: Encoding,
) -> Result<Vec<u8>, Unrepresentable> {
    // encoding_rs only encodes to UTF-8 for UTF-16 codecs; ours never get here.
    let mut encoder = codec.new_encoder();
    let cap = encoder
        .max_buffer_length_from_utf8_without_replacement(text.len())
        .unwrap_or(text.len() * 4 + 16);
    let mut buf = vec![0u8; cap];
    let (mut pos, mut written) = (0usize, 0usize);
    let mut bad: Vec<char> = Vec::new();
    loop {
        let (result, read, wrote) =
            encoder.encode_from_utf8_without_replacement(&text[pos..], &mut buf[written..], true);
        pos += read;
        written += wrote;
        match result {
            EncoderResult::InputEmpty => break,
            EncoderResult::Unmappable(c) => {
                if !bad.contains(&c) {
                    bad.push(c);
                }
            }
            EncoderResult::OutputFull => buf.resize(buf.len() * 2 + 16, 0),
        }
    }
    if bad.is_empty() {
        buf.truncate(written);
        Ok(buf)
    } else {
        Err(Unrepresentable {
            encoding,
            chars: bad,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITALIAN: &str = "Perché la città è già così: più, né, ò, ù.\n";

    fn roundtrip(text: &str, enc: Encoding, eol: Eol) -> Decoded {
        let bytes = encode(text, enc, eol).unwrap();
        decode(&bytes, 1252)
    }

    #[test]
    fn utf8_without_bom() {
        let d = roundtrip(ITALIAN, Encoding::Utf8, Eol::Lf);
        assert_eq!(
            (d.text.as_str(), d.encoding, d.eol),
            (ITALIAN, Encoding::Utf8, Eol::Lf)
        );
    }

    #[test]
    fn utf8_with_bom_is_stripped_and_restored() {
        let bytes = encode(ITALIAN, Encoding::Utf8Bom, Eol::Lf).unwrap();
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        let d = decode(&bytes, 1252);
        assert_eq!((d.text.as_str(), d.encoding), (ITALIAN, Encoding::Utf8Bom));
    }

    #[test]
    fn utf16_le_and_be_with_bom() {
        for (enc, bom) in [
            (Encoding::Utf16Le, [0xFF, 0xFE]),
            (Encoding::Utf16Be, [0xFE, 0xFF]),
        ] {
            let bytes = encode("Ciao 😀 è\r\nmondo", enc, Eol::Crlf).unwrap();
            assert_eq!(&bytes[..2], &bom);
            let d = decode(&bytes, 1252);
            assert_eq!(d.text, "Ciao 😀 è\r\nmondo");
            assert_eq!((d.encoding, d.eol), (enc, Eol::Crlf));
        }
    }

    #[test]
    fn utf16_le_without_bom() {
        let bytes: Vec<u8> = "Ciao, è tutto\nok"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let d = decode(&bytes, 1252);
        assert_eq!(d.text, "Ciao, è tutto\nok");
        assert_eq!(d.encoding, Encoding::Utf16Le);
    }

    #[test]
    fn utf16_be_without_bom() {
        let bytes: Vec<u8> = "Ciao, è tutto\nok"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        let d = decode(&bytes, 1252);
        assert_eq!(d.text, "Ciao, è tutto\nok");
        assert_eq!(d.encoding, Encoding::Utf16Be);
    }

    #[test]
    fn windows_1252_with_accented_italian() {
        let bytes = encode(ITALIAN, Encoding::Windows1252, Eol::Lf).unwrap();
        assert!(std::str::from_utf8(&bytes).is_err());
        let d = decode(&bytes, 1252);
        assert_eq!(
            (d.text.as_str(), d.encoding),
            (ITALIAN, Encoding::Windows1252)
        );
    }

    #[test]
    fn other_legacy_code_page_is_detected_and_roundtrips() {
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("これは日本語のテキストです。\n");
        let d = decode(&bytes, 1252);
        assert_eq!(d.encoding, Encoding::Other(encoding_rs::SHIFT_JIS));
        assert_eq!(d.text, "これは日本語のテキストです。\n");
        assert_eq!(encode(&d.text, d.encoding, d.eol).unwrap(), bytes.to_vec());
    }

    #[test]
    fn ansi_code_page_decides_when_detection_is_unsure() {
        // 0xE0 0xE8 0xEC is "àèì" in 1252 and "janei" in 1251: only the code page tells.
        let d = decode(&[b'a', 0xE0, b' ', 0xE8, b' ', 0xEC], 1251);
        assert_eq!(d.encoding, Encoding::Other(encoding_rs::WINDOWS_1251));
        let d = decode(&[b'a', 0xE0, b' ', 0xE8, b' ', 0xEC], 1252);
        assert_eq!(d.encoding, Encoding::Windows1252);
    }

    #[test]
    fn utf8_or_utf16_ansi_code_page_falls_back_to_1252() {
        for cp in [65001, 1200, 1201] {
            let d = decode(&[b'a', 0xE0, b' ', 0xE8, b' ', 0xEC], cp);
            assert_eq!(d.encoding, Encoding::Windows1252);
        }
    }

    #[test]
    fn empty_file() {
        let d = decode(&[], 1252);
        assert_eq!(
            d,
            Decoded {
                text: String::new(),
                encoding: Encoding::Utf8,
                eol: Eol::Lf,
                lossy: false,
            }
        );
        assert_eq!(
            encode("", Encoding::Utf8, Eol::Lf).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn mixed_eol_takes_the_majority() {
        assert_eq!(detect_eol("a\r\nb\r\nc\nd"), Eol::Crlf);
        assert_eq!(detect_eol("a\nb\nc\r\nd"), Eol::Lf);
        assert_eq!(detect_eol("a\rb\rc\nd"), Eol::Cr);
        assert_eq!(detect_eol("no breaks"), Eol::Lf);
        assert_eq!(decode(b"a\r\nb\r\nc\n", 1252).eol, Eol::Crlf);
    }

    #[test]
    fn encode_converts_mixed_text_to_one_eol() {
        let bytes = encode("a\r\nb\nc\rd", Encoding::Utf8, Eol::Crlf).unwrap();
        assert_eq!(bytes, b"a\r\nb\r\nc\r\nd");
        assert_eq!(convert_eol("a\r\nb\nc\rd", Eol::Cr), "a\rb\rc\rd");
        assert_eq!(convert_eol("a\r\n\r\nb", Eol::Lf), "a\n\nb");
    }

    #[test]
    fn emoji_in_windows_1252_is_unrepresentable() {
        let err = encode("ok 😀 è 😀 ✓", Encoding::Windows1252, Eol::Lf).unwrap_err();
        assert_eq!(err.chars, vec!['😀', '✓']);
        assert_eq!(err.encoding, Encoding::Windows1252);
    }

    #[test]
    fn unrepresentable_in_other_code_page() {
        let err = encode("あ", Encoding::Other(encoding_rs::SHIFT_JIS), Eol::Lf);
        assert!(err.is_ok());
        let err = encode("😀", Encoding::Other(encoding_rs::WINDOWS_1251), Eol::Lf).unwrap_err();
        assert_eq!(err.chars, vec!['😀']);
    }

    #[test]
    fn decode_as_rereads_in_the_chosen_encoding() {
        let bytes = encode(ITALIAN, Encoding::Windows1252, Eol::Lf).unwrap();
        assert_eq!(decode_as(&bytes, Encoding::Windows1252).text, ITALIAN);
        assert!(decode_as(&bytes, Encoding::Utf8).text.contains('\u{FFFD}'));
        let bom = encode("x", Encoding::Utf8Bom, Eol::Lf).unwrap();
        assert_eq!(decode_as(&bom, Encoding::Utf8).text, "x");
    }

    #[test]
    fn clean_files_are_not_lossy() {
        let bytes = encode(ITALIAN, Encoding::Windows1252, Eol::Lf).unwrap();
        assert!(!decode(&bytes, 1252).lossy);
        assert!(!decode_as(&bytes, Encoding::Windows1252).lossy);
        for enc in Encoding::MENU {
            let bytes = encode("Ciao è\r\nmondo", enc, Eol::Crlf).unwrap();
            assert!(!decode(&bytes, 1252).lossy, "{enc}");
            assert!(!decode_as(&bytes, enc).lossy, "{enc}");
        }
        assert!(!decode(&[], 1252).lossy);
    }

    #[test]
    fn malformed_bytes_under_a_bom_are_lossy() {
        // A UTF-8 BOM, then a lone 0xFF.
        let d = decode(&[0xEF, 0xBB, 0xBF, b'a', 0xFF, b'b'], 1252);
        assert_eq!((d.encoding, d.lossy), (Encoding::Utf8Bom, true));
        assert_eq!(d.text, "a\u{FFFD}b");
        // A UTF-16 LE BOM, then an odd byte count.
        let d = decode(&[0xFF, 0xFE, b'a', 0, b'b'], 1252);
        assert_eq!((d.encoding, d.lossy), (Encoding::Utf16Le, true));
        // A UTF-16 BE BOM, then a lone high surrogate.
        let d = decode(&[0xFE, 0xFF, 0xD8, 0x3D, 0, b'a'], 1252);
        assert_eq!((d.encoding, d.lossy), (Encoding::Utf16Be, true));
    }

    #[test]
    fn reopening_in_an_encoding_that_does_not_fit_is_lossy() {
        let bytes = encode(ITALIAN, Encoding::Windows1252, Eol::Lf).unwrap();
        assert!(decode_as(&bytes, Encoding::Utf8).lossy);
        assert!(!decode_as(&[b'a', 0], Encoding::Utf16Le).lossy);
        assert!(decode_as(b"a", Encoding::Utf16Le).lossy);
    }

    #[test]
    fn a_legacy_page_with_bytes_it_does_not_map_is_lossy() {
        // 0x82 0x20 is no Shift_JIS character.
        let d = decode_as(&[b'a', 0x82, 0x20], Encoding::Other(encoding_rs::SHIFT_JIS));
        assert!(d.lossy);
    }

    #[test]
    fn large_file_threshold() {
        assert!(!is_large(50 * 1024 * 1024));
        assert!(is_large(50 * 1024 * 1024 + 1));
        assert!(!is_large(0));
    }

    #[test]
    fn new_files_are_utf8_lf_and_names_match_the_menus() {
        assert_eq!(NEW_FILE, (Encoding::default(), Eol::default()));
        let names: Vec<_> = Encoding::MENU.iter().map(|e| e.to_string()).collect();
        assert_eq!(
            names,
            [
                "UTF-8",
                "UTF-8 with BOM",
                "UTF-16 LE",
                "UTF-16 BE",
                "Windows-1252 (ANSI)"
            ]
        );
        let eols: Vec<_> = Eol::MENU.iter().map(|e| e.to_string()).collect();
        assert_eq!(eols, ["LF", "CRLF", "CR"]);
    }

    #[test]
    fn decode_owned_matches_decode() {
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("caff\u{e8}\r\n".encode_utf16().flat_map(u16::to_le_bytes));
        let samples: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"plain\nlines\n".to_vec(),
            b"dos\r\nlines\r\n".to_vec(),
            "caff\u{e8}\n".as_bytes().to_vec(),
            [&[0xEF, 0xBB, 0xBF][..], b"bom\r\n"].concat(),
            utf16,
            b"caff\xe8 \x80\n".to_vec(),
            b"a\0b\0c\0".to_vec(),
        ];
        for bytes in samples {
            assert_eq!(decode_owned(bytes.clone(), 1252), decode(&bytes, 1252), "{bytes:?}");
        }
    }

    #[test]
    fn a_text_with_no_cr_is_lf_and_a_lone_cr_still_counts() {
        assert_eq!(detect_eol("a\nb\nc"), Eol::Lf);
        assert_eq!(detect_eol("a\rb\rc\n"), Eol::Cr);
        assert_eq!(detect_eol("a\r\nb\r\nc\n"), Eol::Crlf);
    }
}
