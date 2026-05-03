# precommit-check

[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

[日本語版 README](./README-ja.md)

A small Rust CLI that records the verified state of a Git repository in a hash file, then re-checks it at commit time and stamps the commit message with a `Verified:` footer. Designed to be wired into [lefthook](https://github.com/evilmartians/lefthook) (or any other Git hooks runner).

The point: run your slow checks (lint / build / tests) **once**, write a hash, and let every subsequent commit confirm cheaply that nothing has changed since — including untracked files. If you change anything between checks and the commit, the footer flips to ✕ so reviewers can tell.

## Install

```sh
cargo install --locked precommit-check
```

> An npm wrapper (`npx precommit-check`) is planned in a follow-up release.

## Quickstart

Add a `precommit-check` script to your project that runs your real checks and then saves a hash:

```jsonc
// package.json
{
  "scripts": {
    "precommit-check": "pnpm lint && pnpm build && pnpm test && precommit-check save"
  }
}
```

Wire it into Git via lefthook:

```yaml
# lefthook.yml
pre-commit:
  commands:
    precommit-check:
      run: precommit-check check

prepare-commit-msg:
  commands:
    footer:
      run: precommit-check verify-footer {1} {2}
```

Workflow:

1. You run `pnpm precommit-check` after a meaningful change. It runs the slow checks, then writes the repo hash to `.git/precommit-check-hash`.
2. `git commit` triggers the pre-commit hook, which runs `precommit-check check` — fast: it just rehashes the working tree and compares.
3. The prepare-commit-msg hook appends a footer like `Verified: precommit-check ✓ (16-char-hash)` so the commit message records what state was verified.

If anything has been edited since the last `save`, the check fails and the commit is blocked. If the check passes but you have unstaged changes, the footer becomes ` △` (verified the staged + working state, but the working tree drifted from index — a soft warning).

## Subcommands

| Command | What it does |
|---|---|
| `precommit-check save` | Compute the repo hash and write it to `.git/precommit-check-hash`. **Must be invoked via a package-manager script** (npm/pnpm/yarn/bun); rejects direct invocation to make the workflow explicit. |
| `precommit-check check` | Recompute the hash and compare to the saved one. Exits non-zero if they differ or if no hash file exists. |
| `precommit-check compute` | Print the current hash to stdout. Useful for debugging / scripting. |
| `precommit-check verify-footer <msg-file> [source]` | Append (or replace) a `Verified: precommit-check ...` footer in the commit message file. Skips when `source` is `merge` or `squash`. |

### Footer states

| Marker | Meaning |
|---|---|
| ✓ | Hash matches and the working tree is clean. |
| △ | Hash matches but unstaged edits exist (the staged commit is verified; the working tree has drifted). |
| ✕ | No hash recorded, or the hash does not match the current state. |

## How the hash is computed

- Source: `git -c core.quotepath=false ls-files -z -c -o --exclude-standard` (tracked + untracked, NUL-delimited so paths with newlines or non-UTF-8 bytes are handled correctly).
- Files ending in `.md` are skipped (documentation churn shouldn't invalidate the verified state).
- Files are sorted by byte order, then for each file the hash absorbs `<filename> \0 <file content> \0` into a single [blake3](https://github.com/BLAKE3-team/BLAKE3) digest. The NUL delimiters make `(filename, content)` boundaries unambiguous so two different repositories cannot collide just because their bytes shift between the two fields.
- Files that fail to open (broken symlinks, permission errors) are skipped silently.
- The hash file path is resolved via `git rev-parse --git-path precommit-check-hash`, so it lands in the right place inside [worktrees](https://git-scm.com/docs/git-worktree) and submodules.

## FAQ

**Why is `save` blocked when I run it directly?**
To prevent accidental hash updates that bypass the slow checks. The intent is "the script that runs the checks also runs save"; we enforce that by requiring the `npm_lifecycle_event` / `npm_execpath` env vars that package managers set.

**Will it work in a Git worktree?**
Yes — the hash file is stored per worktree (`.git/worktrees/<name>/precommit-check-hash`), so each worktree has its own state.

**Why blake3 and not SHA-256?**
Performance. The whole binary needs to start and finish in well under 30ms to feel snappy at commit time, and blake3's SIMD code easily wins over SHA-256 here. The hex digest is the same 64-character length, so footer rendering is unchanged.

## Migrating from `prepush-hash`

This tool was previously published as `prepush-hash` inside the [`@9wick/eslint-plugin-strict-type-rules`](https://github.com/9wick/eslint-strict-type-rules) package. It has now been split out as its own project, rewritten in Rust, and renamed to match its actual lifecycle (pre-commit, not pre-push).

Differences:

- Hash file: `.git/prepush-hash` → `.git/precommit-check-hash` (the old one is ignored — it is safe to delete).
- Footer text: `Verified: prepush ...` → `Verified: precommit-check ...` (old footers in past commits are left alone; new commits use the new prefix).
- Hash algorithm: SHA-256 → blake3. Hashes from the old tool are not comparable; you must run `save` once after upgrading.

## License

MIT — see [LICENSE](./LICENSE).
