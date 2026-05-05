use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use crate::footer::VerifyStatus;

mod footer;
mod git;
mod hash;

#[derive(Parser)]
#[command(name = "precommit-verify", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Compute and save the repository hash to .git/precommit-verify-hash
    Save,
    /// Verify the saved hash matches the current repository state
    Check,
    /// Print the current repository hash to stdout
    Compute,
    /// Append a Verified footer to a commit message file
    VerifyFooter {
        /// Path to the commit message file
        msg_file: PathBuf,
        /// Source of the commit (skips when "merge" or "squash")
        source: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Save => save(),
        Cmd::Check => check(),
        Cmd::Compute => compute(),
        Cmd::VerifyFooter { msg_file, source } => verify_footer(&msg_file, source.as_deref()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("precommit-verify: {e}");
            ExitCode::FAILURE
        }
    }
}

fn save() -> Result<()> {
    if std::env::var_os("npm_lifecycle_event").is_none()
        && std::env::var_os("npm_execpath").is_none()
    {
        bail!(
            "save must be called via a package.json script (e.g., 'pnpm run precommit')"
        );
    }
    git::ensure_git_repo()?;
    let hex = hash::compute_hash()?;
    let path = git::hash_file_path()?;
    fs::write(&path, format!("{hex}\n"))?;
    println!("precommit-verify: saved {}", short16(&hex));
    Ok(())
}

fn check() -> Result<()> {
    git::ensure_git_repo()?;
    let path = git::hash_file_path()?;
    if !path.exists() {
        bail!(
            "hash not found. Run your package.json precommit script (e.g., 'pnpm run precommit') first."
        );
    }
    let saved = fs::read_to_string(&path)?.trim().to_string();
    let current = hash::compute_hash()?;
    if saved != current {
        bail!(
            "files changed since last save (saved {}, current {}). Run your package.json precommit script to update.",
            short16(&saved),
            short16(&current),
        );
    }
    println!(
        "precommit-verify: verified \u{2713} ({})",
        short16(&current)
    );
    Ok(())
}

fn compute() -> Result<()> {
    git::ensure_git_repo()?;
    let hex = hash::compute_hash()?;
    println!("{hex}");
    Ok(())
}

fn verify_footer(msg_file: &Path, source: Option<&str>) -> Result<()> {
    // Git invokes the prepare-commit-msg hook for these too; we don't want to
    // touch the auto-generated message in those flows.
    if matches!(source, Some("merge" | "squash")) {
        return Ok(());
    }
    let msg = fs::read_to_string(msg_file)?;
    let stripped = footer::strip_existing_footer(&msg);
    let status = compute_status();
    let footer_str = footer::build_footer(&status);
    fs::write(msg_file, footer::embed_footer(&stripped, &footer_str))?;
    Ok(())
}

/// Determine the verification state for the footer, swallowing all internal
/// errors as `Stale` (they shouldn't block commits — the footer just shows ✕).
fn compute_status() -> VerifyStatus {
    let Ok(path) = git::hash_file_path() else {
        return VerifyStatus::Unverified;
    };
    if !path.exists() {
        return VerifyStatus::Unverified;
    }
    let Ok(saved_raw) = fs::read_to_string(&path) else {
        return VerifyStatus::Stale;
    };
    let saved = saved_raw.trim();
    let Ok(current) = hash::compute_hash() else {
        return VerifyStatus::Stale;
    };
    if saved == current {
        let has_unstaged = git::has_unstaged_changes().unwrap_or(false);
        VerifyStatus::Verified {
            hash16: short16(saved).to_string(),
            has_unstaged,
        }
    } else {
        VerifyStatus::Stale
    }
}

fn short16(hex: &str) -> &str {
    &hex[..hex.len().min(16)]
}
