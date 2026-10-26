//! Pure markdown text to style runs for the Source view (spec §5).
//!
//! `style_runs` parses the whole text and returns UTF-8 byte ranges, each with
//! one [`Style`]. The runs are sorted, never overlap, and always start and end
//! on char boundaries, so the Source view can walk them once with `SCI_SETSTYLING`.
//!
//! Bytes that no run covers are plain text: the Source view styles the gaps with
//! its default style. Everything the parser reports as syntax rather than text
//! (`#`, `**`, backticks, `>`, bullets, `[ ]`, table pipes, a link's brackets,
//! and also the blank lines, indentation and HTML between blocks) is
//! [`Style::Marker`].
//!
//! # Nesting
//!
//! Every byte gets exactly one style, so nested spans are resolved here. The
//! first rule that applies to a text byte wins:
//!
//! 1. code block, then inline code (nothing nests inside code);
//! 2. autolink or bare URL (`Url`);
//! 3. heading: bold, italic and strikethrough inside a heading stay `Heading`;
//! 4. link text: bold or italic inside a link stay `Link`;
//! 5. strikethrough;
//! 6. quote: bold and italic inside a quote stay `Quote`;
//! 7. bold and italic, which combine into `BoldItalic`.
//!
//! The syntax characters around any span are always `Marker`, whatever the
//! span's own style.
//!
//! # For other modules
//!
//! - Spell check (14) skips `CodeBlock`, `InlineCode`, `Url` and `Marker`
//!   (see [`Style::is_spell_checked`]).

#![allow(dead_code)]

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

/// Above this many bytes a markdown file gets no styling (spec §5, the 1 MB rule).
pub const STYLING_LIMIT: usize = 1024 * 1024;

/// What a run of text looks like. `Plain` text has no run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Never returned in a run; used while building.
    Plain,
    /// Heading level, 1..=6.
    Heading(u8),
    Bold,
    Italic,
    BoldItalic,
    Strike,
    InlineCode,
    /// A whole code block: fences, info string, text, and the closing fence's
    /// line ending.
    CodeBlock,
    /// Link text, and image alt text.
    Link,
    /// The address of a link or image, an autolink, or a bare URL.
    Url,
    Quote,
    /// The `[x]` of a done task.
    TaskDone,
    /// Markdown syntax characters.
    Marker,
}

impl Style {
    /// False for styles whose text is code, an address or syntax.
    pub fn is_spell_checked(self) -> bool {
        !matches!(
            self,
            Style::InlineCode | Style::CodeBlock | Style::Url | Style::Marker
        )
    }
}

/// Whether markdown styling applies to a text of `len` bytes.
pub fn styling_enabled(len: usize) -> bool {
    len <= STYLING_LIMIT
}

/// Sorted, non-overlapping style runs over `text`, as UTF-8 byte ranges.
pub fn style_runs(text: &str) -> Vec<(Range<usize>, Style)> {
    let mut styles = vec![Style::Marker; text.len()];
    paint_markdown(text, &mut styles);
    paint_bare_urls(text, &mut styles);
    collect_runs(&styles)
}

/// What the enclosing spans make of a leaf of text.
#[derive(Default)]
struct Context {
    heading: u8,
    bold: u32,
    italic: u32,
    strike: u32,
    link: u32,
    autolink: u32,
    quote: u32,
    code_block: bool,
}

impl Context {
    fn text_style(&self) -> Style {
        if self.code_block {
            Style::CodeBlock
        } else if self.autolink > 0 {
            Style::Url
        } else if self.heading > 0 {
            Style::Heading(self.heading)
        } else if self.link > 0 {
            Style::Link
        } else if self.strike > 0 {
            Style::Strike
        } else if self.quote > 0 {
            Style::Quote
        } else {
            match (self.bold > 0, self.italic > 0) {
                (true, true) => Style::BoldItalic,
                (true, false) => Style::Bold,
                (false, true) => Style::Italic,
                _ => Style::Plain,
            }
        }
    }
}

