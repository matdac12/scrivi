//! Pure Tab state machine: commands in, actions out (spec §8).
//!
//! `Tabs` owns every Tab rule in spec §3, §7 and §8 and nothing else: no
//! Win32, no Scintilla, no file system. `app` calls a command, then performs
//! the returned [`Action`]s in order. Work that needs the outside world (a
//! dialog, a read, a write) is an action, and its outcome comes back as
//! another command (`loaded`, `saved`, `close_reply`, ...).
//!
//! Paths are compared the Windows way: case-insensitive, `/` same as `\`.
//! `app` passes absolute paths.

#![allow(dead_code)] // 08 (saving, encodings), 11 (Disk changes) and 13 (Rendered view) call the rest.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::textfile;
#[allow(unused_imports)] // re-exported for callers of `tabs`
pub use crate::textfile::{Encoding, Eol, LARGE_FILE_BYTES};

/// Over this many bytes of text a Markdown file loses styling, spell check
/// and the Rendered view (spec §5, the 1 MB rule).
pub const MARKDOWN_LIMIT_BYTES: usize = 1024 * 1024;

/// How long after scrivi's own save a vanished file is still taken for the
/// middle of that save (a replace-style save deletes, then recreates).
pub const OWN_SAVE_WINDOW: Duration = Duration::from_secs(2);

/// How long a file must stay missing before it counts as deleted on disk:
/// a delete-then-create save (another editor, `git checkout`) brings it back
/// within this.
pub const MISSING_GRACE: Duration = Duration::from_secs(1);

/// Identifies a Tab for its whole life. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

/// The caller's editor document for a Tab (a Scintilla document pointer).
/// `Tabs` stores it and hands it back, never looks inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorDoc(pub isize);

/// What a Tab holds, which decides styling, spell check and Ctrl+E.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A `.md` / `.markdown` file, or an Untitled tab.
    Markdown,
    Text,
}

/// Which view a Tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Source,
    Rendered,
}

/// The bar under the tabs; at most one per Tab (spec §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Changed on disk while the Tab had unsaved edits: [Keep mine] [Reload].
    DiskChange,
    /// Deleted or renamed on disk: [Save].
    DeletedOnDisk,
    /// Opened as a Large file: ×.
    LargeFile,
}

/// A button on a [`Notice`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    Reload,
    KeepMine,
    Save,
    /// The × on the Large file Notice.
    Dismiss,
}

/// The user's answer to a save-before-close dialog: Save / Don't save /
/// Cancel for one Tab, Save all / Discard all / Cancel for the Window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReply {
    Save,
    Discard,
    Cancel,
}

/// A note in the status bar's left slot (spec §4, §5, §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusNote {
    /// "Read-only"
    ReadOnly,
    /// "Markdown styling off (over 1 MB)"
    StylingOff,
    /// "Rendered view · Esc to edit"
    RenderedView,
    /// "Too large to render", after Ctrl+E on a Markdown file over 1 MB.
    TooLargeToRender,
    /// The file has bytes its encoding cannot hold: the Tab is read-only
    /// until Reopen with encoding finds one that fits.
    InvalidBytes,
    /// The safe save (temp file, then replace) failed, but writing over the
    /// file in place worked.
    SavedInPlace,
    /// A recent file picked from File › Recent files no longer exists.
    RecentGone,
}

impl StatusNote {
    /// What the status bar's left slot says.
    pub fn text(self) -> &'static str {
        match self {
            StatusNote::ReadOnly => "Read-only",
            StatusNote::StylingOff => "Markdown styling off (over 1 MB)",
            StatusNote::RenderedView => "Rendered view \u{b7} Esc to edit",
            StatusNote::TooLargeToRender => "Too large to render",
            StatusNote::RecentGone => "That file no longer exists: removed from Recent files",
            StatusNote::SavedInPlace => "Saved in place: the safe replace failed",
            StatusNote::InvalidBytes => concat!(
                "Read-only: some bytes don\u{2019}t fit this encoding ",
                "\u{b7} Encoding \u{203a} Reopen with encoding\u{2026}"
            ),
        }
    }
}

/// A file's size and modified time, as `app` read them from disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub size: u64,
    pub modified: SystemTime,
}

/// Which file a path leads to, whatever the path says: the volume serial and
/// file index `GetFileInformationByHandle` reports. Two paths with the same
/// identity (a junction, a symlink, a hard link, a short name) are one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileId {
    pub volume: u32,
    pub index: u64,
}

/// What `app` found when it performed an [`Action::Load`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadedFile {
    /// The encoding the text was decoded with.
    pub encoding: Encoding,
    pub eol: Eol,
    /// The file as it was when read.
    pub stat: FileStat,
    /// The file's identity, if `app` could read it.
    pub file_id: Option<FileId>,
    /// Length of the text in the editor document, in bytes.
    pub text_len: usize,
    /// Some bytes were not valid in `encoding` (`textfile::Decoded::lossy`).
    pub lossy: bool,
}

