//! Pure functions for commit-message footer manipulation.
//!
//! All functions are deterministic and free of I/O so they can be exhaustively
//! covered by unit tests. The CLI layer (main.rs) wires them to file I/O and
//! `git`-derived state.

const FOOTER_PREFIX: &str = "Verified: precommit-verify";

pub enum VerifyStatus {
    /// Hash matches; `has_unstaged` distinguishes ✓ from △.
    Verified { hash16: String, has_unstaged: bool },
    /// Hash file exists but the current state diverges (or hashing failed).
    Stale,
    /// No hash file recorded yet.
    Unverified,
}

#[must_use]
pub fn build_footer(status: &VerifyStatus) -> String {
    match status {
        VerifyStatus::Verified {
            hash16,
            has_unstaged: false,
        } => {
            format!("{FOOTER_PREFIX} \u{2713} ({hash16})")
        }
        VerifyStatus::Verified {
            hash16,
            has_unstaged: true,
        } => {
            format!("{FOOTER_PREFIX} \u{25b3} ({hash16})")
        }
        VerifyStatus::Stale | VerifyStatus::Unverified => {
            format!("{FOOTER_PREFIX} \u{2715}")
        }
    }
}

/// Remove every line that starts with the precommit-verify footer prefix.
///
/// Implementation note: row-based filter; intentionally avoids the `regex`
/// crate to keep startup cost down (this binary runs on every commit).
#[must_use]
pub fn strip_existing_footer(msg: &str) -> String {
    let kept: Vec<&str> = msg
        .lines()
        .filter(|line| !line.starts_with(FOOTER_PREFIX))
        .collect();
    kept.join("\n")
}

/// Append `footer` to `msg`, ensuring exactly one blank line before it.
#[must_use]
pub fn embed_footer(msg: &str, footer: &str) -> String {
    let trimmed = msg.trim_end();
    if trimmed.is_empty() {
        format!("{footer}\n")
    } else {
        format!("{trimmed}\n\n{footer}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── build_footer ────────────────────────────────────────────────────

    #[test]
    fn build_footer_verified_clean() {
        let s = VerifyStatus::Verified {
            hash16: "abcdef0123456789".into(),
            has_unstaged: false,
        };
        assert_eq!(
            build_footer(&s),
            "Verified: precommit-verify \u{2713} (abcdef0123456789)"
        );
    }

    #[test]
    fn build_footer_verified_with_unstaged() {
        let s = VerifyStatus::Verified {
            hash16: "abcdef0123456789".into(),
            has_unstaged: true,
        };
        assert_eq!(
            build_footer(&s),
            "Verified: precommit-verify \u{25b3} (abcdef0123456789)"
        );
    }

    #[test]
    fn build_footer_stale() {
        assert_eq!(
            build_footer(&VerifyStatus::Stale),
            "Verified: precommit-verify \u{2715}"
        );
    }

    #[test]
    fn build_footer_unverified() {
        assert_eq!(
            build_footer(&VerifyStatus::Unverified),
            "Verified: precommit-verify \u{2715}"
        );
    }

    // ─── strip_existing_footer ───────────────────────────────────────────

    #[test]
    fn strip_no_footer() {
        let msg = "feat: add feature\n\nbody line";
        assert_eq!(strip_existing_footer(msg), "feat: add feature\n\nbody line");
    }

    #[test]
    fn strip_single_footer() {
        let msg = "feat: add feature\n\nVerified: precommit-verify \u{2713} (abcdef0123456789)";
        assert_eq!(strip_existing_footer(msg), "feat: add feature\n");
    }

    #[test]
    fn strip_multiple_footers_amend_bug() {
        // Defensive: should clean up if past bug accidentally appended duplicates.
        let msg = "feat: x\n\nVerified: precommit-verify \u{2713} (aaa)\nVerified: precommit-verify \u{2715}\n";
        assert_eq!(strip_existing_footer(msg), "feat: x\n");
    }

    #[test]
    fn strip_keeps_lines_that_only_resemble_footer() {
        // Footer must be at line start; substrings inside body must remain.
        let msg = "feat: x\nbody mentions Verified: precommit-verify inline\n";
        assert_eq!(
            strip_existing_footer(msg),
            "feat: x\nbody mentions Verified: precommit-verify inline"
        );
    }

    #[test]
    fn strip_keeps_old_prepush_footer() {
        // Old `Verified: prepush ...` is intentionally not touched
        // (managed by old CLI elsewhere; we only own our own prefix).
        let msg = "feat: x\n\nVerified: prepush \u{2713} (abc)\n";
        assert_eq!(
            strip_existing_footer(msg),
            "feat: x\n\nVerified: prepush \u{2713} (abc)"
        );
    }

    // ─── embed_footer ────────────────────────────────────────────────────

    #[test]
    fn embed_footer_trims_and_inserts_blank_line() {
        let result = embed_footer(
            "feat: add feature\n",
            "Verified: precommit-verify \u{2713} (h)",
        );
        assert_eq!(
            result,
            "feat: add feature\n\nVerified: precommit-verify \u{2713} (h)\n"
        );
    }

    #[test]
    fn embed_footer_handles_no_trailing_newline() {
        let result = embed_footer("feat: x", "Verified: precommit-verify \u{2715}");
        assert_eq!(result, "feat: x\n\nVerified: precommit-verify \u{2715}\n");
    }

    #[test]
    fn embed_footer_handles_multiple_trailing_newlines() {
        let result = embed_footer("feat: x\n\n\n", "Verified: precommit-verify \u{2715}");
        assert_eq!(result, "feat: x\n\nVerified: precommit-verify \u{2715}\n");
    }

    #[test]
    fn embed_footer_handles_crlf_msg() {
        // trim_end() strips CR+LF; rebuilt with LF only (Git canonical).
        let result = embed_footer("feat: x\r\n\r\n", "Verified: precommit-verify \u{2715}");
        assert_eq!(result, "feat: x\n\nVerified: precommit-verify \u{2715}\n");
    }

    #[test]
    fn embed_footer_handles_empty_msg() {
        let result = embed_footer("", "Verified: precommit-verify \u{2715}");
        assert_eq!(result, "Verified: precommit-verify \u{2715}\n");
    }

    // ─── round-trip (strip → embed) ──────────────────────────────────────

    #[test]
    fn round_trip_amend_replaces_old_footer() {
        let original = "feat: x\n\nVerified: precommit-verify \u{2713} (oldhash000000000)\n";
        let stripped = strip_existing_footer(original);
        let new_footer = build_footer(&VerifyStatus::Verified {
            hash16: "newhash000000000".into(),
            has_unstaged: false,
        });
        let result = embed_footer(&stripped, &new_footer);
        assert_eq!(
            result,
            "feat: x\n\nVerified: precommit-verify \u{2713} (newhash000000000)\n"
        );
        // Ensure no duplicate footer line.
        assert_eq!(result.matches("Verified: precommit-verify").count(), 1);
    }
}
