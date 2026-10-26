//! Disk change watching: notify events turned into WM_APP messages (spec §9).
//!
//! One debounced watcher on the **folders** of the open files. Atomic saves
//! (a temp file and `ReplaceFileW`) show up there as renames, deletes and
//! creates, so a watch on the file itself would be lost. The debouncer's
//! thread only filters events down to the open files and hands each to `post`;
//! it never touches app state. What an event means is for the UI thread to
//! decide: it re-stats the file and asks `tabs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};

/// Quiet time after the last event of a burst before one is reported: the
/// reload lands well inside a second.
const DEBOUNCE: Duration = Duration::from_millis(200);

/// What the watcher saw happen to a watched file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Changed,
    /// Removed, or renamed away.
    Deleted,
}

/// A path as the open files are compared: Windows paths ignore case and `/`.
fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// The open files, with how many Tabs show each. Shared with the debouncer's
/// thread, which only reads it.
type Files = Arc<Mutex<HashMap<String, (PathBuf, usize)>>>;

pub struct Watcher {
    /// `None` if the OS refused a watcher: Disk changes then go unnoticed.
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
    files: Files,
    /// Folders watched now, as the first path that named each.
    folders: HashMap<String, PathBuf>,
}

impl Watcher {
    /// `post` runs on the debouncer's thread for each change to an open file.
    pub fn new(post: impl Fn(PathBuf, Change) + Send + 'static) -> Self {
        let files: Files = Arc::default();
        let seen = files.clone();
        let debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            let mut sent: Vec<(PathBuf, Change)> = Vec::new();
            for event in events {
                for (path, change) in changes(&event.event) {
                    let known = seen
                        .lock()
                        .map(|f| f.get(&key(&path)).map(|(p, _)| p.clone()))
                        .unwrap_or(None);
                    // Report the path the Tab was opened with.
                    let Some(path) = known else { continue };
                    if !sent.iter().any(|s| *s == (path.clone(), change)) {
                        sent.push((path, change));
                    }
                }
            }
            for (path, change) in sent {
                post(path, change);
            }
        })
        .ok();
        Watcher {
            debouncer,
            files,
            folders: HashMap::new(),
        }
    }

    /// A Tab shows `path`: watch its folder unless it already is.
    pub fn watch(&mut self, path: &Path) {
        let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) else {
            return;
        };
        if let Ok(mut files) = self.files.lock() {
            files
                .entry(key(path))
                .or_insert_with(|| (path.to_path_buf(), 0))
                .1 += 1;
        }
        let Some(debouncer) = self.debouncer.as_mut() else {
            return;
        };
        if !self.folders.contains_key(&key(folder))
            && debouncer.watch(folder, RecursiveMode::NonRecursive).is_ok()
        {
            self.folders.insert(key(folder), folder.to_path_buf());
        }
    }

    /// A Tab no longer shows `path`: stop watching its folder when no open
    /// file is left in it.
    pub fn unwatch(&mut self, path: &Path) {
        let Some(folder) = path.parent() else { return };
        let left: Vec<PathBuf> = {
            let Ok(mut files) = self.files.lock() else {
                return;
            };
            if let Some(entry) = files.get_mut(&key(path)) {
                entry.1 -= 1;
                if entry.1 == 0 {
                    files.remove(&key(path));
                }
            }
            files.values().map(|(p, _)| p.clone()).collect()
        };
        if left
            .iter()
            .any(|p| p.parent().is_some_and(|f| key(f) == key(folder)))
        {
            return;
        }
        if let (Some(watched), Some(debouncer)) =
            (self.folders.remove(&key(folder)), self.debouncer.as_mut())
        {
            let _ = debouncer.unwatch(watched);
        }
    }
}

/// The paths one event touched, each with what happened to it.
fn changes(event: &Event) -> Vec<(PathBuf, Change)> {
    let change = |path: &PathBuf, c| (path.clone(), c);
    match event.kind {
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From)) => event
            .paths
            .iter()
            .map(|p| change(p, Change::Deleted))
            .collect(),
        // From, then To: the first name is gone, the second one is new.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => event
            .paths
            .iter()
            .enumerate()
            .map(|(i, p)| {
                change(
                    p,
                    if i == 0 {
                        Change::Deleted
                    } else {
                        Change::Changed
                    },
                )
            })
            .collect(),
        EventKind::Access(_) | EventKind::Other => Vec::new(),
        _ => event
            .paths
            .iter()
            .map(|p| change(p, Change::Changed))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Instant;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scrivi-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn watcher() -> (Watcher, Receiver<(PathBuf, Change)>) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        (
            Watcher::new(move |p, c| drop(tx.lock().unwrap().send((p, c)))),
            rx,
        )
    }

    /// Events arriving within `ms` for `path`.
    fn events_for(rx: &Receiver<(PathBuf, Change)>, path: &Path, ms: u64) -> Vec<Change> {
        let end = Instant::now() + Duration::from_millis(ms);
        let mut got = Vec::new();
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok((p, c)) if p == path => got.push(c),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        got
    }

    #[test]
    fn rewriting_a_watched_file_reports_it_changed_within_a_second() {
        let dir = temp_dir("rewrite");
        let file = dir.join("a.md");
        std::fs::write(&file, "one").unwrap();
        let (mut w, rx) = watcher();
        w.watch(&file);
        let t = Instant::now();
        std::fs::write(&file, "two, longer").unwrap();
        let (path, change) = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("an event within a second");
        assert_eq!((path, change), (file.clone(), Change::Changed));
        assert!(t.elapsed() < Duration::from_secs(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_a_watched_file_reports_it_deleted() {
        let dir = temp_dir("delete");
        let file = dir.join("a.md");
        std::fs::write(&file, "one").unwrap();
        let (mut w, rx) = watcher();
        w.watch(&file);
        std::fs::remove_file(&file).unwrap();
        assert!(events_for(&rx, &file, 1500).contains(&Change::Deleted));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_files_in_the_folder_are_not_reported() {
        let dir = temp_dir("others");
        let file = dir.join("a.md");
        std::fs::write(&file, "one").unwrap();
        let (mut w, rx) = watcher();
        w.watch(&file);
        std::fs::write(dir.join("b.md"), "other").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_millis(800)).ok(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_folder_is_watched_until_its_last_file_is_unwatched() {
        let dir = temp_dir("refcount");
        let (a, b) = (dir.join("a.md"), dir.join("b.md"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let (mut w, rx) = watcher();
        w.watch(&a);
        w.watch(&b);
        w.unwatch(&a);
        std::fs::write(&b, "bb").unwrap();
        assert!(!events_for(&rx, &b, 1000).is_empty(), "b is still watched");
        w.unwatch(&b);
        assert!(w.folders.is_empty());
        std::fs::write(&b, "bbb").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_millis(800)).ok(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_reports_the_old_name_deleted_and_the_new_one_changed() {
        let event = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(PathBuf::from(r"C:\d\old.md"))
            .add_path(PathBuf::from(r"C:\d\new.md"));
        assert_eq!(
            changes(&event),
            [
                (PathBuf::from(r"C:\d\old.md"), Change::Deleted),
                (PathBuf::from(r"C:\d\new.md"), Change::Changed)
            ]
        );
    }

    #[test]
    fn reads_of_a_file_are_no_change() {
        let event = Event::new(EventKind::Access(notify::event::AccessKind::Any))
            .add_path(PathBuf::from(r"C:\a.md"));
        assert!(changes(&event).is_empty());
    }
}
