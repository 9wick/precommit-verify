# precommit-check: prepush-hash CLI の Rust 実装（Phase 1）

> **Scope**: 本セッションでは **Phase 1（Rust 実装本体 + テスト + README）まで**。CI/配布パイプライン（Phase 2）と eslint-strict-type-rules 側の削除（Phase 3）は別タスク。

## Context

`@9wick/eslint-plugin-strict-type-rules` リポジトリには、ESLint プラグイン本体に加えて `prepush-hash` という CLI ツール（`bin/prepush-hash.mjs`、~140 行 + テスト 333 行）が同居している。これは pre-commit hook で「最後にチェック（lint/build/test）が通ったときと比べてファイルが変わっていないか」を確認し、commit message に `Verified: prepush ✓` 風の footer を追記する仕組み。

この同居には 3 つの問題がある：

1. **関心事が混在**（A）: ESLint ルールだけ欲しい利用者と、precommit hook 機能だけ欲しい利用者の双方が、関係ない側にも依存させられる。
2. **起動速度が UX に直結**（B）: pre-commit hook は commit のたびに走る。Node.js 起動 (~100ms) は `verify-footer` のような軽量操作で体感に出る。
3. **命名と実態の乖離**（C）: 名前は `prepush` だが、実際は pre-commit / prepare-commit-msg hook で実行している。命名がメンタルモデルを誤らせる。

これを **空の `precommit-check` リポジトリに Rust で再構築** する。本セッションは Rust 実装本体まで。

## Purpose Hierarchy

```
[A 関心事分離] OSS 利用者が必要な機能だけを最小依存で導入できる
  ↓ 仮定: ESLint ルールと CLI は本当に独立している（相互参照なし、確認済み）
[B 起動速度] commit のたびに hook の遅さを意識させない
  ↓ 仮定: CLI 単体で 30ms 以下なら verify-footer の体感が滑らか
[C 命名一貫性] ツール名から動作タイミング (pre-commit) と用途を直感的に理解できる
  ↓ 仮定: 旧名 prepush の痕跡を完全に消さないと混乱が残る
[配布] (Phase 2) crates.io + npm wrapper の二系統で TS でも非 TS でも導入容易
```

## Pre-flight checks（実装開始時に必須）

実装着手の最初に名前の空き状況を確認する。**`precommit-check` という名前が両方で空いていなければユーザーに別名を相談**。

```bash
cargo search precommit-check
npm view precommit-check
```

ユーザー指示: npm scope は **無し**（`precommit-check`）で進める。空いていなければ別名を提示して相談する。crates.io と npm で同じ名前にすることが望ましい（命名一貫性 = 目的 C）。

## Repository Layout（Phase 1 で作成するファイル）

```
precommit-check/
├── .gitignore                # target/, *.tsbuildinfo (空 ignore)
├── Cargo.toml                # 単一 crate、exact pin (=x.y.z)、unsafe_code = forbid、clippy::pedantic = warn
├── Cargo.lock                # cargo build で生成
├── rust-toolchain.toml       # channel = "1.85"
├── rustfmt.toml              # edition 2021, imports_granularity = Crate, group_imports = StdExternalCrate
├── LICENSE                   # MIT (layer-conform と同じ)
├── README.md                 # CLI の使い方、lefthook 設定例（cargo install のみ言及、npm wrapper は "Coming soon"）
├── README-ja.md              # 日本語版
├── src/
│   ├── main.rs               # clap CLI entrypoint + 各サブコマンド実装
│   ├── hash.rs               # blake3 ハッシュ計算（pure 部分は #[cfg(test)] で unit test）
│   ├── git.rs                # git ls-files (-z) / rev-parse / diff の subprocess wrapper
│   └── footer.rs             # commit msg footer 生成・置換（pure function 中心）
└── tests/
    └── cli.rs                # assert_cmd ベースの統合テスト（CLI 経由の挙動のみ。pure 関数は src 内 unit test）

> **lib.rs を作らない** (rust-reviewer 指摘・YAGNI): integration test は assert_cmd でバイナリを subprocess 起動するため、lib 公開の必然性はない。公開すると API 互換責務が発生 + バイナリサイズ増。再利用ニーズが出てから追加する。
```

