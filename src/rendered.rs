//! The Rendered view: HTML building and the on-demand WebView2 lifecycle (spec §6).

pub mod webview;

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::tabs::MARKDOWN_LIMIT_BYTES;
use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};

/// The host local images are served from (`SetVirtualHostNameToFolderMapping`).
/// `.example` is reserved, so it never shadows a real site.
pub const HOST: &str = "scrivi-doc.example";

/// The page scrivi shows for a Markdown file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Page {
    pub html: String,
    /// The folder to map to [`HOST`]: the nearest common ancestor of the
    /// file's folder and its images, within limits (spec §6).
    pub root: PathBuf,
}

/// The Rendered view's one stylesheet: the mockup's (spec §6).
const STYLESHEET: &str = include_str!("rendered.css");

/// `NavigateToString` refuses a page whose base64 `data:` URL passes 2 MiB:
/// about 1.57 MB of UTF-8 HTML, measured (research/webview2-on-demand.md).
const PAGE_LIMIT_BYTES: usize = 1_500_000;

/// The text is over the 1 MB rule, or its page too big to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLarge;

/// `markdown` as a sanitised page, its relative images resolved against `folder`.
pub fn to_html(markdown: &str, folder: &Path) -> Result<Page, TooLarge> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    to_html_within(markdown, folder, home.as_deref())
}

/// [`to_html`], with the user profile folder given: the mapped folder never
/// climbs to it, nor to a drive root.
pub fn to_html_within(markdown: &str, folder: &Path, home: Option<&Path>) -> Result<Page, TooLarge> {
    if markdown.len() > MARKDOWN_LIMIT_BYTES {
        return Err(TooLarge);
    }
    let opts = Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut events: Vec<Event> = Parser::new_ext(markdown, opts).collect();
    name_headings(&mut events);
    let mut raw = String::new();
    html::push_html(&mut raw, events.into_iter());

    // Relative image sources become placeholders while sanitising, since the
    // mapped root (and so every URL) is known only once all are seen.
    let token = placeholder_token(markdown);
    let images: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    let mut builder = ammonia::Builder::default();
    builder
        // Task list boxes: always a disabled checkbox, whatever raw HTML asked.
        .add_tags(["input"])
        .add_tag_attributes("input", ["checked"])
        .set_tag_attribute_value("input", "type", "checkbox")
        .set_tag_attribute_value("input", "disabled", "")
        // Table column alignment; `style_filter` below keeps only that.
        .add_tag_attributes("th", ["style"])
        .add_tag_attributes("td", ["style"])
        .add_tag_attributes("h1", ["id"])
        .add_tag_attributes("h2", ["id"])
        .add_tag_attributes("h3", ["id"])
        .add_tag_attributes("h4", ["id"])
        .add_tag_attributes("h5", ["id"])
        .add_tag_attributes("h6", ["id"]);
    {
        let images = Arc::clone(&images);
        let folder = folder.to_path_buf();
        let token = token.clone();
        builder.attribute_filter(move |element, attribute, value| match (element, attribute) {
            ("img", "src") => {
                let path = resolve_image(&folder, value)?;
                let mut images = images.lock().ok()?;
                images.push(path);
                Some(format!("/{token}/{}", images.len() - 1).into())
            }
            ("th" | "td", "style") => ALIGNMENTS.contains(&value).then_some(value.into()),
            _ => Some(value.into()),
        });
    }
    let mut body = builder.clean(&raw).to_string();
    let images = std::mem::take(&mut *images.lock().map_err(|_| TooLarge)?);

    let root = mapped_root(folder, &images, home);
    // Last first, so "/t/1" is never taken for the start of "/t/10".
    for (i, image) in images.iter().enumerate().rev() {
        let placeholder = format!(" src=\"/{token}/{i}\"");
        let with = match url_under(&root, image) {
            Some(url) => format!(" src=\"{url}\""),
            // Outside the mapped folder: the image can't load, so it has no source.
            None => String::new(),
        };
        body = body.replace(&placeholder, &with);
    }
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
         img-src https://{HOST}; style-src 'unsafe-inline'\">\
         <meta name=\"color-scheme\" content=\"light\">\
         <style>{STYLESHEET}</style></head><body><article>{body}</article></body></html>"
    );
    if html.len() > PAGE_LIMIT_BYTES {
        return Err(TooLarge);
    }
    Ok(Page { html, root })
}