/// Where the Save As dialog starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveAsFolder {
    /// The user's Documents folder, which only `app` can look up.
    Documents,
    Folder(PathBuf),
}

/// The Save As dialog's starting folder and file name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveAsDefault {
    pub folder: SaveAsFolder,
    pub file_name: String,
}

/// Something `app` must do. Perform them in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Create an empty editor document for this new Untitled tab, then call
    /// [`Tabs::set_document`].
    CreateDocument(TabId),
    /// Read `path` and decode it (as `encoding` if given, otherwise by
    /// detection) into the Tab's editor document, creating or replacing the
    /// document as the size needs. `keep_caret_line`: the Tab already shows
    /// this file, so keep the caret on the same line as far as possible.
    /// Then call [`Tabs::loaded`] or [`Tabs::load_failed`].
    Load {
        id: TabId,
        path: PathBuf,
        encoding: Option<Encoding>,
        keep_caret_line: bool,
    },
    /// Encode the Tab's text and write it to `path`, then call
    /// [`Tabs::saved`] or [`Tabs::save_failed`] before handling any other
    /// message, so the watcher's events for this write find it recorded.
    Write {
        id: TabId,
        path: PathBuf,
        encoding: Encoding,
        eol: Eol,
    },
    /// Convert every line ending in the Tab's text to `eol`.
    ConvertEol { id: TabId, eol: Eol },
    /// Show this Tab: swap its document in and restore its caret, view,
    /// Notice and status bar.
    Activate(TabId),
    /// Something shown about this Tab changed (name, unsaved dot, Notice,
    /// kind, encoding, EOL, view, read-only): redraw it, and if it is the
    /// active Tab reapply its editor settings and view.
    Changed(TabId),
    /// The Tab is gone: release its document, if it had one.
    Closed {
        id: TabId,
        document: Option<EditorDoc>,
    },
    /// Ask Save / Don't save / Cancel, then call [`Tabs::close_reply`].
    AskSaveBeforeClose(TabId),
    /// One dialog listing these unsaved Tabs with Save all / Discard all /
    /// Cancel, then call [`Tabs::close_window_reply`].
    AskSaveAll(Vec<TabId>),
    /// Show the Save As dialog, then call [`Tabs::save_as_chosen`] or, if
    /// cancelled, [`Tabs::save_failed`].
    AskSaveAs { id: TabId, default: SaveAsDefault },
    /// Ask whether to drop unsaved edits and reopen in `encoding`; on yes
    /// call [`Tabs::reopen_confirmed`].
    AskDiscardForReopen { id: TabId, encoding: Encoding },
    /// Start watching this file for Disk changes.
    Watch(PathBuf),
    /// Stop watching this file.
    Unwatch(PathBuf),
    /// Stat `path` again `after` this long and pass the result to
    /// [`Tabs::disk_event`]: the file is missing, but may be in the middle
    /// of a delete-then-create save (scrivi's own or another program's).
    Recheck { path: PathBuf, after: Duration },
    /// Move this file to the top of the recent files.
    AddRecent(PathBuf),
    /// Save As refused: tell the user "<name> is open in another tab.
    /// Close it first."
    OpenInAnotherTab { name: String },
    /// Show this note in the status bar for a moment.
    Flash(StatusNote),
    /// Every Tab is dealt with: close the Window.
    CloseWindow,
}

/// One Tab's state. Read it through [`Tabs::get`] and the queries below.
#[derive(Clone, Debug)]
pub struct Tab {
    id: TabId,
    /// `None` for an Untitled tab.
    path: Option<PathBuf>,
    /// The N in "Untitled N"; kept after a Save As, but then unused.
    untitled_number: u32,
    document: Option<EditorDoc>,
    /// The text is away from its save point: edits not yet in the file.
    edited: bool,
    /// An encoding or line-ending choice not yet in the file.
    chosen: bool,
    /// The file has bytes its encoding cannot hold: read-only.
    lossy: bool,
    /// The file was deleted on disk and is still gone.
    missing: bool,
    view: View,
    notice: Option<Notice>,
    encoding: Encoding,
    eol: Eol,
    large: bool,
    /// Length of the text in bytes, for the 1 MB rule.
    text_len: usize,
    /// Whether the file has been read into the Tab once.
    loaded: bool,
    /// The file as the Tab's text came from it: the last load or save.
    /// Only `loaded` and `saved` move it.
    loaded_stat: Option<FileStat>,
    /// The identity of the file the Tab last loaded or saved.
    file_id: Option<FileId>,
    /// The latest file state the Tab acted on (reloaded for, or showed a
    /// Notice for), so a repeated event does not act twice.
    seen: Option<FileStat>,
    /// The latest file state reported while the first load was running.
    seen_while_loading: Option<Option<FileStat>>,
    /// When the file was first reported missing, while it may yet return.
    missing_since: Option<Instant>,
    /// When scrivi last saved this Tab, while that save's events may still
    /// be arriving.
    own_save: Option<Instant>,
}