**Phase 1 では作らない**（後続タスク）:
- `.github/workflows/*` (CI/release)
- `release-plz.toml`
- `npm/` ディレクトリ全体（メタパッケージ・platform package・shim）

ただし将来追加しやすい構造を選ぶ（単一 crate、配布形式に依存しない src 配置）。

## CLI 仕様（旧版から不変の部分）

| サブコマンド | 動作 |
|---|---|
| `save` | git ls-files + 各ファイル content を hash → `.git/precommit-check-hash` に保存。**`npm_lifecycle_event` または `npm_execpath` がないと拒否**（旧仕様維持。env 経由で package manager 内実行を担保するため） |
| `check` | 保存済み hash と現在の hash を比較。不一致なら exit 1 + 何が不一致かのメッセージ |
| `compute` | 現在の hash を stdout に出力 |
| `verify-footer <msg-file> [source]` | commit msg に `Verified: precommit-check ✓ / △ / ✕ (hash16桁)` を追記。`source == "merge"` または `"squash"` は skip。amend 時は既存 footer を置換（重複防止） |

### CLI 仕様の変更点（旧 → 新）

| 項目 | 旧 | 新 |
|---|---|---|
| バイナリ名 | `prepush-hash` | `precommit-check` |
| hash file | `.git/prepush-hash` | `.git/precommit-check-hash` |
| ハッシュアルゴリズム | SHA-256 (`node:crypto`) | **blake3** (SIMD で高速、出力は同じ 64 hex) |
| footer 文言 | `Verified: prepush ✓` | `Verified: precommit-check ✓` |
| エラーメッセージ | `prepush-hash:` prefix | `precommit-check:` prefix |
| `.md` 除外 | ✓ | ✓（維持） |
| package manager ガード | ✓ | ✓（`save` のみ、env で判定） |

## 実装方針

### git は subprocess、libgit2 は使わない
理由: `git2` crate は libgit2 リンク + 起動コスト。今回必要なのは `git ls-files` / `git diff` / `git rev-parse` のみ。`std::process::Command` で十分、TS 版と同じファイルセット・同じ env を維持できる。

### hash file の保存先解決（codex 指摘反映）
旧 TS 版は `git rev-parse --git-dir` + `path.join` で `.git/prepush-hash` を直接組み立てていたが、worktree や submodule では `.git` がディレクトリでなくファイル（pointer）の場合がある。

**対策**: `git rev-parse --git-path precommit-check-hash` を使う。git が適切な場所（worktree なら `.git/worktrees/<name>/precommit-check-hash`、通常なら `.git/precommit-check-hash`）を返す。

### git ls-files の安全なパース（rust + codex 指摘反映）
旧 TS 版は `git ls-files -c -o --exclude-standard` の出力を改行 split していたが、改行を含むファイル名や非 UTF-8 パスで壊れる（Windows 配布も視野）。

**対策**: `git -c core.quotepath=false ls-files -z -c -o --exclude-standard` で **NUL 区切り**出力 → `bstr::ByteSlice::split_str(b"\0")` でバイト列のまま処理。`String` 化は最後の `.md` 拡張子チェックでのみ（拡張子部分は ASCII 前提で OK）。

### ハッシュ計算
- `git ls-files -z` 出力 → NUL split → 空除外 → `.md` 除外 → sort（バイト列比較）
- 各ファイルについて:
  - `hasher.update(filename_bytes)`
  - `hasher.update(b"\0")` ← **境界明確化**（codex 指摘: filename と content の連結だけだと理論上衝突可能。NUL delimiter で防ぐ）
  - `hasher.update(file_content)`
  - `hasher.update(b"\0")`
- 読めないファイル（broken symlink、循環 symlink 等）は skip（旧版と同じ挙動）
- ファイル I/O はストリーミング（`std::fs::File` + `Hasher::update_reader` 相当）で大ファイルでもメモリ展開しない
- 並列化（rayon）は YAGNI、ベンチで必要性確認後
- 出力は blake3 の hex 64 文字

### `save` の package manager ガード
- `std::env::var("npm_lifecycle_event").is_ok() || std::env::var("npm_execpath").is_ok()` で判定
- false なら stderr に "save must be called via a package manager script" 出して exit 1