/// Paints the leaves pulldown-cmark reports. Bytes no leaf covers stay `Marker`.
fn paint_markdown(src: &str, styles: &mut [Style]) {
    let bytes = src.as_bytes();
    let opts = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut cx = Context::default();
    // End of the last leaf seen: an inline link's address follows its text.
    let mut last_leaf_end = 0;
    for (event, range) in Parser::new_ext(src, opts).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => cx.heading = level as u8,
                Tag::Strong | Tag::TableHead => cx.bold += 1,
                Tag::Emphasis => cx.italic += 1,
                Tag::Strikethrough => cx.strike += 1,
                Tag::Link { link_type, .. } => {
                    if matches!(link_type, LinkType::Autolink | LinkType::Email) {
                        cx.autolink += 1;
                    } else {
                        cx.link += 1;
                    }
                }
                Tag::Image { .. } => cx.link += 1,
                Tag::BlockQuote(_) => cx.quote += 1,
                Tag::CodeBlock(kind) => {
                    cx.code_block = true;
                    let mut end = range.end;
                    // Take the closing fence's line ending too, so the tint fills the row.
                    // (An indented block's range already ends after its line ending.)
                    if matches!(kind, CodeBlockKind::Fenced(_)) {
                        if bytes.get(end) == Some(&b'\r') {
                            end += 1;
                        }
                        if bytes.get(end) == Some(&b'\n') {
                            end += 1;
                        }
                    }
                    styles[range.start..end].fill(Style::CodeBlock);
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => cx.heading = 0,
                TagEnd::Strong | TagEnd::TableHead => cx.bold -= 1,
                TagEnd::Emphasis => cx.italic -= 1,
                TagEnd::Strikethrough => cx.strike -= 1,
                TagEnd::Link => {
                    if cx.autolink > 0 {
                        cx.autolink -= 1;
                    } else {
                        cx.link -= 1;
                        paint_destination(src, last_leaf_end.max(range.start), range.end, styles);
                    }
                }
                TagEnd::Image => {
                    cx.link -= 1;
                    paint_destination(src, last_leaf_end.max(range.start), range.end, styles);
                }
                TagEnd::BlockQuote(_) => cx.quote -= 1,
                TagEnd::CodeBlock => cx.code_block = false,
                _ => {}
            },
            Event::Text(_) | Event::SoftBreak => {
                styles[range.clone()].fill(cx.text_style());
                last_leaf_end = range.end;
            }
            Event::Code(_) => {
                let ticks = bytes[range.clone()]
                    .iter()
                    .take_while(|&&c| c == b'`')
                    .count();
                styles[range.clone()].fill(Style::InlineCode);
                styles[range.start..range.start + ticks].fill(Style::Marker);
                styles[(range.end - ticks).max(range.start + ticks)..range.end].fill(Style::Marker);
                last_leaf_end = range.end;
            }
            Event::TaskListMarker(true) => styles[range].fill(Style::TaskDone),
            // Rules, breaks, HTML and unchecked boxes stay markers.
            _ => {}
        }
    }
}

/// After an inline link's text, `](address "title")` follows: paint the address.
fn paint_destination(src: &str, from: usize, end: usize, styles: &mut [Style]) {
    // The last `](` is this link's: an image nested in the text has its own before it.
    let Some(open) = src[from..end].rfind("](") else {
        return; // a reference link: its address is elsewhere
    };
    let start = from + open + 2;
    let stop = if src[..end].ends_with(')') {
        end - 1
    } else {
        end
    };
    if start < stop {
        styles[start..stop].fill(Style::Url);
    }
}

/// Styles that bare URLs may overwrite: ordinary text, not code or syntax.
fn is_text(s: Style) -> bool {
    matches!(
        s,
        Style::Plain
            | Style::Bold
            | Style::Italic
            | Style::BoldItalic
            | Style::Strike
            | Style::Quote
            | Style::Heading(_)
    )
}

