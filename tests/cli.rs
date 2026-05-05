//! Integration tests for the precommit-verify CLI.
//!
//! All scenarios spawn the real binary inside a real `git init`-ed tempdir so
//! we exercise the full path: subprocess startup, argv parsing, git
//! invocation, file I/O. Mocks are intentionally avoided per project policy.

// Test code uses short, conventional names (`cmd`, `cwd`, `out`, etc.) that
// clippy::pedantic flags as too similar; readability wins for tests.
#![allow(clippy::similar_names)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use assert_cmd::cargo::CommandCargoExt;
use tempfile::TempDir;

// ─── Helpers ─────────────────────────────────────────────────────────────

/// Isolate every git invocation from the developer's `~/.gitconfig`
/// (`commit.gpgsign`, signing, includes, etc. would otherwise pollute tests).
fn isolate_git(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
}

fn create_tmp_git_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();
    git_in(p, &["init", "-b", "main"]);
    git_in(p, &["config", "user.email", "test@test.com"]);
    git_in(p, &["config", "user.name", "Test"]);
    dir
}

fn git_in(cwd: &Path, args: &[&str]) -> Output {
    let out = isolate_git(Command::new("git").args(args).current_dir(cwd))
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn run(args: &[&str], cwd: &Path) -> Output {
    let mut cmd = Command::cargo_bin("precommit-verify").expect("cargo_bin");
    cmd.args(args)
        .current_dir(cwd)
        .env("npm_lifecycle_event", "precommit-verify");
    isolate_git(&mut cmd)
        .output()
        .expect("spawn precommit-verify")
}

fn run_without_pkg_manager(args: &[&str], cwd: &Path) -> Output {
    let mut cmd = Command::cargo_bin("precommit-verify").expect("cargo_bin");
    cmd.args(args)
        .current_dir(cwd)
        .env_remove("npm_lifecycle_event")
        .env_remove("npm_execpath");
    isolate_git(&mut cmd)
        .output()
        .expect("spawn precommit-verify")
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "expected success but got status={:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn assert_fail_with(out: &Output, needle: &str) {
    assert!(
        !out.status.success(),
        "expected failure but command succeeded"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains(needle),
        "expected output to contain {needle:?}, got: {combined:?}"
    );
}

fn stdout_str(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ─── compute ─────────────────────────────────────────────────────────────

#[test]
fn compute_returns_consistent_hash() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let h1 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();
    let h2 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn compute_excludes_md_files() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let before = stdout_str(&run(&["compute"], dir.path()));

    fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();
    git_in(dir.path(), &["add", "README.md"]);
    let after = stdout_str(&run(&["compute"], dir.path()));

    assert_eq!(before.trim(), after.trim());
}

#[test]
fn compute_changes_on_content_change() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let h1 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    fs::write(dir.path().join("a.ts"), "const a = 2;\n").unwrap();
    let h2 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    assert_ne!(h1, h2);
}

#[test]
fn compute_changes_on_rename() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let h1 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    fs::rename(dir.path().join("a.ts"), dir.path().join("b.ts")).unwrap();
    git_in(dir.path(), &["add", "-A"]);
    let h2 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    assert_ne!(h1, h2);
}

#[test]
fn compute_includes_untracked_files() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("tracked.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "tracked.ts"]);
    let h1 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    fs::write(dir.path().join("untracked.ts"), "const b = 2;\n").unwrap();
    let h2 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    assert_ne!(h1, h2);
}

#[test]
fn compute_skips_gitignored_files() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.ts\n").unwrap();
    git_in(dir.path(), &["add", "a.ts", ".gitignore"]);
    let before = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    fs::write(dir.path().join("ignored.ts"), "const x = 1;\n").unwrap();
    let after = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    assert_eq!(before, after);
}

#[test]
fn compute_handles_empty_files() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("empty.ts"), "").unwrap();
    git_in(dir.path(), &["add", "empty.ts"]);
    let out = run(&["compute"], dir.path());
    assert_ok(&out);
    let hex = stdout_str(&out);
    assert_eq!(hex.trim().len(), 64);
}

#[cfg(unix)]
#[test]
fn compute_handles_filename_with_newline() {
    let dir = create_tmp_git_repo();
    let weird_name = "weird\nname.ts";
    fs::write(dir.path().join(weird_name), "x").unwrap();
    git_in(dir.path(), &["add", "--", weird_name]);
    let out = run(&["compute"], dir.path());
    assert_ok(&out);
    assert_eq!(stdout_str(&out).trim().len(), 64);
}

