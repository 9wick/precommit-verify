//! Repository state hashing via blake3.

use std::{fs::File, path::PathBuf};

use anyhow::Result;
use blake3::Hasher;

use crate::git;

const MD_EXTENSION: &[u8] = b".md";

pub fn compute_hash() -> Result<String> {
    let mut files = git::ls_files()?;
    files.retain(|f| !f.ends_with(MD_EXTENSION));
    files.sort_unstable();

    let mut hasher = Hasher::new();
    for file in &files {
        let path = bytes_to_path(file);
        // Skip silently on open failure (broken symlinks, permission errors,
        // races with concurrent file removal, etc.) — matches the old TS
        // behavior, which used a try/catch around the read.
        let Ok(mut f) = File::open(&path) else {
            continue;
        };
        hasher.update(file);
        // NUL delimiters bound the (filename, content) pair so that two
        // different (filename, content) pairings cannot collide just by
        // shifting the byte boundary between them.
        hasher.update(b"\0");
        hasher.update_reader(&mut f)?;
        hasher.update(b"\0");
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};
    Path::new(OsStr::from_bytes(bytes)).to_path_buf()
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    // Windows: best-effort UTF-8 conversion. True path-byte fidelity on
    // Windows requires WTF-8 plumbing that is out of scope for Phase 1.
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}
