//! Writing a file so a crash or a full disk never leaves it half written
//! (SCRIVI-16): the bytes go to a temp file in the same folder, which then
//! takes the file's place.
//!
//! - An existing file is swapped with `ReplaceFileW`, which keeps its
//!   attributes, ACL and creation time. If the ACL can't be carried over the
//!   swap fails, and the in-place write below takes over.
//! - A new file is moved into place with `MoveFileExW`.
//! - A file with other hard links is written in place: a swap would break
//!   the links.
//! - If the swap fails (a read-only file, some network shares), the bytes
//!   are written over the file in place instead; the error says when both
//!   failed. A temp file that can't be written (a full disk) stops the save
//!   with the file untouched.
//! - The temp file is only deleted once the text is safely in the file. If
//!   the file is gone or half written, the temp file is the only complete
//!   copy: it stays, and the error gives its path.
//!
//! A folder watcher sees a replace as rename, delete and create events, not a
//! single change: `tabs`' own-save marker covers them (spec §8, 11).

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, MoveFileExW, ReplaceFileW, BY_HANDLE_FILE_INFORMATION,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, OPEN_EXISTING,
    REPLACEFILE_IGNORE_MERGE_ERRORS,
};

use crate::tabs::FileId;

/// How the bytes got there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    /// An existing file, swapped with the temp file.
    Replaced,
    /// A new file (or one that vanished mid-replace), moved into place.
    Moved,
    /// Written over the file in place, after the safe way failed.
    InPlace,
}

/// Why nothing (or, for `InPlace`, maybe only part) was written.
#[derive(Debug)]
pub struct WriteError {
    /// The safe way's failure, when a write in place was tried after it.
    pub replace: Option<io::Error>,
    /// The last try's failure.
    pub write: io::Error,
    /// A temp file that holds the complete text, left because the file itself
    /// is missing or may be half written.
    pub kept: Option<PathBuf>,
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.replace {
            Some(replace) => write!(
                f,
                "Replacing the file failed ({replace}), and writing it in place failed too ({}).",
                self.write
            )?,
            None => write!(f, "{}", self.write)?,
        }
        match &self.kept {
            Some(temp) => write!(f, "\n\nYour text was saved to {}.", temp.display()),
            None => Ok(()),
        }
    }
}

/// The steps of a save, so tests can make one fail.
struct Steps {
    /// The bytes in a new temp file next to the path.
    temp: fn(&Path, &[u8]) -> io::Result<PathBuf>,
    /// Swap the temp file in for the existing path.
    replace: fn(&Path, &Path) -> io::Result<Written>,
    /// Move the temp file to a path that doesn't exist.
    place: fn(&Path, &Path) -> io::Result<Written>,
    /// Write over the existing file.
    in_place: fn(&Path, &[u8]) -> Result<(), InPlaceError>,
}

const STEPS: Steps = Steps {
    temp: write_temp,
    replace,
    place: move_into,
    in_place: write_in_place,
};

/// Write `bytes` to `path` (spec §7 and the SCRIVI-16 notes).
pub fn write(path: &Path, bytes: &[u8]) -> Result<Written, WriteError> {
    write_with(path, bytes, &STEPS)
}

fn write_with(path: &Path, bytes: &[u8], steps: &Steps) -> Result<Written, WriteError> {
    // No temp file (a full disk): writing over the file could only leave it
    // half written, so stop with it untouched.
    let temp = (steps.temp)(path, bytes).map_err(|write| WriteError {
        replace: None,
        write,
        kept: None,
    })?;
    let info = file_info(path);
    if info.is_none() && std::fs::metadata(path).is_err() {
        // A new file has nothing to write over.
        return match (steps.place)(path, &temp) {
            Ok(written) => Ok(written),
            // The temp file is the only copy of the text.
            Err(write) => Err(WriteError {
                replace: None,
                write,
                kept: Some(temp),
            }),
        };
    }
    // Other hard links would be left on the old bytes by a swap.
    let linked = info.is_some_and(|i| i.links > 1);
    let replace_error = if linked {
        None
    } else {
        match (steps.replace)(path, &temp) {
            Ok(written) => return Ok(written),
            Err(e) => {
                // The original is gone and the move failed too: the temp
                // file holds the text.
                if std::fs::metadata(path).is_err() {
                    return Err(WriteError {
                        replace: None,
                        write: e,
                        kept: Some(temp),
                    });
                }
                Some(e)
            }
        }
    };
    match (steps.in_place)(path, bytes) {
        Ok(()) => {
            let _ = std::fs::remove_file(&temp);
            Ok(Written::InPlace)
        }
        Err(e) => {
            // Untouched file: the temp copy isn't needed. Truncated: it is
            // the only complete copy.
            let kept = if e.truncated {
                Some(temp)
            } else {
                let _ = std::fs::remove_file(&temp);
                None
            };
            Err(WriteError {
                replace: replace_error,
                write: e.error,
                kept,
            })
        }
    }
}