impl Tab {
    pub fn id(&self) -> TabId {
        self.id
    }

    /// The file name, or "Untitled N".
    pub fn name(&self) -> String {
        match &self.path {
            Some(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            None => format!("Untitled {}", self.untitled_number),
        }
    }

    /// `None` for an Untitled tab.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_untitled(&self) -> bool {
        self.path.is_none()
    }

    pub fn document(&self) -> Option<EditorDoc> {
        self.document
    }

    /// Markdown for `.md` / `.markdown` (any case) and Untitled tabs.
    pub fn kind(&self) -> Kind {
        let markdown = match &self.path {
            None => true,
            Some(path) => path.extension().is_some_and(|e| {
                e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown")
            }),
        };
        if markdown {
            Kind::Markdown
        } else {
            Kind::Text
        }
    }

    /// Shows the unsaved dot and asks before closing: it has edits not yet
    /// written, or its file was deleted on disk.
    pub fn unsaved(&self) -> bool {
        self.edited || self.chosen || self.missing
    }

    pub fn view(&self) -> View {
        self.view
    }

    /// The Notice to show while this Tab is active.
    pub fn notice(&self) -> Option<Notice> {
        self.notice
    }

    /// How the next save encodes the text.
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn eol(&self) -> Eol {
        self.eol
    }

    /// Large files are read-only, and so are files with bytes their
    /// encoding cannot hold: saving them back would change those bytes.
    pub fn read_only(&self) -> bool {
        self.large || self.lossy
    }

    /// Word wrap may be on (it is always off for Large files).
    pub fn wrap_allowed(&self) -> bool {
        !self.large
    }

    /// Gets markdown styling: a Markdown file within the 1 MB rule, never a
    /// Large file.
    pub fn styled(&self) -> bool {
        self.kind() == Kind::Markdown && !self.large && self.text_len <= MARKDOWN_LIMIT_BYTES
    }

    /// Gets spell check: the same files as styling.
    pub fn spell_checked(&self) -> bool {
        self.styled()
    }

    /// Ctrl+E may show the Rendered view: the same files as styling.
    pub fn can_render(&self) -> bool {
        self.styled()
    }

    /// The status bar's left-slot note for this Tab.
    pub fn status_note(&self) -> Option<StatusNote> {
        if self.lossy {
            Some(StatusNote::InvalidBytes)
        } else if self.large {
            Some(StatusNote::ReadOnly)
        } else if self.view == View::Rendered {
            Some(StatusNote::RenderedView)
        } else if self.kind() == Kind::Markdown && !self.styled() {
            Some(StatusNote::StylingOff)
        } else {
            None
        }
    }

    /// The file's size on disk, for the Large file Notice; `None` for an
    /// Untitled tab.
    pub fn file_size(&self) -> Option<u64> {
        self.loaded_stat.map(|s| s.size)
    }

    /// Back to the Source view if the Tab may no longer be rendered.
    fn fit_view(&mut self) {
        if !self.can_render() {
            self.view = View::Source;
        }
    }

    /// Rereads the Tab's file in place, as `encoding` or by detection.
    fn reread(&self, encoding: Option<Encoding>) -> Action {
        Action::Load {
            id: self.id,
            path: self.path.clone().expect("only files reread"),
            encoding,
            keep_caret_line: true,
        }
    }

    /// The file was reported missing at `now`: recheck until it has stayed
    /// gone past [`MISSING_GRACE`] (and past [`OWN_SAVE_WINDOW`] after an own
    /// save), then mark it deleted on disk.
    fn file_missing(&mut self, now: Instant) -> Vec<Action> {
        if self.missing {
            return Vec::new();
        }
        let since = *self.missing_since.get_or_insert(now);
        let mut deadline = since + MISSING_GRACE;
        if let Some(saved_at) = self.own_save {
            deadline = deadline.max(saved_at + OWN_SAVE_WINDOW);
        }
        if now < deadline {
            let path = self.path.clone().expect("loaded Tabs have a path");
            return vec![Action::Recheck {
                path,
                after: deadline - now,
            }];
        }
        self.missing = true;
        self.missing_since = None;
        self.notice = Some(Notice::DeletedOnDisk);
        vec![Action::Changed(self.id)]
    }

    fn write(&self, path: PathBuf) -> Action {
        Action::Write {
            id: self.id,
            path,
            encoding: self.encoding,
            eol: self.eol,
        }
    }

    fn is_at(&self, path: &Path) -> bool {
        self.is_at_id(path, None)
    }

    /// On `path`, or on the file with identity `id`: two paths to one file
    /// (a junction, an alias) count as one. The path still matches when the
    /// identities differ, since a replaced file gets a new one.
    fn is_at_id(&self, path: &Path, id: Option<FileId>) -> bool {
        let same_file = matches!((self.file_id, id), (Some(a), Some(b)) if a == b);
        same_file || self.path.as_deref().is_some_and(|p| same_path(p, path))
    }
}