/// A link the page may follow: out to the default browser or mail client.
/// Everything else is cancelled; `#anchor` links never ask, they just scroll.
pub fn opens_outside(uri: &str) -> bool {
    let scheme = uri.split(':').next().unwrap_or("").to_ascii_lowercase();
    uri.contains(':') && matches!(scheme.as_str(), "http" | "https" | "mailto")
}

/// The only `style` values kept: pulldown-cmark's table column alignments.
const ALIGNMENTS: [&str; 3] = ["text-align: left", "text-align: center", "text-align: right"];

/// Give every heading an `id` as GitHub does, so `[see](#a-heading)` jumps:
/// its text in lower case, punctuation dropped, spaces as `-`, and `-1`,
/// `-2`, ... on repeats.
fn name_headings(events: &mut [Event]) {
    let mut used: HashSet<String> = HashSet::new();
    let mut next_suffix: HashMap<String, usize> = HashMap::new();
    let mut i = 0;
    while i < events.len() {
        if !matches!(events[i], Event::Start(Tag::Heading { id: None, .. })) {
            i += 1;
            continue;
        }
        let mut text = String::new();
        let mut j = i + 1;
        while j < events.len() && !matches!(events[j], Event::End(TagEnd::Heading(_))) {
            if let Event::Text(t) | Event::Code(t) = &events[j] {
                text.push_str(t);
            }
            j += 1;
        }
        let base: String = text
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect();
        let next = next_suffix.entry(base.clone()).or_insert(0);
        let slug = loop {
            let candidate = if *next == 0 { base.clone() } else { format!("{base}-{next}") };
            *next += 1;
            if used.insert(candidate.clone()) {
                break candidate;
            }
        };
        if let Event::Start(Tag::Heading { id, .. }) = &mut events[i] {
            *id = Some(CowStr::from(slug));
        }
        i = j;
    }
}

/// A path segment that appears nowhere in `markdown`, so no text the user
/// wrote can pass for a placeholder.
fn placeholder_token(markdown: &str) -> String {
    (0u32..)
        .map(|n| format!("scrivi-image-{n:x}"))
        .find(|t| !markdown.contains(t.as_str()))
        .expect("some token is free")
}

/// Image paths written by Windows hands use either slash.
const SEPARATORS: [char; 2] = ['/', '\\'];

