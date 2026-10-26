//! Remembered state and recent files in scrivi.ini (spec §10).
//!
//! Pure: [`parse`] and [`serialise`] turn the `.ini` text into [`Prefs`] and
//! back, [`push_recent`] and [`remove_recent`] hold the recent-list rules,
//! and [`clamp_to_work_areas`] fits a remembered rectangle onto a monitor.
//! Reading and writing the file is the thin [`load`] and [`save`] at the end.
//!
//! The file is small and hand-readable; there is no settings UI. A corrupt
//! file, or a bad line in one, falls back to the defaults for what it spoils.

use std::path::{Path, PathBuf};

/// How many recent files are kept.
pub const MAX_RECENT: usize = 10;
/// Zoom is Scintilla's own unit, points added to every style.
pub const ZOOM_MIN: i32 = -10;
pub const ZOOM_MAX: i32 = 20;
/// A remembered window smaller than this (in pixels) is a corrupt one.
const MIN_SIZE: i32 = 100;

/// A window rectangle in screen pixels: the restored one when maximised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// Word wrap: on by default.
    pub wrap: bool,
    /// Zoom, in Scintilla points; 0 is 100%.
    pub zoom: i32,
    pub window: Option<Placement>,
    pub maximized: bool,
    /// Newest first, at most [`MAX_RECENT`].
    pub recent: Vec<PathBuf>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { wrap: true, zoom: 0, window: None, maximized: false, recent: Vec::new() }
    }
}

/// Read `.ini` text. Unknown keys, bad values and comments are skipped.
pub fn parse(text: &str) -> Prefs {
    let mut prefs = Prefs::default();
    let (mut x, mut y, mut w, mut h) = (None, None, None, None);
    let mut recent: Vec<(usize, PathBuf)> = Vec::new();
    let mut section = String::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        match (section.as_str(), key.as_str()) {
            ("view", "wrap") => prefs.wrap = parse_bool(value).unwrap_or(prefs.wrap),
            ("view", "zoom") => {
                if let Ok(z) = value.parse::<i32>() {
                    prefs.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
                }
            }
            ("window", "x") => x = value.parse::<i32>().ok(),
            ("window", "y") => y = value.parse::<i32>().ok(),
            ("window", "w") => w = value.parse::<i32>().ok(),
            ("window", "h") => h = value.parse::<i32>().ok(),
            ("window", "maximized") => prefs.maximized = parse_bool(value).unwrap_or(false),
            ("recent", n) => {
                if let (Ok(n), false) = (n.parse::<usize>(), value.is_empty()) {
                    recent.push((n, PathBuf::from(value)));
                }
            }
            _ => {}
        }
    }
    if let (Some(x), Some(y), Some(w), Some(h)) = (x, y, w, h) {
        if w >= MIN_SIZE && h >= MIN_SIZE {
            prefs.window = Some(Placement { x, y, w, h });
        }
    }
    recent.sort_by_key(|(n, _)| *n);
    for (_, path) in recent {
        push_last(&mut prefs.recent, path);
    }
    prefs
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The `.ini` text for `prefs`.
pub fn serialise(prefs: &Prefs) -> String {
    let mut out = String::from("[view]\n");
    out += &format!("wrap={}\nzoom={}\n", prefs.wrap as u8, prefs.zoom.clamp(ZOOM_MIN, ZOOM_MAX));
    out += "\n[window]\n";
    if let Some(p) = prefs.window {
        out += &format!("x={}\ny={}\nw={}\nh={}\n", p.x, p.y, p.w, p.h);
    }
    out += &format!("maximized={}\n", prefs.maximized as u8);
    out += "\n[recent]\n";
    for (i, path) in prefs.recent.iter().take(MAX_RECENT).enumerate() {
        out += &format!("{}={}\n", i + 1, path.display());
    }
    out
}

// --- Recent files ------------------------------------------------------------

/// Windows compares paths without regard to case.
fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
}

/// Opening or saving `path` moves it to the top; the list keeps [`MAX_RECENT`].
pub fn push_recent(list: &mut Vec<PathBuf>, path: &Path) {
    remove_recent(list, path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX_RECENT);
}

/// A missing file picked from the list is dropped from it.
pub fn remove_recent(list: &mut Vec<PathBuf>, path: &Path) {
    list.retain(|p| !same_path(p, path));
}