/// pulldown-cmark has no GFM autolinks: find `http(s)://` and `www.` in text.
fn paint_bare_urls(src: &str, styles: &mut [Style]) {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Only `h`/`w` can open a URL; skip the comparisons for every other byte.
        let prefix = if matches!(bytes[i] | 0x20, b'h' | b'w') {
            url_prefix_len(&bytes[i..])
        } else {
            0
        };
        if prefix == 0 || (i > 0 && bytes[i - 1].is_ascii_alphanumeric()) || !is_text(styles[i]) {
            i += 1;
            continue;
        }
        let mut end = i + prefix;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() && bytes[end] != b'<' {
            end += 1;
        }
        end = i + trim_url(&bytes[i..end]);
        if let Some(cut) = styles[i..end].iter().position(|&s| !is_text(s)) {
            end = i + cut;
        }
        if end > i + prefix {
            styles[i..end].fill(Style::Url);
        }
        i = end.max(i + prefix);
    }
}

/// Length of a URL opener (`https://`, `http://`, `www.`) at the start of `rest`, or 0.
fn url_prefix_len(rest: &[u8]) -> usize {
    [&b"https://"[..], b"http://", b"www."]
        .iter()
        .find(|p| rest.len() >= p.len() && rest[..p.len()].eq_ignore_ascii_case(p))
        .map_or(0, |p| p.len())
}