/// Where a relative image source points on disk; `None` for anything else
/// (remote, absolute, `data:`, a bare `#fragment`), which never loads.
fn resolve_image(folder: &Path, src: &str) -> Option<PathBuf> {
    let src = src.split(['?', '#']).next().unwrap_or("");
    let relative = !src.is_empty()
        && !src.starts_with(SEPARATORS)
        && !src.split(SEPARATORS).next().is_some_and(|first| first.contains(':'));
    if !relative {
        return None;
    }
    let decoded = percent_decode(src);
    let mut path = folder.to_path_buf();
    path.extend(decoded.split(SEPARATORS).filter(|s| !s.is_empty()));
    Some(normalize(&path))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `path` with `.` and `..` worked out on the text alone; `..` stops at the root.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// A component as Windows compares it: case-insensitively.
fn key(c: Component) -> String {
    c.as_os_str().to_string_lossy().to_lowercase()
}

/// `inner` is `outer` or lies inside it.
fn within(inner: &Path, outer: &Path) -> bool {
    let mut inner = inner.components();
    outer.components().all(|o| inner.next().is_some_and(|i| key(i) == key(o)))
}

/// The nearest common ancestor of `folder` and every image's folder. It
/// stays `folder` rather than climb to a drive root or the user profile.
fn mapped_root(folder: &Path, images: &[PathBuf], home: Option<&Path>) -> PathBuf {
    let mut root = normalize(folder);
    for image in images {
        let dir = image.parent().unwrap_or(image);
        while !within(dir, &root) {
            if !root.pop() {
                break;
            }
        }
    }
    let drive_root = !root.components().any(|c| matches!(c, Component::Normal(_)));
    let at_or_above_home = home.is_some_and(|h| within(&normalize(h), &root));
    if root != normalize(folder) && (drive_root || at_or_above_home) {
        return normalize(folder);
    }
    root
}

/// The virtual-host URL of `image`, if it lies inside `root`.
fn url_under(root: &Path, image: &Path) -> Option<String> {
    if !within(image, root) {
        return None;
    }
    let rest: Vec<String> = image
        .components()
        .skip(root.components().count())
        .map(|c| percent_encode(&c.as_os_str().to_string_lossy()))
        .collect();
    Some(format!("https://{HOST}/{}", rest.join("/")))
}

/// Everything but RFC 3986's unreserved characters, as `%XX`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(md: &str) -> String {
        to_html(md, Path::new(r"C:\Users\me\Documents\notes")).unwrap().html
    }

    /// The document's own part of the page, without the stylesheet.
    fn body(page: &str) -> String {
        let start = page.find("<article>").unwrap();
        page[start..].to_string()
    }

    #[test]
    fn scripts_are_removed() {
        let html = page("Hi\n\n<script>alert(1)</script>\n\n<p onclick=\"x()\">p</p>");
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("alert(1)"), "{html}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(html.contains("<p>Hi</p>"), "{html}");
    }

    #[test]
    fn a_relative_image_points_at_the_virtual_host_and_its_folder_is_mapped() {
        let page = to_html("![cat](img/my%20cat.png)", Path::new(r"C:\Users\me\Documents\notes")).unwrap();
        assert!(page.html.contains(r#"src="https://scrivi-doc.example/img/my%20cat.png""#), "{}", page.html);
        assert_eq!(page.root, PathBuf::from(r"C:\Users\me\Documents\notes"));
    }

    const HOME: &str = r"C:\Users\me";

    fn within_home(md: &str, folder: &str) -> Page {
        to_html_within(md, Path::new(folder), Some(Path::new(HOME))).unwrap()
    }

    #[test]
    fn a_parent_folder_image_maps_the_common_ancestor() {
        // Markdown reads `\.` as an escaped dot, so a backslash path doubles it there.
        let md = r"![a](pics/a.png) ![b](../shared/b.png) ![c](..\\..\\work\c.png)";
        let page = within_home(md, r"C:\Users\me\Documents\work\notes");
        assert_eq!(page.root, PathBuf::from(r"C:\Users\me\Documents\work"));
        let h = &page.html;
        assert!(h.contains(r#"src="https://scrivi-doc.example/notes/pics/a.png""#), "{h}");
        assert!(h.contains(r#"src="https://scrivi-doc.example/shared/b.png""#), "{h}");
        assert!(h.contains(r#"src="https://scrivi-doc.example/c.png""#), "{h}");
    }

    #[test]
    fn the_mapping_never_climbs_to_the_user_profile_or_a_drive_root() {
        let page = within_home("![x](../../Pictures/x.png) ![y](y.png)", r"C:\Users\me\Documents\notes");
        assert_eq!(page.root, PathBuf::from(r"C:\Users\me\Documents\notes"));
        assert!(page.html.contains(r#"src="https://scrivi-doc.example/y.png""#), "{}", page.html);
        // The outside image has no source rather than a wrong one.
        assert!(page.html.contains(r#"<img alt="x">"#), "{}", page.html);

        let page = within_home("![z](../../other/z.png)", r"D:\notes\week");
        assert_eq!(page.root, PathBuf::from(r"D:\notes\week"));
    }

    #[test]
    fn remote_absolute_and_file_images_never_load() {
        let md = "![a](https://example.com/a.png) ![b](file:///C:/b.png) ![c](C:\\c.png) ![d](/d.png)";
        let page = within_home(md, r"C:\Users\me\Documents");
        assert!(!page.html.contains("src="), "{}", page.html);
    }

    #[test]
    fn task_lists_tables_and_strikethrough_survive_sanitising() {
        let html = page("- [x] done\n- [ ] todo\n\n| a | b |\n|:--|:-:|\n| 1 | ~~2~~ |\n");
        // Forced attributes come out in no fixed order.
        let boxes: Vec<&str> = html.split("<li><input ").skip(1).map(|s| &s[..s.find('>').unwrap()]).collect();
        assert_eq!(boxes.len(), 2, "{html}");
        for b in &boxes {
            assert!(b.contains(r#"type="checkbox""#) && b.contains(r#"disabled="""#), "{html}");
        }
        assert!(boxes[0].contains("checked") && !boxes[1].contains("checked"), "{html}");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains(r#"<th style="text-align: center">b</th>"#), "{html}");
        assert!(html.contains("<del>2</del>"), "{html}");
    }

    #[test]
    fn raw_html_cannot_smuggle_styles_or_inputs() {
        let html = body(&page("<td style=\"background:url(https://x)\">t</td>\n\n<input type=\"text\" value=\"v\">"));
        assert!(!html.contains("background"), "{html}");
        assert!(!html.contains("type=\"text\""), "{html}");
    }

    #[test]
    fn ten_thousand_duplicate_headings_are_named_fast() {
        let md = "# a
".repeat(10_000);
        let mut events: Vec<Event> = Parser::new(&md).collect();
        let start = std::time::Instant::now();
        name_headings(&mut events);
        assert!(start.elapsed() < std::time::Duration::from_millis(100), "{:?}", start.elapsed());
        let last = events.iter().rev().find_map(|e| match e {
            Event::Start(Tag::Heading { id, .. }) => id.clone(),
            _ => None,
        });
        assert_eq!(last.as_deref(), Some("a-9999"));
    }

    #[test]
    fn duplicate_suffixes_skip_ids_already_taken() {
        let html = page("# a\n\n# a\n\n# a-1\n\n# a\n\n# a-1\n");
        for id in ["a", "a-1", "a-1-1", "a-2", "a-1-2"] {
            assert!(html.contains(&format!(r#"<h1 id="{id}">"#)), "{id}: {html}");
        }
    }

    #[test]
    fn headings_get_anchor_ids_that_links_can_jump_to() {
        let html = page("# Hello, World!\n\n## Hello, World!\n\n### `code` and *em*\n\n[jump](#hello-world-1)");
        assert!(html.contains(r#"<h1 id="hello-world">"#), "{html}");
        assert!(html.contains(r#"<h2 id="hello-world-1">"#), "{html}");
        assert!(html.contains(r#"<h3 id="code-and-em">"#), "{html}");
        assert!(html.contains(r##"<a href="#hello-world-1""##), "{html}");
    }

    #[test]
    fn the_page_is_one_document_with_the_stylesheet_and_no_remote_loads() {
        let html = page("text");
        assert!(html.starts_with("<!doctype html>"), "{html}");
        assert!(html.contains("<style>") && html.contains("Sitka"), "{html}");
        assert!(html.contains("default-src 'none'"), "{html}");
        assert!(html.contains("img-src https://scrivi-doc.example"), "{html}");
        assert!(html.contains("<p>text</p>"), "{html}");
    }

    #[test]
    fn markdown_over_1_mb_is_too_large() {
        let big = "a".repeat(crate::tabs::MARKDOWN_LIMIT_BYTES + 1);
        assert_eq!(to_html(&big, Path::new(r"C:\n")), Err(TooLarge));
        let fits = "a".repeat(crate::tabs::MARKDOWN_LIMIT_BYTES);
        assert!(to_html(&fits, Path::new(r"C:\n")).is_ok());
    }

    #[test]
    fn a_page_navigate_to_string_cannot_take_is_too_large() {
        // Each "- a" line grows to "<li>a</li>": 600 KB of text, a 1.65 MB page.
        let md = "- a
".repeat(150_000);
        assert_eq!(to_html(&md, Path::new(r"C:\n")), Err(TooLarge));
    }

    #[test]
    fn only_web_and_mail_links_open_outside() {
        assert!(opens_outside("https://example.com/a?b#c"));
        assert!(opens_outside("HTTP://example.com"));
        assert!(opens_outside("mailto:me@example.com"));
        for uri in ["file:///C:/x.md", "about:blank#x", "javascript:alert(1)", "other.md", "ms-settings:", "data:text/html,x"] {
            assert!(!opens_outside(uri), "{uri}");
        }
    }

    #[test]
    fn the_notes_fixture_maps_its_parent_and_keeps_its_links() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rendered");
        let md = std::fs::read_to_string(dir.join("notes/notes.md")).unwrap();
        let page = to_html_within(&md, &dir.join("notes"), None).unwrap();
        assert_eq!(page.root, normalize(&dir));
        let h = body(&page.html);
        assert!(h.contains(r#"src="https://scrivi-doc.example/notes/img/window.svg""#), "{h}");
        assert!(h.contains(r#"src="https://scrivi-doc.example/shared/mark.svg""#), "{h}");
        assert!(h.contains(r##"href="#tools""##) && h.contains(r#"<h3 id="tools">"#), "{h}");
        assert!(h.contains(r#"href="mailto:someone@example.com""#), "{h}");
        assert!(!h.contains("scripts ran"), "{h}");
    }

    #[test]
    fn a_raw_html_image_is_rewritten_too() {
        let page = within_home("<img src=\"art/b.svg\" alt=b>", r"C:\Users\me\Documents");
        assert!(page.html.contains(r#"src="https://scrivi-doc.example/art/b.svg""#), "{}", page.html);
    }
}
