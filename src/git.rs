//! Thin wrappers around `git` invoked as a subprocess.
//!
//! Subprocess (rather than libgit2) is intentional: keeps the binary small
//! and the startup cost low, and matches the file set the user's `git` would
//! see (respecting their `.gitconfig` aliases, includes, etc.).

use std::{path::PathBuf, process::Command};

use anyhow::{anyhow, Context, Result};
use bstr::ByteSlice;

fn run_git(args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .output()
        .context("failed to spawn git")
}

pub fn ensure_git_repo() -> Result<()> {
    let out = run_git(&["rev-parse", "--git-dir"])?;
    if !out.status.success() {
        return Err(anyhow!("not a git repository"));
    }
    Ok(())
}

/// Resolve the hash file path via `git rev-parse --git-path`, which produces
/// the correct location inside worktrees and submodules where `.git` is a
/// pointer file rather than a directory.
pub fn hash_file_path() -> Result<PathBuf> {
    let out = run_git(&["rev-parse", "--git-path", "precommit-check-hash"])?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("git rev-parse failed: {}", stderr.trim()));
    }
    let path_str = String::from_utf8(out.stdout).context("git rev-parse output is not UTF-8")?;
    Ok(PathBuf::from(path_str.trim_end()))
}

/// List tracked + untracked (excluding `.gitignore`d) paths as raw bytes,
/// using NUL-delimited output so paths with newlines or non-UTF-8 bytes are
/// handled correctly.
pub fn ls_files() -> Result<Vec<Vec<u8>>> {
    let out = run_git(&[
        "-c",
        "core.quotepath=false",
        "ls-files",
        "-z",
        "-c",
        "-o",
        "--exclude-standard",
    ])?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("git ls-files failed: {}", stderr.trim()));
    }
    Ok(out
        .stdout
        .split_str(b"\0")
        .filter(|s| !s.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}

pub fn has_unstaged_changes() -> Result<bool> {
    let out = run_git(&["diff", "--name-only"])?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("git diff failed: {}", stderr.trim()));
    }
    Ok(!out.stdout.iter().all(u8::is_ascii_whitespace))
}