### `verify-footer`（pure function 分離 + regex 不使用）
tdd-reviewer 指摘: pure 部分とファイル I/O を分離し、unit test を pure 部分に集中させる。

```
// pure functions (footer.rs)
enum VerifyStatus { Verified(String), Unverified, Stale }
fn build_footer(status: &VerifyStatus) -> String  // ✓/△/✕ 文字列生成
fn strip_existing_footer(msg: &str) -> String    // 既存 Verified 行除去（regex なし）
fn embed_footer(msg: &str, footer: &str) -> String  // trim_end + \n\n + footer + \n

// CLI 側 (main.rs)
fn verify_footer(msg_file: &Path, source: Option<&str>) -> Result<()> {
    if matches!(source, Some("merge" | "squash")) { return Ok(()); }
    let msg = fs::read_to_string(msg_file)?;
    let stripped = strip_existing_footer(&msg);
    let status = compute_status()?;  // hash の状況判定
    let footer = build_footer(&status);
    fs::write(msg_file, embed_footer(&stripped, &footer))?;
    Ok(())
}
```

`strip_existing_footer` の実装: `msg.lines().filter(|l| !l.starts_with("Verified: precommit-check")).join("\n")`。**regex 不使用**（依存削減・起動高速化）。

旧 footer (`Verified: prepush ...`) は **削除しない**（旧 CLI 利用者が混在環境で残る分は別問題、Phase 3 で旧 CLI ごと消える）。precommit-check 自身が書いた行のみ管理する。

### compute_status の分岐
- hash file 無し → `Unverified` → `Verified: precommit-check ✕`
- hash file あり、現在 hash と一致、unstaged 変更なし → `Verified(hash)` ✓ → `Verified: precommit-check ✓ (16桁hex)`
- hash file あり、現在 hash と一致、unstaged 変更あり → `Verified(hash)` △ → `Verified: precommit-check △ (16桁hex)`
- hash file あり、現在 hash と不一致 or compute エラー → `Stale` → `Verified: precommit-check ✕`
  - rust-reviewer 指摘: compute エラー時は `Stale` 扱い（旧 TS の try/catch 相当）。msg ファイル read/write エラーは伝播。

## 依存（Cargo.toml、exact pin）

実装時に `cargo search` で最新版確認して exact pin（`=x.y.z`）。layer-conform 流儀。

| crate | 用途 | 備考 |
|---|---|---|
| `clap` (derive) | CLI parsing | rust-reviewer から「`pico-args` の方が起動速い」指摘あり。**まず clap で進めて、Phase 1 完了時の hyperfine 計測で 30ms 超なら入れ替え検討**（開発体験 trade-off） |
| `blake3` | ハッシュ計算 | SIMD 高速化 |
| `bstr` | NUL 区切りバイト列処理 | `git ls-files -z` 出力の安全パース用 |
| `anyhow` | エラー伝播 | main で `Result<()>` |
| dev: `assert_cmd` | CLI 統合テスト | |
| dev: `predicates` | assert_cmd 述語 | |
| dev: `tempfile` | テスト用 tempdir | |

`thiserror` は使わない（lib 公開エラー型を作らないため `anyhow` で十分）。`regex` も使わない（footer 処理は行ベースで十分、依存削減）。

## Cargo.toml の lints と release profile（layer-conform 準拠 + 起動最適化）

```toml
[package]
# ...
rust-version = "1.85"  # MSRV、cargo がチェック

[profile.release]
lto = "fat"           # 起動速度最適化（rust-reviewer 指摘）
codegen-units = 1
strip = true
panic = "abort"
opt-level = 3

[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
must_use_candidate = "allow"
```

## テスト戦略

**Testing Trophy 構成**:
- **Unit test** (src 内 `#[cfg(test)] mod tests`): pure function（footer.rs の `build_footer` / `strip_existing_footer` / `embed_footer`、hash の決定論性 helper）。subprocess 起動コスト無し、表形式 (table-driven) で大量ケース回せる。
- **Integration test** (tests/cli.rs): `assert_cmd` で CLI を subprocess 起動、stdout/exit code 検証。**外部境界の挙動**（git subprocess、ファイル I/O、env var）を実体で叩く。**モック皆無**（tdd-reviewer 指摘: 外部境界も実体で叩く方針を明文化）。