/// Windows path equality: case-insensitive, `/` same as `\`.
fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// Every open Tab, in strip order, and which one is active.
#[derive(Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
    /// The Tab an `open` wants shown once it has loaded.
    show_on_load: Option<TabId>,
    /// A close waiting on saves.
    pending_close: Option<PendingClose>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingClose {
    /// Save in the Save / Don't save / Cancel dialog.
    Tab(TabId),
    /// Save all in the Window's dialog.
    Window,
}

impl Tabs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// Every Tab in strip order.
    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    /// Opens each path in its own Tab, or switches to the Tab that already
    /// has it. The last path's Tab is shown. Every opened file moves to the
    /// top of the recent files (a new Tab's once it has loaded).
    pub fn open(&mut self, paths: &[PathBuf]) -> Vec<Action> {
        let paths: Vec<_> = paths.iter().map(|p| (p.clone(), None)).collect();
        self.open_with_ids(&paths)
    }

    /// [`Tabs::open`], with each file's identity where `app` could read it,
    /// so a second path to an open file switches to its Tab.
    pub fn open_with_ids(&mut self, paths: &[(PathBuf, Option<FileId>)]) -> Vec<Action> {
        let mut actions = Vec::new();
        for (i, (path, file_id)) in paths.iter().enumerate() {
            let last = i + 1 == paths.len();
            if let Some(tab) = self.tabs.iter().find(|t| t.is_at_id(path, *file_id)) {
                let (id, loaded) = (tab.id, tab.loaded);
                let known = tab.path.clone();
                if !loaded {
                    // `loaded` shows it and adds it to the recent files.
                    if last {
                        self.show_on_load = Some(id);
                    }
                    continue;
                }
                if last {
                    self.show_on_load = None;
                    actions.extend(self.activate(id));
                }
                actions.extend(known.map(Action::AddRecent));
                continue;
            }
            let id = self.push(Some(path.clone()), 0);
            if last {
                self.show_on_load = Some(id);
            }
            // Watch first, so a change during the load is not missed.
            actions.push(Action::Watch(path.clone()));
            actions.push(Action::Load {
                id,
                path: path.clone(),
                encoding: None,
                keep_caret_line: false,
            });
        }
        actions
    }

    /// `app` performed an [`Action::Load`].
    pub fn loaded(&mut self, id: TabId, file: LoadedFile) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        let first = !tab.loaded;
        let was_large = tab.large;
        tab.loaded = true;
        tab.encoding = file.encoding;
        tab.eol = file.eol;
        tab.loaded_stat = Some(file.stat);
        tab.file_id = file.file_id;
        tab.seen = Some(file.stat);
        tab.missing = false;
        tab.missing_since = None;
        tab.text_len = file.text_len;
        tab.large = textfile::is_large(file.stat.size);
        tab.lossy = file.lossy;
        tab.edited = false;
        tab.chosen = false;
        // A change reported while loading, and not in what was read.
        let raced = tab
            .seen_while_loading
            .take()
            .is_some_and(|latest| latest != Some(file.stat));
        let keep_large_notice = tab.notice == Some(Notice::LargeFile);
        tab.notice =
            (tab.large && (first || !was_large || keep_large_notice)).then_some(Notice::LargeFile);
        tab.fit_view();
        let path = tab.path.clone();
        let reload = raced.then(|| tab.reread(None));

        let mut actions = vec![Action::Changed(id)];
        if self.show_on_load == Some(id) {
            self.show_on_load = None;
            self.active = Some(id);
            actions.push(Action::Activate(id));
        }
        if first {
            actions.extend(path.map(Action::AddRecent));
        }
        actions.extend(reload);
        actions
    }

    /// `app` could not perform an [`Action::Load`]. A Tab that never loaded
    /// goes away (an Untitled tab takes the place of the last one, and if
    /// nothing is shown the last Tab is). A Tab that already shows text keeps
    /// it; if the file on disk differs from that text, the Disk change Notice
    /// offers the reread again rather than take the change as read.
    pub fn load_failed(&mut self, id: TabId) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        if tab.loaded {
            if tab.seen.is_some() && tab.seen != tab.loaded_stat {
                tab.notice = Some(Notice::DiskChange);
                return vec![Action::Changed(id)];
            }
            return Vec::new();
        }
        if self.show_on_load == Some(id) {
            self.show_on_load = None;
        }
        let mut actions = self.remove(id);
        if self.tabs.is_empty() {
            actions.extend(self.new_untitled());
        } else if self.active.is_none() && self.show_on_load.is_none() {
            let last = self.tabs[self.tabs.len() - 1].id;
            actions.extend(self.activate(last));
        }
        actions
    }

    /// Adds "Untitled N" at the end, N the lowest number no open Untitled
    /// tab uses, and makes it active.
    pub fn new_untitled(&mut self) -> Vec<Action> {
        let number = (1..)
            .find(|n| {
                !self
                    .tabs
                    .iter()
                    .any(|t| t.is_untitled() && t.untitled_number == *n)
            })
            .expect("u32 has room");
        let id = self.push(None, number);
        self.active = Some(id);
        vec![Action::CreateDocument(id), Action::Activate(id)]
    }

    /// Attaches the editor document `app` made for this Tab. Returns the
    /// document `app` must now release: the one it replaces, or `doc` itself
    /// if the Tab is gone.
    pub fn set_document(&mut self, id: TabId, doc: EditorDoc) -> Option<EditorDoc> {
        match self.tab_mut(id) {
            Some(tab) => tab.document.replace(doc),
            None => Some(doc),
        }
    }

    /// Shows this Tab.
    pub fn activate(&mut self, id: TabId) -> Vec<Action> {
        if self.active == Some(id) || self.get(id).is_none() {
            return Vec::new();
        }
        self.active = Some(id);
        vec![Action::Activate(id)]
    }

    /// Closes a Tab, or asks Save / Don't save / Cancel if it has unsaved
    /// edits. Closing the last Tab closes the Window.
    pub fn close(&mut self, id: TabId) -> Vec<Action> {
        let Some(tab) = self.get(id) else {
            return Vec::new();
        };
        if tab.unsaved() {
            let mut actions = self.activate(id);
            actions.push(Action::AskSaveBeforeClose(id));
            return actions;
        }
        self.close_now(id)
    }

    /// The answer to [`Action::AskSaveBeforeClose`].
    pub fn close_reply(&mut self, id: TabId, reply: CloseReply) -> Vec<Action> {
        if self.get(id).is_none() {
            return Vec::new();
        }
        match reply {
            CloseReply::Save => {
                self.pending_close = Some(PendingClose::Tab(id));
                self.save(id)
            }
            CloseReply::Discard => self.close_now(id),
            CloseReply::Cancel => Vec::new(),
        }
    }

    /// Closes the Window, or asks once about every unsaved Tab: Save all /
    /// Discard all / Cancel.
    pub fn close_window(&mut self) -> Vec<Action> {
        let unsaved: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.unsaved())
            .map(|t| t.id)
            .collect();
        if unsaved.is_empty() {
            vec![Action::CloseWindow]
        } else {
            vec![Action::AskSaveAll(unsaved)]
        }
    }

    /// The answer to [`Action::AskSaveAll`]. Save all saves the unsaved
    /// Tabs one at a time, in strip order; an Untitled tab is shown and goes
    /// through Save As. A cancelled or failed save stops the close.
    pub fn close_window_reply(&mut self, reply: CloseReply) -> Vec<Action> {
        match reply {
            CloseReply::Save => {
                self.pending_close = Some(PendingClose::Window);
                self.save_next_for_window()
            }
            CloseReply::Discard => vec![Action::CloseWindow],
            CloseReply::Cancel => Vec::new(),
        }
    }

    fn save_next_for_window(&mut self) -> Vec<Action> {
        let Some(tab) = self.tabs.iter().find(|t| t.unsaved()) else {
            self.pending_close = None;
            return vec![Action::CloseWindow];
        };
        let id = tab.id;
        let mut actions = Vec::new();
        if tab.is_untitled() {
            actions.extend(self.activate(id));
        }
        actions.extend(self.save(id));
        actions
    }

    /// The text changed; `text_len` is its new length in bytes.
    pub fn edited(&mut self, id: TabId, text_len: usize) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        if tab.read_only() {
            return Vec::new();
        }
        let before = (tab.unsaved(), tab.styled());
        tab.edited = true;
        tab.text_len = text_len;
        if (tab.unsaved(), tab.styled()) == before {
            Vec::new()
        } else {
            vec![Action::Changed(id)]
        }
    }

    /// Writes the Tab to its file; an Untitled tab goes through Save As.
    /// A lossy Tab never overwrites its file: it flashes why instead.
    pub fn save(&mut self, id: TabId) -> Vec<Action> {
        let Some(tab) = self.get(id) else {
            return Vec::new();
        };
        match &tab.path {
            Some(_) if tab.lossy => self.refuse_lossy_save(id),
            Some(path) => vec![tab.write(path.clone())],
            None => self.save_as(id),
        }
    }

    fn refuse_lossy_save(&mut self, id: TabId) -> Vec<Action> {
        self.save_failed(id);
        vec![Action::Flash(StatusNote::InvalidBytes)]
    }

    /// The Tab's text is back at its save point (Scintilla's
    /// `SCN_SAVEPOINTREACHED`, after undo or redo): the edits are gone. An
    /// encoding or line-ending choice, or a deleted file, still needs saving.
    pub fn save_point_reached(&mut self, id: TabId) -> Vec<Action> {
        self.set_edited(id, false)
    }

    /// The Tab's text left its save point (`SCN_SAVEPOINTLEFT`).
    pub fn save_point_left(&mut self, id: TabId) -> Vec<Action> {
        self.set_edited(id, true)
    }

    fn set_edited(&mut self, id: TabId, edited: bool) -> Vec<Action> {
        match self.tab_mut(id) {
            Some(tab) if !tab.read_only() => {
                let before = tab.unsaved();
                tab.edited = edited;
                if tab.unsaved() == before {
                    Vec::new()
                } else {
                    vec![Action::Changed(id)]
                }
            }
            _ => Vec::new(),
        }
    }

    pub fn save_as(&mut self, id: TabId) -> Vec<Action> {
        match self.save_as_default(id) {
            Some(default) => vec![Action::AskSaveAs { id, default }],
            None => Vec::new(),
        }
    }

    /// Where Save As starts: an Untitled tab as `Untitled N.md` in
    /// Documents, a file as itself in its folder.
    pub fn save_as_default(&self, id: TabId) -> Option<SaveAsDefault> {
        let tab = self.get(id)?;
        Some(match &tab.path {
            None => SaveAsDefault {
                folder: SaveAsFolder::Documents,
                file_name: format!("{}.md", tab.name()),
            },
            Some(path) => SaveAsDefault {
                folder: match path.parent() {
                    Some(parent) => SaveAsFolder::Folder(parent.to_path_buf()),
                    None => SaveAsFolder::Documents,
                },
                file_name: tab.name(),
            },
        })
    }

    /// The user picked `path` in the Save As dialog.
    /// A file open in another Tab is refused: both Tabs stay as they are,
    /// and a close waiting on this save is called off.
    pub fn save_as_chosen(&mut self, id: TabId, path: PathBuf) -> Vec<Action> {
        self.save_as_chosen_with_id(id, path, None)
    }

    /// [`Tabs::save_as_chosen`], with the chosen file's identity if it exists
    /// and `app` could read it: an alias of an open file is refused too, and
    /// an alias of a lossy Tab's own file is not overwritten.
    pub fn save_as_chosen_with_id(&mut self, id: TabId, path: PathBuf, file_id: Option<FileId>) -> Vec<Action> {
        let Some(tab) = self.get(id) else {
            return Vec::new();
        };
        let elsewhere = self.tabs.iter().find(|t| t.id != id && t.is_at_id(&path, file_id));
        match elsewhere {
            Some(other) => {
                let name = other.name();
                self.save_failed(id);
                vec![Action::OpenInAnotherTab { name }]
            }
            None if tab.lossy && tab.is_at_id(&path, file_id) => self.refuse_lossy_save(id),
            None => vec![tab.write(path)],
        }
    }

    /// `app` wrote the Tab to `path` (its own, or a new one from Save As)
    /// and found `stat` there afterwards, at `now`. The extension decides
    /// the kind from here on.
    pub fn saved(&mut self, id: TabId, path: PathBuf, stat: FileStat, now: Instant) -> Vec<Action> {
        self.saved_with_id(id, path, stat, None, now)
    }

    /// [`Tabs::saved`], with the written file's identity if `app` read it.
    pub fn saved_with_id(
        &mut self,
        id: TabId,
        path: PathBuf,
        stat: FileStat,
        file_id: Option<FileId>,
        now: Instant,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(tab) = self.tab_mut(id) else {
            return actions;
        };
        match tab.path.replace(path.clone()) {
            Some(old) if same_path(&old, &path) => {}
            Some(old) => {
                actions.push(Action::Unwatch(old));
                actions.push(Action::Watch(path.clone()));
            }
            None => actions.push(Action::Watch(path.clone())),
        }
        tab.loaded = true;
        tab.edited = false;
        tab.chosen = false;
        // The file now holds what the Tab shows.
        tab.lossy = false;
        tab.missing = false;
        tab.missing_since = None;
        tab.loaded_stat = Some(stat);
        tab.file_id = file_id;
        tab.seen = Some(stat);
        tab.own_save = Some(now);
        if matches!(tab.notice, Some(Notice::DiskChange | Notice::DeletedOnDisk)) {
            tab.notice = None;
        }
        tab.fit_view();
        actions.push(Action::Changed(id));
        actions.push(Action::AddRecent(path.clone()));

        match self.pending_close {
            Some(PendingClose::Tab(waiting)) if waiting == id => {
                self.pending_close = None;
                actions.extend(self.close_now(id));
            }
            Some(PendingClose::Window) => actions.extend(self.save_next_for_window()),
            _ => {}
        }
        actions
    }

    /// The watcher reported `path`, and `app` re-checked it: `on_disk` is
    /// its size and modified time now, `None` if it no longer exists.
    ///
    /// Per Tab with that file:
    /// - the file as the Tab's text came from it (which covers scrivi's own
    ///   saves): nothing; if it had been deleted, it is back and the
    ///   deleted-on-disk Notice goes;
    /// - changed, with no unsaved edits: a silent reload;
    /// - changed, with unsaved edits: the Disk change Notice;
    /// - missing: [`Action::Recheck`] until it has stayed missing for
    ///   [`MISSING_GRACE`] (a delete-then-create save brings it back sooner),
    ///   then the text is kept, marked unsaved, with the deleted-on-disk
    ///   Notice.
    ///
    /// A repeat of a state already acted on does nothing more. An event that
    /// arrives while the first load runs is held, and `loaded` rereads if
    /// the file moved on from what was read.
    ///
    /// The pending-save marker: within [`OWN_SAVE_WINDOW`] of scrivi's own
    /// save, and until a stat shows that save's size and modified time, a
    /// missing file is taken for the save in mid-replace and rechecked after
    /// the window. Known limit: scrivi tells its own write from another by
    /// size and modified time only, so another program's write with the same
    /// size and time, or one that lands between scrivi's write and its stat,
    /// goes unnoticed.
    pub fn disk_event(
        &mut self,
        path: &Path,
        on_disk: Option<FileStat>,
        now: Instant,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for id in self.tabs_at(path) {
            actions.extend(self.react_to_disk(id, on_disk, now));
        }
        actions
    }

    fn react_to_disk(&mut self, id: TabId, on_disk: Option<FileStat>, now: Instant) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        if !tab.loaded {
            tab.seen_while_loading = Some(on_disk);
            return Vec::new();
        }
        let Some(stat) = on_disk else {
            // NTFS may give the file's index to another file now.
            tab.file_id = None;
            return tab.file_missing(now);
        };
        tab.missing_since = None;
        if Some(stat) == tab.loaded_stat {
            tab.own_save = None;
            tab.seen = Some(stat);
            if !tab.missing {
                return Vec::new();
            }
            // Back as it was: unsaved again only if it has edits.
            tab.missing = false;
            if tab.notice == Some(Notice::DeletedOnDisk) {
                tab.notice = None;
            }
            return vec![Action::Changed(id)];
        }
        let was_missing = std::mem::take(&mut tab.missing);
        if Some(stat) == tab.seen && !was_missing {
            return Vec::new();
        }
        tab.seen = Some(stat);
        if tab.edited {
            tab.notice = Some(Notice::DiskChange);
            vec![Action::Changed(id)]
        } else {
            vec![tab.reread(None)]
        }
    }

    /// A button on the Tab's Notice. Reload drops the edits and rereads the
    /// file; Keep mine dismisses the Disk change Notice and the next save
    /// overwrites the file; Save writes a deleted file again.
    pub fn notice_action(&mut self, id: TabId, action: NoticeAction) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        match (tab.notice, action) {
            (Some(Notice::DiskChange), NoticeAction::Reload) => vec![tab.reread(None)],
            (Some(Notice::DiskChange), NoticeAction::KeepMine)
            | (Some(Notice::LargeFile), NoticeAction::Dismiss) => {
                tab.notice = None;
                vec![Action::Changed(id)]
            }
            (Some(Notice::DeletedOnDisk), NoticeAction::Save) => self.save(id),
            _ => Vec::new(),
        }
    }

    /// Line endings ›: converts the whole Tab at once and marks it unsaved.
    pub fn set_eol(&mut self, id: TabId, eol: Eol) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        if tab.read_only() || tab.eol == eol {
            return Vec::new();
        }
        tab.eol = eol;
        tab.chosen = true;
        vec![Action::ConvertEol { id, eol }, Action::Changed(id)]
    }

    /// Encoding ›: the next save writes in `encoding`. The Tab is marked
    /// unsaved, so closing it does not lose the choice.
    pub fn set_encoding(&mut self, id: TabId, encoding: Encoding) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        if tab.read_only() || tab.encoding == encoding {
            return Vec::new();
        }
        tab.encoding = encoding;
        tab.chosen = true;
        vec![Action::Changed(id)]
    }

    /// "Save as UTF-8 instead", after the Tab's encoding could not hold its
    /// text: the save that follows writes UTF-8. Unlike Encoding › this works
    /// on a read-only Tab too, since it is part of a save already asked for.
    pub fn use_utf8_instead(&mut self, id: TabId) -> Vec<Action> {
        match self.tab_mut(id) {
            Some(tab) if tab.encoding != Encoding::Utf8 => {
                tab.encoding = Encoding::Utf8;
                tab.chosen = true;
                vec![Action::Changed(id)]
            }
            _ => Vec::new(),
        }
    }

    /// Reopen with encoding…: rereads the file as `encoding`, asking first
    /// if that would drop unsaved edits.
    pub fn reopen_with_encoding(&mut self, id: TabId, encoding: Encoding) -> Vec<Action> {
        match self.get(id) {
            Some(tab) if tab.is_untitled() => Vec::new(),
            Some(tab) if tab.unsaved() => vec![Action::AskDiscardForReopen { id, encoding }],
            Some(_) => self.reopen_confirmed(id, encoding),
            None => Vec::new(),
        }
    }

    /// Yes to [`Action::AskDiscardForReopen`].
    pub fn reopen_confirmed(&mut self, id: TabId, encoding: Encoding) -> Vec<Action> {
        match self.get(id) {
            Some(tab) if !tab.is_untitled() => vec![tab.reread(Some(encoding))],
            _ => Vec::new(),
        }
    }

    /// Esc: back to the Source view.
    pub fn show_source(&mut self, id: TabId) -> Vec<Action> {
        match self.tab_mut(id) {
            Some(tab) if tab.view == View::Rendered => {
                tab.view = View::Source;
                vec![Action::Changed(id)]
            }
            _ => Vec::new(),
        }
    }

    /// Ctrl+E: switches the Tab of a Markdown file between the Source and
    /// Rendered views. Over the 1 MB rule it only flashes "Too large to render"; in
    /// a Text file it does nothing.
    pub fn toggle_view(&mut self, id: TabId) -> Vec<Action> {
        let Some(tab) = self.tab_mut(id) else {
            return Vec::new();
        };
        match tab.view {
            View::Rendered => tab.view = View::Source,
            View::Source if tab.can_render() => tab.view = View::Rendered,
            View::Source if tab.kind() == Kind::Markdown => {
                return vec![Action::Flash(StatusNote::TooLargeToRender)]
            }
            View::Source => return Vec::new(),
        }
        vec![Action::Changed(id)]
    }

    /// Some Tab is in the Rendered view, so WebView2 must exist.
    pub fn any_rendered(&self) -> bool {
        self.tabs.iter().any(|t| t.view == View::Rendered)
    }

    /// Ctrl+Tab: the next Tab, wrapping around.
    pub fn next(&mut self) -> Vec<Action> {
        self.step(1)
    }

    /// Ctrl+Shift+Tab: the previous Tab, wrapping around.
    pub fn prev(&mut self) -> Vec<Action> {
        self.step(self.tabs.len().saturating_sub(1))
    }

    fn step(&mut self, by: usize) -> Vec<Action> {
        let Some(at) = self.tabs.iter().position(|t| Some(t.id) == self.active) else {
            return Vec::new();
        };
        let id = self.tabs[(at + by) % self.tabs.len()].id;
        self.activate(id)
    }

    /// The Save As dialog was cancelled, or the write failed (`app` shows
    /// why). A close waiting on this save is called off: this Tab's own, or
    /// the Window's Save all.
    pub fn save_failed(&mut self, id: TabId) -> Vec<Action> {
        match self.pending_close {
            Some(PendingClose::Tab(waiting)) if waiting != id => {}
            _ => self.pending_close = None,
        }
        Vec::new()
    }

    fn close_now(&mut self, id: TabId) -> Vec<Action> {
        let mut actions = self.remove(id);
        if self.tabs.is_empty() {
            actions.push(Action::CloseWindow);
        }
        actions
    }

    /// Takes a Tab out of the strip. If it was active, its right neighbour
    /// (else its left one) is shown.
    fn remove(&mut self, id: TabId) -> Vec<Action> {
        let Some(at) = self.tabs.iter().position(|t| t.id == id) else {
            return Vec::new();
        };
        let tab = self.tabs.remove(at);
        let mut actions = vec![Action::Closed {
            id,
            document: tab.document,
        }];
        if let Some(path) = tab.path {
            if self.find(&path).is_none() {
                actions.push(Action::Unwatch(path));
            }
        }
        if self.tabs.is_empty() {
            self.active = None;
        } else if self.active == Some(id) {
            let next = self.tabs[at.min(self.tabs.len() - 1)].id;
            self.active = Some(next);
            actions.push(Action::Activate(next));
        }
        actions
    }

    /// Appends a fresh Tab with the defaults for new files (`textfile::NEW_FILE`).
    fn push(&mut self, path: Option<PathBuf>, untitled_number: u32) -> TabId {
        self.next_id += 1;
        let id = TabId(self.next_id);
        self.tabs.push(Tab {
            id,
            path,
            untitled_number,
            document: None,
            edited: false,
            chosen: false,
            lossy: false,
            missing: false,
            view: View::Source,
            notice: None,
            encoding: textfile::NEW_FILE.0,
            eol: textfile::NEW_FILE.1,
            large: false,
            text_len: 0,
            loaded: false,
            loaded_stat: None,
            file_id: None,
            seen: None,
            seen_while_loading: None,
            missing_since: None,
            own_save: None,
        });
        id
    }

    /// Every Tab on this file (at most one: open switches, Save As refuses).
    fn tabs_at(&self, path: &Path) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|t| t.is_at(path))
            .map(|t| t.id)
            .collect()
    }

    fn find(&self, path: &Path) -> Option<TabId> {
        self.tabs.iter().find(|t| t.is_at(path)).map(|t| t.id)
    }

    fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }
}

#[cfg(test)]
mod tests;