/// A short name that doesn't depend on the file's: a long file name would
/// push the temp path over the limit.
fn write_temp(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let folder = path.parent().unwrap_or(Path::new("."));
    let mut last = None;
    for _ in 0..16 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = folder.join(format!(".scrivi-{}-{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(mut file) => {
                let done = file.write_all(bytes).and_then(|()| file.sync_all());
                drop(file);
                return match done {
                    Ok(()) => Ok(temp),
                    Err(e) => {
                        let _ = std::fs::remove_file(&temp);
                        Err(e)
                    }
                };
            }
            // A leftover from a crashed save: try the next name.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no free temp file name")))
}

/// Swap `temp` in for the existing `path`. An ACL that can't be copied
/// fails the swap rather than being widened silently.
fn replace(path: &Path, temp: &Path) -> io::Result<Written> {
    let flags = REPLACEFILE_IGNORE_MERGE_ERRORS;
    let (path_w, temp_w) = (extended(path), extended(temp));
    let replaced = unsafe { ReplaceFileW(&path_w, &temp_w, PCWSTR::null(), flags, None, None) };
    match replaced {
        Ok(()) => Ok(Written::Replaced),
        // ERROR_UNABLE_TO_MOVE_REPLACEMENT without a backup leaves the file
        // gone and the temp file in place: finish the move.
        Err(_) if std::fs::metadata(path).is_err() => move_into(path, temp),
        Err(e) => Err(io_error(e)),
    }
}

fn move_into(path: &Path, temp: &Path) -> io::Result<Written> {
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    unsafe { MoveFileExW(&extended(temp), &extended(path), flags) }.map_err(io_error)?;
    Ok(Written::Moved)
}

/// A write over the file that failed.
struct InPlaceError {
    error: io::Error,
    /// The file was already emptied: it may hold part of the text.
    truncated: bool,
}

/// Truncate the existing file and write the bytes into it.
fn write_in_place(path: &Path, bytes: &[u8]) -> Result<(), InPlaceError> {
    let untouched = |error| InPlaceError {
        error,
        truncated: false,
    };
    // Opening without truncating first: a refusal leaves the file as it was.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(untouched)?;
    let cut = |error| InPlaceError {
        error,
        truncated: true,
    };
    file.set_len(0).map_err(cut)?;
    file.write_all(bytes).map_err(cut)?;
    file.sync_all().map_err(cut)
}

/// What a path leads to, as `GetFileInformationByHandle` tells.
pub struct FileInfo {
    pub id: Option<FileId>,
    pub links: u32,
}

/// The identity and link count of the file at `path`; `None` if it can't be
/// opened.
pub fn file_info(path: &Path) -> Option<FileInfo> {
    unsafe {
        let handle = CreateFileW(
            &extended(path),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
        .ok()?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        let got = GetFileInformationByHandle(handle, &mut info);
        let _ = CloseHandle(handle);
        got.ok()?;
        let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
        Some(FileInfo {
            // Index 0: the file system has no file index to tell files by.
            id: (index != 0).then_some(FileId {
                volume: info.dwVolumeSerialNumber,
                index,
            }),
            links: info.nNumberOfLinks,
        })
    }
}

/// Paths this long or longer get the `\\?\` prefix, which lifts the
/// `MAX_PATH` limit without needing `longPathAware` and the system's
/// long-path setting (std does the same for its own calls).
const LONG_PATH: usize = 200;

/// `path` as the wide string for a Win32 call.
fn extended(path: &Path) -> HSTRING {
    HSTRING::from(extended_str(&path.to_string_lossy()))
}

fn extended_str(path: &str) -> String {
    let drive_absolute = path.as_bytes().get(1) == Some(&b':')
        && matches!(path.as_bytes().get(2), Some(b'\\' | b'/'));
    let unc = path.starts_with(r"\\") && !path.starts_with(r"\\?\") && !path.starts_with(r"\\.\");
    if path.len() < LONG_PATH {
        return path.to_string();
    }
    let slashes = path.replace('/', "\\");
    if drive_absolute {
        format!(r"\\?\{slashes}")
    } else if unc {
        format!(r"\\?\UNC\{}", &slashes[2..])
    } else {
        path.to_string()
    }
}

/// A Win32 error as the `io::Error` the rest of the save reports.
fn io_error(e: windows::core::Error) -> io::Error {
    let hr = e.code().0 as u32;
    if hr & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((hr & 0xFFFF) as i32)
    } else {
        io::Error::other(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textfile;

    /// A fresh, empty folder under the temp folder.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scrivi-atomic-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Open, then save without an edit: the bytes come back exactly.
    fn round_trip(name: &str, original: &[u8]) {
        let dir = folder(name);
        let path = dir.join(name);
        std::fs::write(&path, original).unwrap();
        let decoded = textfile::decode(&std::fs::read(&path).unwrap(), 1252);
        assert!(!decoded.lossy, "{name}");
        let bytes = textfile::encode(&decoded.text, decoded.encoding, decoded.eol).unwrap();
        assert_eq!(write(&path, &bytes).unwrap(), Written::Replaced);
        assert_eq!(std::fs::read(&path).unwrap(), original, "{name}");
        assert_eq!(entries(&dir), [name], "no temp file is left behind");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unedited_utf16_le_file_round_trips_byte_for_byte() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(
            "Perché è già così\r\n😀 fine\r\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        round_trip("utf16le.txt", &bytes);
    }

    #[test]
    fn an_unedited_windows_1252_file_round_trips_byte_for_byte() {
        // "Perché la città è già così: 20 €" in 1252, LF.
        let bytes = b"Perch\xe9 la citt\xe0 \xe8 gi\xe0 cos\xec: 20 \x80\nfine\n";
        round_trip("ansi.txt", bytes);
    }

    #[test]
    fn an_unedited_crlf_file_round_trips_byte_for_byte() {
        round_trip(
            "crlf.md",
            b"# Title\r\n\r\n- one\r\n- two\r\nno final break",
        );
    }

    #[test]
    fn a_new_file_is_created() {
        let dir = folder("new");
        let path = dir.join("new.md");
        assert_eq!(write(&path, b"hello\n").unwrap(), Written::Moved);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\n");
        assert_eq!(entries(&dir), ["new.md"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_read_only_file_is_left_alone_and_the_error_says_both_tries_failed() {
        let dir = folder("readonly");
        let path = dir.join("locked.txt");
        std::fs::write(&path, b"old").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();

        let err = write(&path, b"new").unwrap_err();
        assert!(err.replace.is_some());
        // Refused before any byte moved: no temp copy is needed.
        assert!(err.kept.is_none());
        assert!(err.to_string().contains("in place"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(entries(&dir), ["locked.txt"]);

        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_temp_file_that_cannot_be_written_never_touches_the_file() {
        // Stands in for a full disk: the temp write fails, so the swap and
        // the write in place are never tried.
        let dir = folder("notemp");
        let path = dir.join("keep.txt");
        std::fs::write(&path, b"precious").unwrap();
        let steps = Steps {
            temp: |_, _| Err(io::Error::other("disk full")),
            ..STEPS
        };
        let err = write_with(&path, b"new", &steps).unwrap_err();
        assert!(err.replace.is_none());
        assert_eq!(err.to_string(), "disk full");
        assert_eq!(std::fs::read(&path).unwrap(), b"precious");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_swap_that_loses_the_file_keeps_the_temp_copy_and_names_it() {
        // ReplaceFileW's ERROR_UNABLE_TO_MOVE_REPLACEMENT: the original is
        // gone and the recovery move failed too.
        let dir = folder("gone");
        let path = dir.join("doc.txt");
        std::fs::write(&path, b"old").unwrap();
        let steps = Steps {
            replace: |path, _| {
                std::fs::remove_file(path).unwrap();
                Err(io::Error::other("move failed"))
            },
            ..STEPS
        };
        let err = write_with(&path, b"new text", &steps).unwrap_err();
        let kept = err.kept.clone().expect("the temp path is reported");
        assert_eq!(std::fs::read(&kept).unwrap(), b"new text");
        assert!(err.to_string().contains("Your text was saved to"), "{err}");
        assert!(
            err.to_string().contains(&kept.display().to_string()),
            "{err}"
        );
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_file_that_cannot_be_moved_into_place_keeps_the_temp_copy() {
        let dir = folder("noplace");
        let path = dir.join("new.txt");
        let steps = Steps {
            place: |_, _| Err(io::Error::other("denied")),
            ..STEPS
        };
        let err = write_with(&path, b"text", &steps).unwrap_err();
        assert_eq!(std::fs::read(err.kept.as_ref().unwrap()).unwrap(), b"text");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_write_in_place_that_fails_after_truncating_keeps_the_temp_copy() {
        let dir = folder("truncated");
        let path = dir.join("doc.txt");
        std::fs::write(&path, b"old").unwrap();
        let steps = Steps {
            replace: |_, _| Err(io::Error::other("swap refused")),
            in_place: |path, _| {
                std::fs::write(path, b"").unwrap();
                Err(InPlaceError {
                    error: io::Error::other("disk full"),
                    truncated: true,
                })
            },
            ..STEPS
        };
        let err = write_with(&path, b"new text", &steps).unwrap_err();
        let kept = err.kept.clone().expect("the temp copy is kept");
        assert_eq!(std::fs::read(&kept).unwrap(), b"new text");
        assert!(err.to_string().contains("Your text was saved to"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_temp_copy_goes_only_after_the_write_in_place_succeeds() {
        let dir = folder("inplace-ok");
        let path = dir.join("doc.txt");
        std::fs::write(&path, b"old").unwrap();
        let steps = Steps {
            replace: |_, temp| {
                // The temp file is still there while the swap fails.
                assert!(temp.exists());
                Err(io::Error::other("swap refused"))
            },
            in_place: |path, bytes| {
                // ...and still there when the in-place write starts.
                let temp = std::fs::read_dir(path.parent().unwrap())
                    .unwrap()
                    .any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"));
                assert!(temp);
                write_in_place(path, bytes)
            },
            ..STEPS
        };
        assert_eq!(write_with(&path, b"new", &steps).unwrap(), Written::InPlace);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(entries(&dir), ["doc.txt"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_hard_linked_file_is_written_in_place_so_the_links_survive() {
        let dir = folder("hardlink");
        let (path, other) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&path, b"old").unwrap();
        std::fs::hard_link(&path, &other).unwrap();
        assert_eq!(write(&path, b"new").unwrap(), Written::InPlace);
        assert_eq!(
            std::fs::read(&other).unwrap(),
            b"new",
            "the other link sees the new text"
        );
        assert_eq!(file_info(&path).unwrap().links, 2);
        assert_eq!(entries(&dir), ["a.txt", "b.txt"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn two_names_for_one_file_have_one_identity() {
        let dir = folder("identity");
        let (path, other, third) = (dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt"));
        std::fs::write(&path, b"x").unwrap();
        std::fs::hard_link(&path, &other).unwrap();
        std::fs::write(&third, b"x").unwrap();
        assert_eq!(file_info(&path).unwrap().id, file_info(&other).unwrap().id);
        assert_ne!(file_info(&path).unwrap().id, file_info(&third).unwrap().id);
        assert!(file_info(&dir.join("missing")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_temp_name_is_short_whatever_the_file_name() {
        let dir = folder("tempname");
        let path = dir.join(format!("{}.txt", "n".repeat(200)));
        let temp = write_temp(&path, b"x").unwrap();
        let name = temp.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.len() < 30 && name.starts_with(".scrivi-") && name.ends_with(".tmp"),
            "{name}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_path_past_max_path_is_saved_both_new_and_replaced() {
        let dir = folder("long");
        let deep = dir.join("d".repeat(120)).join("e".repeat(120));
        std::fs::create_dir_all(&deep).unwrap();
        let path = deep.join(format!("{}.txt", "f".repeat(100)));
        assert!(path.as_os_str().len() > 260);
        assert_eq!(write(&path, b"one").unwrap(), Written::Moved);
        assert_eq!(write(&path, b"two").unwrap(), Written::Replaced);
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(entries(&deep).len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn long_absolute_paths_get_the_extended_prefix_and_short_ones_do_not() {
        let long = format!(r"C:\{}\f.txt", "a".repeat(250));
        assert_eq!(extended_str(&long), format!(r"\\?\{long}"));
        let unc = format!(r"\\srv\share\{}\f.txt", "a".repeat(250));
        assert_eq!(extended_str(&unc), format!(r"\\?\UNC\{}", &unc[2..]));
        assert_eq!(extended_str(r"C:\a\b.txt"), r"C:\a\b.txt");
        let prefixed = format!(r"\\?\C:\{}", "a".repeat(250));
        assert_eq!(extended_str(&prefixed), prefixed);
    }

    #[test]
    fn a_missing_folder_fails_without_a_replace_error() {
        let path = std::env::temp_dir()
            .join("scrivi-no-such-folder-0b1f")
            .join("x.txt");
        let err = write(&path, b"x").unwrap_err();
        assert!(err.replace.is_none());
    }
}
