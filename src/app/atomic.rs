//! Writing a file so a crash or a full disk never leaves it half written
//! (SCRIVI-16): the bytes go to a temp file in the same folder, which then
//! takes the file's place.
//!
//! - An existing file is swapped with `ReplaceFileW`, which keeps its
//!   attributes, ACL and creation time.
//! - A new file is moved into place with `MoveFileExW`.
//! - If the swap fails (a read-only file, some network shares), the bytes
//!   are written over the file in place instead; the error says when both
//!   failed. A temp file that can't be written (a full disk) stops the save
//!   with the file untouched.
//!
//! A folder watcher sees a replace as rename, delete and create events, not a
//! single change: `tabs`' own-save marker covers them (spec §8, 11).

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, REPLACEFILE_IGNORE_ACL_ERRORS,
    REPLACEFILE_IGNORE_MERGE_ERRORS,
};

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
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.replace {
            Some(replace) => write!(
                f,
                "Replacing the file failed ({replace}), and writing it in place failed too ({}).",
                self.write
            ),
            None => write!(f, "{}", self.write),
        }
    }
}

/// Write `bytes` to `path` (spec §7 and the SCRIVI-16 notes).
pub fn write(path: &Path, bytes: &[u8]) -> Result<Written, WriteError> {
    write_with(path, bytes, write_temp)
}

/// `write`, with the temp-file step given (tests stand in a full disk).
fn write_with(
    path: &Path,
    bytes: &[u8],
    temp: impl FnOnce(&Path, &[u8]) -> io::Result<PathBuf>,
) -> Result<Written, WriteError> {
    // No temp file (a full disk): writing over the file could only leave it
    // half written, so stop with it untouched.
    let temp = temp(path, bytes).map_err(|write| WriteError { replace: None, write })?;
    let existed = std::fs::metadata(path).is_ok();
    let swapped = if existed { replace(path, &temp) } else { move_into(path, &temp) };
    let replace = match swapped {
        Ok(written) => return Ok(written),
        Err(e) => e,
    };
    let _ = std::fs::remove_file(&temp);
    if !existed {
        // A new file has nothing to write over.
        return Err(WriteError { replace: None, write: replace });
    }
    match write_in_place(path, bytes) {
        Ok(()) => Ok(Written::InPlace),
        Err(write) => Err(WriteError { replace: Some(replace), write }),
    }
}

/// The bytes in a new temp file next to `path`, flushed to disk.
fn write_temp(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let name = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    let folder = path.parent().unwrap_or(Path::new("."));
    let mut last = None;
    for n in 0..16 {
        let temp = folder.join(format!(".{name}.{}-{n}.scrivi-tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
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

/// Swap `temp` in for the existing `path`.
fn replace(path: &Path, temp: &Path) -> io::Result<Written> {
    let flags = REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS;
    let (path_w, temp_w) = (HSTRING::from(path), HSTRING::from(temp));
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
    unsafe { MoveFileExW(&HSTRING::from(temp), &HSTRING::from(path), flags) }.map_err(io_error)?;
    Ok(Written::Moved)
}

/// Truncate the existing file and write the bytes into it.
fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
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
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
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
        bytes.extend("Perché è già così\r\n😀 fine\r\n".encode_utf16().flat_map(u16::to_le_bytes));
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
        round_trip("crlf.md", b"# Title\r\n\r\n- one\r\n- two\r\nno final break");
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
        let err = write_with(&path, b"new", |_, _| Err(io::Error::other("disk full"))).unwrap_err();
        assert!(err.replace.is_none());
        assert_eq!(err.to_string(), "disk full");
        assert_eq!(std::fs::read(&path).unwrap(), b"precious");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_folder_fails_without_a_replace_error() {
        let path = std::env::temp_dir().join("scrivi-no-such-folder-0b1f").join("x.txt");
        let err = write(&path, b"x").unwrap_err();
        assert!(err.replace.is_none());
    }
}