#[cfg(unix)]
#[test]
fn compute_skips_broken_symlinks() {
    use std::os::unix::fs::symlink;
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let h1 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();

    // Create a symlink pointing to a nonexistent file (broken symlink).
    symlink("nonexistent", dir.path().join("link.ts")).unwrap();
    let h2 = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();
    // The link is listed by ls-files but unreadable; should silently skip and
    // produce the same hash as before.
    assert_eq!(h1, h2);
}

// ─── save ────────────────────────────────────────────────────────────────

#[test]
fn save_creates_hash_file_in_git_dir() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));
    let hash_path = dir.path().join(".git/precommit-verify-hash");
    let content = fs::read_to_string(hash_path).expect("hash file exists");
    assert_eq!(content.trim().len(), 64);
}

#[test]
fn save_writes_hash_matching_compute() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));
    let saved = fs::read_to_string(dir.path().join(".git/precommit-verify-hash"))
        .unwrap()
        .trim()
        .to_string();
    let computed = stdout_str(&run(&["compute"], dir.path()))
        .trim()
        .to_string();
    assert_eq!(saved, computed);
}

#[test]
fn save_rejects_when_not_via_package_manager() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "x").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let out = run_without_pkg_manager(&["save"], dir.path());
    assert_fail_with(&out, "package.json script");
}

// ─── check ───────────────────────────────────────────────────────────────

#[test]
fn check_succeeds_when_hash_matches() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));
    let out = run(&["check"], dir.path());
    assert_ok(&out);
    assert!(stdout_str(&out).contains("verified"));
}

#[test]
fn check_fails_when_no_hash_file() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "x").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    let out = run(&["check"], dir.path());
    assert_fail_with(&out, "hash not found");
}

#[test]
fn check_fails_when_files_changed_since_save() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));
    fs::write(dir.path().join("a.ts"), "const a = 999;\n").unwrap();
    let out = run(&["check"], dir.path());
    assert_fail_with(&out, "files changed since last save");
}

// ─── verify-footer ──────────────────────────────────────────────────────

fn write_msg(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join(".git/COMMIT_EDITMSG");
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn verify_footer_appends_check_when_hash_matches() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));

    let msg = write_msg(dir.path(), "feat: add feature\n");
    assert_ok(&run(&["verify-footer", msg.to_str().unwrap()], dir.path()));

    let result = fs::read_to_string(&msg).unwrap();
    assert!(
        result.contains("Verified: precommit-verify \u{2713}"),
        "got: {result}"
    );
    assert!(result.contains("feat: add feature"));
    // Must include a 16-char hash in parentheses.
    assert!(
        result
            .lines()
            .any(|l| l.starts_with("Verified: precommit-verify \u{2713} (") && l.ends_with(')')),
        "footer should have hash in parens; got: {result}"
    );
}

#[test]
fn verify_footer_appends_cross_when_no_hash() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "x").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);

    let msg = write_msg(dir.path(), "feat: x\n");
    assert_ok(&run(&["verify-footer", msg.to_str().unwrap()], dir.path()));

    let result = fs::read_to_string(&msg).unwrap();
    assert!(
        result.contains("Verified: precommit-verify \u{2715}"),
        "got: {result}"
    );
}

#[test]
fn verify_footer_appends_cross_when_hash_mismatches() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));

    fs::write(dir.path().join("a.ts"), "const a = 999;\n").unwrap();

    let msg = write_msg(dir.path(), "feat: x\n");
    assert_ok(&run(&["verify-footer", msg.to_str().unwrap()], dir.path()));

    let result = fs::read_to_string(&msg).unwrap();
    assert!(
        result.contains("Verified: precommit-verify \u{2715}"),
        "got: {result}"
    );
    assert!(!result.contains('\u{2713}'));
}