旧 `tests/cli/prepush-hash.test.ts`（333 行、22 ケース）を踏襲しつつ、ケースを下記表に振り分ける：

| describe | ケース | unit / integration |
|---|---|---|
| compute | 一貫した hash | unit (build_footer 経由) + integration 1 |
| compute | `.md` 除外 | integration |
| compute | 内容変更で hash 変化 | integration |
| compute | rename で hash 変化 | integration |
| compute | untracked 含有 (`-c -o --exclude-standard` の `-o`) | integration |
| save | hash file 作成（`git rev-parse --git-path` で解決された path） | integration |
| save | hash と compute 出力一致 | integration |
| save | package manager 経由でないと拒否 | integration |
| check | 一致で exit 0 | integration |
| check | hash file 無しで exit 1 | integration |
| check | 変更後で exit 1 | integration |
| verify-footer | ✓ 追記 (build_footer + embed_footer の組合せ) | unit + integration 1 |
| verify-footer | ✕ when no hash | unit (build_footer) |
| verify-footer | ✕ when 不一致 | unit |
| verify-footer | △ when unstaged 有り | unit + integration 1 |
| verify-footer | amend で既存 Verified 置換（重複防止） | unit (strip_existing_footer) |
| verify-footer | merge skip | integration |
| verify-footer | squash skip | integration |
| error handling | 非 git ディレクトリで exit 1 | integration |

### 追加エッジケース（tdd-reviewer 指摘）

| ケース | テスト種別 | 理由 |
|---|---|---|
| CRLF / 改行末尾無し commit msg | unit (embed_footer) | trim_end + `\n\n` 挙動の確定 |
| 既存 Verified 行が複数行ある | unit (strip_existing_footer) | 過去の amend バグで重複した場合 |
| `.gitignore` で除外されたファイルが hash 不参加 | integration | `--exclude-standard` 動作確認 |
| 空ファイル | integration | hash 計算で skip しない（boundary を NUL で明示してるので OK） |
| シンボリックリンク（broken / 循環） | integration | read 失敗 → skip |
| 改行を含むファイル名 | integration | `-z` パース動作確認 |
| worktree 内での save/check | integration | `git rev-parse --git-path` 解決確認 |

### ヘルパー関数 (tests/cli.rs)
- `create_tmp_git_repo() -> TempDir`: tempdir 作成 + `git init -b main` + user config
- `run_cli(cmd: &str, dir: &Path) -> Output`: `assert_cmd::Command` で `precommit-check <cmd>` 実行
  - env に `npm_lifecycle_event=precommit` セット
  - **`GIT_CONFIG_GLOBAL=/dev/null` セット** (rust-reviewer 指摘): `~/.gitconfig` の `commit.gpgsign` 等の影響を排除して再現性確保
- `run_cli_fail(cmd: &str, dir: &Path) -> Output`: 失敗を期待

### Phase 1 では入れない（後付けで導入可）
- `insta` (snapshot test): footer 文言や error message の一括レビューに有用だが、22 ケースなら手書き assert で十分
- `rstest` / `#[test_case]` (parametrized): 同上、必要になってから
- `criterion` ベンチ: hyperfine で sanity check するので不要

## 実装タスク順序

precommit-check リポジトリは空コミット状態。**main で直接初期コミット相当の作業**（コミット自体はユーザー指示まで保留 ＝ CLAUDE.md の「git commit は明示指示まで NEVER」に従う）。

worktree は切らない（空コミットで切れない、かつ単一セッションなので並行調整不要）。

**進め方の原則** (tdd-reviewer 指摘反映): pure function を持つモジュール（hash / footer）は **テスト → 実装** の順（Red → Green）。subprocess を叩く git.rs と CLI 統合部分は実装 → 統合テストで確認。

