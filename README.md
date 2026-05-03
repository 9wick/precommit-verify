# precommit-verify

[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

[日本語版 README](./README-ja.md)

A small Rust CLI for projects where AI / agent commits are part of the workflow. It makes "commit without actually running the checks" both **harder to do** and **trivial to detect afterward**.

Wired into Git hooks (via [lefthook](https://github.com/evilmartians/lefthook) or similar), it does two things:

1. **Blocks commits** unless the slow checks (lint / build / tests) actually ran on the tree being committed.
2. **Stamps every commit message** with a `Verified: precommit-verify ✓ / △ / ✕` footer. Even if the pre-commit hook is bypassed (`git commit --no-verify`, an agent that knows the trick), the prepare-commit-msg hook still runs and writes `✕` into the message — so the bypass is visible in `git log` and code review afterward.

## Install

### npm (TypeScript / Node projects)

```sh
npm install --save-dev precommit-verify
# or
npx precommit-verify --help
```

The npm package bundles pre-built binaries for Linux (x64, arm64), macOS (x64, arm64), and Windows (x64). Linux binaries are built against glibc 2.35; older distros (Debian 11, CentOS 7/8, Alpine) are not supported in this release.

### Cargo (Rust users)

```sh
cargo install --locked precommit-verify
```

### Pre-built binaries

Download from [Releases](https://github.com/9wick/precommit-check/releases) — `.tar.gz` for Linux/macOS, `.zip` for Windows.

### MSRV

Built with Rust 1.85. The MSRV is bumped when a dependency requires it; we do not chase stable for its own sake.

## Quickstart

Add a `precommit-verify` script to your project that runs your real checks and records the verified state:

```jsonc
// package.json
{
  "scripts": {
    "precommit-verify": "pnpm lint && pnpm build && pnpm test && precommit-verify save"
  }
}
```

Wire it into Git via lefthook:

```yaml
# lefthook.yml
pre-commit:
  commands:
    precommit-verify:
      run: precommit-verify check

prepare-commit-msg:
  commands:
    footer:
      run: precommit-verify verify-footer {1} {2}
```

Workflow:

1. After making meaningful changes, run `pnpm precommit-verify`. It runs your slow checks and, on success, records that "this tree was verified".
2. `git commit` triggers the pre-commit hook, which **blocks the commit** unless the recorded state still matches the tree being committed (no checks are re-run — fast).
3. The prepare-commit-msg hook appends `Verified: precommit-verify ✓ / △ / ✕` to the commit message, recording what state was actually verified at commit time.

Bypass story: `git commit --no-verify` skips step 2 but **does not** skip the prepare-commit-msg hook. So the footer still gets written — as `✕` if checks weren't run — and bypassed commits become obvious in `git log`.

## Subcommands

| Command | What it does |
|---|---|
| `precommit-verify save` | Record that the current tree has been verified. **Must be invoked via a package-manager script** (npm/pnpm/yarn/bun); rejects direct invocation to make the workflow explicit. |
| `precommit-verify check` | Verify that the tree being committed matches the last recorded verified state. Exits non-zero if it doesn't, or if nothing has been recorded yet. |
| `precommit-verify verify-footer <msg-file> [source]` | Append (or replace) a `Verified: precommit-verify ...` footer in the commit message file. Skips when `source` is `merge` or `squash`. |
| `precommit-verify compute` | Print an internal fingerprint of the current tree. Debugging only — you should not need this in normal use. |

### Footer states

| Marker | Meaning |
|---|---|
| ✓ | Checks passed on exactly this tree, no further edits since. |
| △ | Checks passed on this tree, but the working tree has drifted since (typical: edits made after the last `precommit-verify` run, not yet re-verified). |
| ✕ | No record of checks running, or the tree changed since checks last passed. **Treat as unverified.** |

## What counts as a change

The tool watches every file Git can see in your working tree:

- All tracked files
- All untracked files

These don't count as changes:

- Files matched by `.gitignore` (or by `.git/info/exclude`, your global gitignore, or any other `core.excludesFile`)
- Markdown files (`.md`) — documentation edits don't affect lint / build / test outcomes, so they don't invalidate the verified state
- Files that can't be opened (broken symlinks, permission errors) — silently ignored, not treated as a failure

### Customizing the file set

Not currently supported. The lists above are baked in. If you need to:

- exclude additional file patterns (e.g. snapshots, generated code),
- or stop excluding `.md`,

please open an issue describing the use case. A config file is on the table but hasn't been done yet — there's no point shipping a config surface before someone needs it.

## FAQ

**`precommit-verify save` failed with "save must be called via a package manager script". Why?**

The `save` command intentionally refuses direct invocation, so the verified state is only ever updated alongside your real checks. Wrap it in a `package.json` script (or your package manager's equivalent) and run it that way:

```jsonc
{ "scripts": { "precommit-verify": "pnpm test && precommit-verify save" } }
```

Then run `pnpm precommit-verify`, not `precommit-verify save` directly.

**Does it work in a Git worktree?**

Yes. Each worktree tracks its own verified state independently, so running `save` in one worktree doesn't affect commits in another.

**I edited a README. Do I need to re-run `save`?**

No. Markdown files (`.md`) don't count as changes for this tool, since editing them doesn't affect lint / build / test outcomes.

**What happens on `git commit --amend`?**

The existing `Verified: precommit-verify ...` line is replaced (not duplicated) with the current state. So if you amend without re-running `save`, the footer reflects whatever your working tree looks like now.

**What happens on a merge or squash commit?**

`verify-footer` skips them — no footer is added or modified, since those messages are generated by Git's merge / squash flow rather than by you.

## License

MIT — see [LICENSE](./LICENSE).