#[test]
fn verify_footer_appends_triangle_when_unstaged_changes_exist() {
    let dir = create_tmp_git_repo();
    // Stage bad code…
    fs::write(dir.path().join("a.ts"), "const a = BAD;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    // …then fix it in the working tree without staging.
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    // Save the hash based on the working tree (= the good code).
    assert_ok(&run(&["save"], dir.path()));

    let msg = write_msg(dir.path(), "feat: x\n");
    assert_ok(&run(&["verify-footer", msg.to_str().unwrap()], dir.path()));

    let result = fs::read_to_string(&msg).unwrap();
    assert!(
        result.contains("Verified: precommit-verify \u{25b3}"),
        "got: {result}"
    );
    assert!(!result.contains('\u{2713}'));
    assert!(!result.contains('\u{2715}'));
}

#[test]
fn verify_footer_replaces_existing_on_amend() {
    let dir = create_tmp_git_repo();
    fs::write(dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(dir.path(), &["add", "a.ts"]);
    assert_ok(&run(&["save"], dir.path()));

    let msg = write_msg(
        dir.path(),
        "feat: x\n\nVerified: precommit-verify \u{2713} (abcdef0123456789)\n",
    );
    assert_ok(&run(&["verify-footer", msg.to_str().unwrap()], dir.path()));

    let result = fs::read_to_string(&msg).unwrap();
    assert_eq!(
        result.matches("Verified: precommit-verify").count(),
        1,
        "expected exactly one Verified line; got: {result}"
    );
    assert!(
        !result.contains("abcdef0123456789"),
        "old hash should be replaced"
    );
}

#[test]
fn verify_footer_skips_when_source_is_merge() {
    let dir = create_tmp_git_repo();
    let msg = write_msg(dir.path(), "Merge branch 'main'\n");
    assert_ok(&run(
        &["verify-footer", msg.to_str().unwrap(), "merge"],
        dir.path(),
    ));
    let result = fs::read_to_string(&msg).unwrap();
    assert_eq!(result, "Merge branch 'main'\n");
    assert!(!result.contains("Verified"));
}

#[test]
fn verify_footer_skips_when_source_is_squash() {
    let dir = create_tmp_git_repo();
    let msg = write_msg(dir.path(), "Squash commit\n");
    assert_ok(&run(
        &["verify-footer", msg.to_str().unwrap(), "squash"],
        dir.path(),
    ));
    let result = fs::read_to_string(&msg).unwrap();
    assert_eq!(result, "Squash commit\n");
    assert!(!result.contains("Verified"));
}

// ─── error handling ─────────────────────────────────────────────────────

#[test]
fn errors_when_not_in_git_repo() {
    let non_git = tempfile::tempdir().unwrap();
    let out = run(&["compute"], non_git.path());
    assert_fail_with(&out, "not a git repository");
}

// ─── worktree (codex-reviewer 指摘の path 解決動作確認) ─────────────────

#[test]
fn save_works_inside_a_worktree() {
    let main_dir = create_tmp_git_repo();
    fs::write(main_dir.path().join("a.ts"), "const a = 1;\n").unwrap();
    git_in(main_dir.path(), &["add", "a.ts"]);
    git_in(main_dir.path(), &["commit", "-m", "init"]);

    // Sibling path so the worktree lives outside main_dir (avoids
    // tempdir cleanup issues).
    let worktree_path = main_dir.path().parent().unwrap().join(format!(
        "wt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    git_in(
        main_dir.path(),
        &[
            "worktree",
            "add",
            "-b",
            "feat-x",
            worktree_path.to_str().unwrap(),
        ],
    );

    fs::write(worktree_path.join("b.ts"), "const b = 2;\n").unwrap();
    git_in(&worktree_path, &["add", "b.ts"]);
    assert_ok(&run(&["save"], &worktree_path));

    // The file should land under .git/worktrees/<name>/, not at the main repo's
    // .git/ root. Verifying by listing the per-worktree git dir contents.
    let per_worktree_dir = main_dir.path().join(".git/worktrees");
    let entries: Vec<_> = fs::read_dir(&per_worktree_dir).unwrap().collect();
    assert!(
        !entries.is_empty(),
        "expected per-worktree git dir to exist"
    );
    let wt_subdir = entries.into_iter().next().unwrap().unwrap().path();
    assert!(
        wt_subdir.join("precommit-verify-hash").exists(),
        "expected precommit-verify-hash inside per-worktree git dir, got: {:?}",
        fs::read_dir(&wt_subdir).unwrap().collect::<Vec<_>>()
    );

    // Cleanup (best-effort).
    let _ = fs::remove_dir_all(&worktree_path);
}