1. **Pre-flight**: `cargo search precommit-check` / `npm view precommit-check` で名前空き確認。空いてなければユーザーに相談して別名検討
2. `.gitignore` / `LICENSE`
3. `Cargo.toml`（`[profile.release]` 最適化、`rust-version`、lints 含む） / `rust-toolchain.toml` / `rustfmt.toml`
4. `cargo init --bin --vcs none` 相当のスキャフォールド（`src/main.rs` 最小）→ `cargo build` が通る状態に
5. `src/footer.rs`: pure function (`build_footer`, `strip_existing_footer`, `embed_footer`) と `#[cfg(test)] mod tests` (CRLF / 複数 Verified 行 / 既存 footer 置換 等のエッジケース含む)
6. `src/git.rs`: subprocess wrapper
   - `ensure_git_repo() -> Result<()>`
   - `hash_file_path() -> Result<PathBuf>` ← **`git rev-parse --git-path precommit-check-hash`**
   - `ls_files() -> Result<Vec<Vec<u8>>>` ← **`git ls-files -z`** + bstr で NUL split
   - `has_unstaged_changes() -> Result<bool>`
7. `src/hash.rs`: `compute_hash() -> Result<String>`（git.rs に依存、blake3、`.md` 除外、NUL delimiter で境界明確化）
8. `src/main.rs`: clap でサブコマンドパース、`save` / `check` / `compute` / `verify_footer` の各実装（pure 部分は footer.rs 呼び出し）
9. `tests/cli.rs`: 統合テスト（assert_cmd）。**`GIT_CONFIG_GLOBAL=/dev/null` で隔離**
10. `cargo fmt && cargo clippy -- -D warnings && cargo test` 全 pass
11. `cargo build --release` 成功確認
12. **sanity bench** (rust-reviewer 指摘): `hyperfine './target/release/precommit-check compute'` を tempdir で 1 回計測、起動 + compute が概ね 30ms 以下を確認。超えていたら clap → pico-args 入れ替え検討
13. `README.md` / `README-ja.md`
14. ユーザーに完了報告（コミットは指示まで保留）

## 検証方法

- `cargo test` で全テスト pass
  - unit: footer.rs / hash.rs の pure function （CRLF / 複数 Verified 行 / 境界 NUL 含むエッジケース全部）
  - integration: 旧 TS の 22 ケース + 追加エッジケース（worktree、改行ファイル名、空ファイル、symlink）= 30 ケース前後
- `cargo clippy -- -D warnings` warning ゼロ
- `cargo fmt --check` 差分ゼロ
- `cargo build --release` 成功
- 手動 sanity check:
  - 新規 tempdir で `git init -b main` → 適当なファイル add → `./target/release/precommit-check compute` が hex 64 文字を返すこと、複数回呼んで同一
  - **worktree 内で `save` → `.git/worktrees/<name>/precommit-check-hash` に保存されること**（codex 指摘の path 解決動作確認）
  - `hyperfine` で起動 + compute 時間計測、30ms 以下なら OK（超えてたらレビュー反映課題に追加）

## Out of Scope（後続タスク）

### Phase 2: 配布パイプライン（別セッション）
- `release-plz.toml`、`.github/workflows/{ci,release-plz,release-binaries}.yml`
- `npm/` ディレクトリ（メタパッケージ + 各 platform package + shim）
- 戦略: optional dependencies 方式（biome / esbuild と同じ）。`postinstall` DL 方式は採らない（CI/オフラインで死ぬ）
- platform 対象: linux x64/arm64, darwin x64/arm64, win32 x64
- 必要 secret: `NPM_TOKEN`, `CARGO_REGISTRY_TOKEN`, `RELEASE_PLZ_TOKEN`

### Phase 3: eslint-strict-type-rules 側整理（別セッション、**npm publish 完了後**に実施）
- `bin/prepush-hash.mjs` 削除
- `tests/cli/prepush-hash.test.ts` 削除
- `package.json`: `bin`/`files` から bin 削除、**`precommit-check` を `devDependency` に追加**（ユーザー確定方針）、`scripts.prepush` を `... && precommit-check save` に書き換え
- `README.md`: prepush-hash セクション削除し「[precommit-check](https://github.com/9wick/precommit-check) に独立」と誘導

## Files to Create（Phase 1）

precommit-check リポジトリに新規作成:

```
.gitignore
Cargo.toml
rust-toolchain.toml
rustfmt.toml
LICENSE
README.md
README-ja.md
src/main.rs
src/hash.rs
src/git.rs
src/footer.rs
tests/cli.rs
```

加えて `cargo build` 実行で生成: `Cargo.lock`, `target/`（target は `.gitignore` 済み）。

## Files to Modify（Phase 1）

なし（precommit-check は空リポジトリ、eslint-strict-type-rules は触らない）。