/// Length of `url` without trailing punctuation and unbalanced closing brackets.
fn trim_url(url: &[u8]) -> usize {
    let count = |s: &[u8], c: u8| s.iter().filter(|&&b| b == c).count();
    let mut end = url.len();
    while end > 0 {
        let head = &url[..end];
        let drop = match head[end - 1] {
            b'.' | b',' | b';' | b':' | b'!' | b'?' | b'\'' | b'"' | b'>' => true,
            b')' => count(head, b'(') < count(head, b')'),
            b']' => count(head, b'[') < count(head, b']'),
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    end
}

/// Merges equal neighbours and drops `Plain`.
fn collect_runs(styles: &[Style]) -> Vec<(Range<usize>, Style)> {
    let mut runs = Vec::new();
    let mut start = 0;
    for i in 1..=styles.len() {
        if i == styles.len() || styles[i] != styles[start] {
            if styles[start] != Style::Plain {
                runs.push((start..i, styles[start]));
            }
            start = i;
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &str = include_str!("../tests/fixtures/mdstyle/all.md");

    /// The runs as (covered text, style), checking the run invariants.
    fn runs(text: &str) -> Vec<(&str, Style)> {
        let runs = style_runs(text);
        let mut end = 0;
        for (r, s) in &runs {
            assert!(r.start >= end, "overlap or unsorted at {r:?}");
            assert!(r.start < r.end, "empty run");
            assert!(r.end <= text.len());
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
            assert_ne!(*s, Style::Plain);
            end = r.end;
        }
        runs.into_iter().map(|(r, s)| (&text[r], s)).collect()
    }

    fn has(runs: &[(&str, Style)], text: &str, style: Style) {
        assert!(
            runs.contains(&(text, style)),
            "missing {text:?} as {style:?} in {runs:#?}"
        );
    }

    fn crlf(s: &str) -> String {
        s.replace("\r\n", "\n").replace('\n', "\r\n")
    }

    #[test]
    fn limit_is_one_mebibyte() {
        assert!(styling_enabled(0));
        assert!(styling_enabled(STYLING_LIMIT));
        assert!(!styling_enabled(STYLING_LIMIT + 1));
    }

    #[test]
    fn plain_text_has_no_runs() {
        assert!(style_runs("just some words\nand more").is_empty());
        assert!(style_runs("").is_empty());
    }

    #[test]
    fn headings_by_level() {
        let r = runs(ALL);
        assert!(r
            .iter()
            .any(|&(t, s)| s == Style::Marker && t.contains('#')));
        has(&r, "Title", Style::Heading(1));
        has(&r, "Second level", Style::Heading(2));
        let r = runs("###### six\n\nSetext\n======\n");
        has(&r, "six", Style::Heading(6));
        has(&r, "Setext", Style::Heading(1));
    }

    #[test]
    fn inline_emphasis_and_strike() {
        let r = runs(ALL);
        has(&r, "bold", Style::Bold);
        has(&r, "italic", Style::Italic);
        has(&r, "both", Style::BoldItalic);
        has(&r, "struck", Style::Strike);
        has(&r, "**", Style::Marker);
        has(&r, "~~", Style::Marker);
    }

    #[test]
    fn nested_emphasis_combines() {
        let r = runs("*a **b** c*");
        has(&r, "a ", Style::Italic);
        has(&r, "b", Style::BoldItalic);
        has(&r, " c", Style::Italic);
    }

    #[test]
    fn emphasis_inside_heading_stays_heading() {
        let r = runs("## big **bold** and *it*\n");
        has(&r, "big ", Style::Heading(2));
        has(&r, "bold", Style::Heading(2));
        has(&r, "it", Style::Heading(2));
        has(&r, "**", Style::Marker);
    }

    #[test]
    fn emphasis_inside_quote_and_link_is_absorbed() {
        let r = runs("> a **b**\n\n[x *y*](http://z.example)\n");
        has(&r, "b", Style::Quote);
        has(&r, "y", Style::Link);
    }

    #[test]
    fn inline_code_keeps_backticks_as_markers() {
        let r = runs("a ``co`de`` b");
        has(&r, "``", Style::Marker);
        has(&r, "co`de", Style::InlineCode);
        let r = runs("# t `c`\n");
        has(&r, "c", Style::InlineCode);
    }

    #[test]
    fn fenced_block_covers_fences_and_line_ending() {
        let text = "intro\n\n```rust\nlet x = 1;\n```\nafter\n";
        let r = runs(text);
        has(&r, "```rust\nlet x = 1;\n```\n", Style::CodeBlock);
        let text = crlf(text);
        let r = runs(&text);
        has(&r, "```rust\r\nlet x = 1;\r\n```\r\n", Style::CodeBlock);
    }

    #[test]
    fn indented_block_and_no_styling_inside_code() {
        let r = runs("para\n\n    **not bold**\n");
        assert!(r
            .iter()
            .any(|&(t, s)| s == Style::CodeBlock && t.contains("**not bold**")));
        assert!(!r.iter().any(|&(_, s)| s == Style::Bold));
    }

    #[test]
    fn links_and_destinations() {
        let r = runs(ALL);
        has(&r, "link", Style::Link);
        has(&r, "https://example.com/a \"t\"", Style::Url);
        let r = runs("![alt](img.png) and [ref][1]\n\n[1]: http://x.example\n");
        has(&r, "alt", Style::Link);
        has(&r, "img.png", Style::Url);
        has(&r, "ref", Style::Link);
    }

    #[test]
    fn quote_and_markers() {
        let r = runs(ALL);
        has(&r, "quoted line", Style::Quote);
        assert!(r
            .iter()
            .any(|&(t, s)| s == Style::Marker && t.contains('>')));
        assert!(r
            .iter()
            .any(|&(t, s)| s == Style::Marker && t.contains("- ")));
    }

    #[test]
    fn task_boxes() {
        let r = runs(ALL);
        has(&r, "[x]", Style::TaskDone);
        assert!(r
            .iter()
            .any(|&(t, s)| s == Style::Marker && t.contains("[ ]")));
        assert!(!r
            .iter()
            .any(|&(t, _)| t.contains("open task") && t.contains("[ ]")));
    }

    #[test]
    fn table_header_is_bold_and_pipes_are_markers() {
        let r = runs(ALL);
        has(&r, "a", Style::Bold);
        has(&r, "b", Style::Bold);
        assert!(r
            .iter()
            .any(|&(t, s)| s == Style::Marker && t.contains('|')));
    }

    #[test]
    fn bare_and_auto_urls() {
        let r = runs(ALL);
        has(&r, "https://example.org/path?q=1", Style::Url);
        has(&r, "https://auto.example", Style::Url);
        let r = runs("see www.example.org, (http://a.example/b) and **http://c.example**.\n");
        has(&r, "www.example.org", Style::Url);
        has(&r, "http://a.example/b", Style::Url);
        has(&r, "http://c.example", Style::Url);
    }

    #[test]
    fn nested_image_link_and_url_ending_in_underscore() {
        let r = runs(
            "[![a](i.png)](u.html) http://a.example/x_ end
",
        );
        has(&r, "i.png", Style::Url);
        has(&r, "u.html", Style::Url);
        has(&r, "http://a.example/x_", Style::Url);
    }

    #[test]
    fn indented_block_keeps_the_blank_line_after_it() {
        let r = runs(
            "    code

para
",
        );
        assert!(r.iter().any(|&(t, s)| s == Style::CodeBlock
            && t.ends_with(
                "code
"
            )));
        assert!(!r.iter().any(|&(t, s)| s == Style::CodeBlock
            && t.ends_with(
                "

"
            )));
    }

    #[test]
    fn urls_in_code_are_not_urls() {
        let r = runs("`http://a.example`\n\n```\nhttp://b.example\n```\n");
        assert!(!r.iter().any(|&(_, s)| s == Style::Url));
    }

    #[test]
    fn crlf_offsets_stay_correct() {
        let text = crlf(ALL);
        let r = runs(&text);
        has(&r, "Title", Style::Heading(1));
        has(&r, "bold", Style::Bold);
        has(&r, "[x]", Style::TaskDone);
        has(&r, "https://example.org/path?q=1", Style::Url);
        let lf = runs(ALL);
        let words = |v: &[(&str, Style)]| -> Vec<(String, Style)> {
            v.iter()
                .filter(|(_, s)| !matches!(s, Style::Marker | Style::CodeBlock))
                .map(|&(t, s)| (t.to_string(), s))
                .collect()
        };
        assert_eq!(words(&r), words(&lf));
    }

    #[test]
    fn multibyte_text_keeps_char_boundaries() {
        let r = runs("Città — **perché** `é`\n\nhttp://à.example/ü fine\n");
        has(&r, "perché", Style::Bold);
        has(&r, "é", Style::InlineCode);
    }

    #[test]
    fn spell_check_skips_code_urls_and_markers() {
        assert!(Style::Bold.is_spell_checked());
        assert!(Style::Link.is_spell_checked());
        for s in [
            Style::InlineCode,
            Style::CodeBlock,
            Style::Url,
            Style::Marker,
        ] {
            assert!(!s.is_spell_checked());
        }
    }

    /// Styles `fixture` repeated to 5 MB and returns (runs, ms).
    fn time_five_megabytes(fixture: &str) -> (usize, f64) {
        let mut text = String::new();
        while text.len() < 5 * 1024 * 1024 {
            text.push_str(fixture);
            text.push('\n');
        }
        let start = std::time::Instant::now();
        let n = style_runs(&text).len();
        (n, start.elapsed().as_secs_f64() * 1000.0)
    }

    /// `cargo test --release mdstyle -- --ignored --nocapture`. The limit applies to
    /// a prose Markdown file; the all-markup fixture is a stress test and only reported.
    #[test]
    #[ignore]
    fn bench_five_megabytes() {
        let (n, dense) = time_five_megabytes(ALL);
        eprintln!("5 MB, every line markup: {n} runs in {dense:.1} ms");
        let (n, prose) = time_five_megabytes(include_str!("../tests/fixtures/mdstyle/prose.md"));
        eprintln!("5 MB, a prose Markdown file: {n} runs in {prose:.1} ms");
        assert!(prose < 100.0, "took {prose:.1} ms");
    }
}