/// Add at the end, for reading a stored list that is already newest first.
fn push_last(list: &mut Vec<PathBuf>, path: PathBuf) {
    if list.len() < MAX_RECENT && !list.iter().any(|p| same_path(p, &path)) {
        list.push(path);
    }
}

/// A menu label for a recent file: `&1 path`, with the 10th as `1&0`, `&`
/// doubled and a long path shortened in the middle.
pub fn menu_label(index: usize, path: &Path) -> String {
    const MAX_CHARS: usize = 64;
    let text = path.display().to_string();
    let chars: Vec<char> = text.chars().collect();
    let shown: String = if chars.len() > MAX_CHARS {
        let head: String = chars[..MAX_CHARS / 3].iter().collect();
        let tail: String = chars[chars.len() - (MAX_CHARS - MAX_CHARS / 3 - 1)..].iter().collect();
        format!("{head}\u{2026}{tail}")
    } else {
        text
    };
    let shown = shown.replace('&', "&&");
    match index + 1 {
        10 => format!("1&0  {shown}"),
        n => format!("&{n}  {shown}"),
    }
}

// --- The window ----------------------------------------------------------------

/// Fit `want` onto the work area it overlaps most: shrunk if it is bigger,
/// moved if it hangs off an edge. `None` if it overlaps none (the monitor it
/// was on is gone), so the caller places the Window by default.
pub fn clamp_to_work_areas(want: Placement, work_areas: &[Placement]) -> Option<Placement> {
    let overlap = |a: &Placement| {
        let w = (want.x + want.w).min(a.x + a.w) - want.x.max(a.x);
        let h = (want.y + want.h).min(a.y + a.h) - want.y.max(a.y);
        if w > 0 && h > 0 {
            w as i64 * h as i64
        } else {
            0
        }
    };
    let best = work_areas.iter().filter(|a| overlap(a) > 0).max_by_key(|a| overlap(a))?;
    let w = want.w.min(best.w);
    let h = want.h.min(best.h);
    let x = want.x.clamp(best.x, best.x + best.w - w);
    let y = want.y.clamp(best.y, best.y + best.h - h);
    Some(Placement { x, y, w, h })
}

// --- The file ----------------------------------------------------------------

/// The folder holding scrivi.ini, or `None` when prefs are off.
///
/// Debug builds only: `SCRIVI_TEST_PREFS_DIR` names another folder, and a
/// `SCRIVI_TEST_INSTANCE` without one turns prefs off, so no test or debug
/// run touches the real file. Release builds ignore both.
pub fn dir() -> Option<PathBuf> {
    // Unit tests of the binary never read or write a real file either.
    if cfg!(test) {
        return None;
    }
    dir_for(
        cfg!(debug_assertions),
        std::env::var_os("SCRIVI_TEST_PREFS_DIR"),
        std::env::var_os("SCRIVI_TEST_INSTANCE").is_some(),
        std::env::var_os("LOCALAPPDATA"),
    )
}

fn dir_for(
    debug: bool,
    override_dir: Option<std::ffi::OsString>,
    test_instance: bool,
    local_app_data: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    if debug {
        if let Some(d) = override_dir.filter(|d| !d.is_empty()) {
            return Some(PathBuf::from(d));
        }
        if test_instance {
            return None;
        }
    }
    local_app_data.filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("scrivi"))
}

/// True when this is a test or debug run with its own prefs (or none): the
/// shell's Recent list and jump list must stay untouched then.
pub fn sandboxed() -> bool {
    cfg!(debug_assertions)
        && (std::env::var_os("SCRIVI_TEST_PREFS_DIR").is_some() || std::env::var_os("SCRIVI_TEST_INSTANCE").is_some())
}

const FILE: &str = "scrivi.ini";

/// The stored prefs; the defaults if there are none or the file is unreadable.
pub fn load() -> Prefs {
    let Some(dir) = dir() else { return Prefs::default() };
    std::fs::read(dir.join(FILE)).map(|b| parse(&String::from_utf8_lossy(&b))).unwrap_or_default()
}

/// Write `prefs` (the temp file, then a rename, so a crash never leaves half
/// a file). Failure is ignored: remembering is a courtesy.
pub fn save(prefs: &Prefs) {
    let Some(dir) = dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let tmp = dir.join("scrivi.ini.tmp");
    if std::fs::write(&tmp, serialise(prefs)).is_ok() && std::fs::rename(&tmp, dir.join(FILE)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests;
