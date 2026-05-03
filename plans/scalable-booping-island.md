# precommit-check Phase 2: 配布パイプライン（crates.io + npm）

> **Scope**: Phase 2 のみ。crates.io publish + GitHub Release への binary 添付 + npm wrapper（**単一パッケージに 5 binary 同梱方式**）。
> Phase 3（eslint-strict-type-rules 側の旧 CLI 削除）は別タスク。

## Context

Phase 1 で空リポジトリに Rust 実装本体（src 472 行 + tests 520 行）を構築完了。crate metadata（name/version/license/MSRV/keywords/categories）と `[profile.release]` 最適化（lto fat / strip / opt-level 3 / panic abort）まで整っており、配布準備は揃っている。

Phase 2 ではこれを **2 系統で配布**する:

1. **crates.io**: Rust ユーザー向け（`cargo install precommit-check`）
2. **npm（`precommit-check`、unscoped）**: TS / Node ユーザー向け（`npx precommit-check`、`devDependency`）

### 配布が満たすべき目的

Phase 1 の Purpose Hierarchy の最終段「配布」を実装する。

```
[A 関心事分離] ESLint ルールと無関係に CLI だけ install できる
  → npm の devDependency として独立 install 可能になる
[B 起動速度] hook 体感を遅くしない
  → Rust binary を直 spawn（Node.js wrapper 経由でも spawnSync 1 回）
[C 命名一貫性] crates.io / npm で同じ `precommit-check`
  → unscoped 名で両方確保（Phase 1 で空き確認済み）
```

## ユーザー確定事項（プラン作成時に確認済み）

| 論点 | 決定 | 理由 |
|---|---|---|
| platform 別 npm パッケージ分割 | **しない**。単一 `precommit-check` パッケージに 5 binary 同梱 | binary は小さい（~1-3MB/個 × 5 = ~10MB）。biome / esbuild の optional deps 方式は overkill。複数 package 間の publish race condition / 版番号 drift を回避 |
| publish 認証 | **Trusted Publishing / OIDC**（crates.io + npm 両方） | 2025-2026 標準。token rotation 不要。CI 側は `id-token: write` permission のみ |

### Trusted Publishing 利用に伴う事前手動設定（ユーザー作業）

実装前にユーザー側で以下の設定が必要。実装開始時に手順を案内する。

#### Phase 2-A: 初回手動 publish（**必須**）

release-plz 公式 docs で明示: **「新規 crate の初回 publish は trusted publishing では出来ない」**（crates.io 側の制約）。npm も同様に未存在 package には trusted publisher を紐付けられない。**最初の v0.1.0 だけは手動 publish** が必要:

1. ローカルで `cargo login <token>` → `cargo publish` で v0.1.0 を crates.io に publish
2. 後述の launcher 実装を済ませた状態で、ローカルで 5 OS 分の binary を集めるのは現実的に不可なため、**初回 npm publish 用に「現在の環境（linux x64）でビルドした binary 1 つだけ含む 0.1.0」を `npm publish --access public` で publish**。OR、CI で `release-binaries.yml` を tag 手動 push で先に走らせて GitHub Release だけ生成 → 5 binary を手元に download → npm publish
3. 上記いずれか実施後、registry 側で package が認知されれば trusted publisher 設定が可能になる

→ **シンプル化提案**: 初回も CI 経由で publish したい場合、`release-plz.yml` と `release-binaries.yml` を **token 経由（CARGO_REGISTRY_TOKEN, NPM_TOKEN）で 1 回 release 完走** → 設定後に workflow を OIDC 化、の二段階で進める。Phase 2 実装はこの順。

#### Phase 2-B: Trusted Publisher 設定（v0.1.0 publish 後）

1. **crates.io** (https://crates.io/crates/precommit-check/settings — owner として要 login):
   - Trusted Publishers セクションで GitHub Actions 連携を追加
   - Repository owner: `9wick`, Repository name: `precommit-check`
   - Workflow filename: `release-plz.yml`
   - Environment: 空欄（GitHub Environments 使う場合のみ）
2. **npmjs.com** (https://www.npmjs.com/package/precommit-check/access — owner として要 login):
   - "Trusted Publishers" → GitHub Actions 追加
   - GitHub repository: `9wick/precommit-check`
   - Workflow filename: `release-binaries.yml`
3. **GitHub repo settings** (https://github.com/9wick/precommit-check/settings/actions):
   - Workflow permissions: "Read and write permissions" を許可
   - "Allow GitHub Actions to create and approve pull requests" を ON（release-plz が release PR を作るため）
4. **`RELEASE_PLZ_TOKEN` PAT 作成・登録**:
   - GitHub fine-grained PAT, scope: `Contents: Read and write` + `Pull requests: Read and write`、対象 repo: `9wick/precommit-check`
   - Settings → Secrets → Actions に `RELEASE_PLZ_TOKEN` で登録
   - これは OIDC 化後も**必須**: 通常の `GITHUB_TOKEN` で push した tag は他 workflow を起動しないため、release job が `release-binaries.yml` を tag push 経由で起動するには PAT が必要

#### Phase 2-C: workflow を OIDC 化（v0.1.1 以降）

`release-plz.yml` から `CARGO_REGISTRY_TOKEN` 削除、`release-binaries.yml` から `NODE_AUTH_TOKEN` 削除、両方とも `id-token: write` permission のみで動作。Phase 2 内で workflow ファイルは最初から OIDC 前提で書くが、Phase 2-A 時点では token 経由で 1 回回す（同一 workflow が両モード対応するように env 経由で切替）。

## アーキテクチャ概要

```
   ┌─────────────────────┐
   │  push to main       │
   └──────────┬──────────┘
              │
              ▼
   ┌─────────────────────────┐         ┌──────────────────┐
   │  ci.yml (PR/push 全部)  │         │  release-plz.yml │
   │  - cargo fmt --check    │         │  - 通常: PR 作成 │
   │  - cargo clippy -D warn │         │  - merge 後:     │
   │  - cargo test           │         │    cargo publish │
   │  - matrix: linux/mac/win│         │    git tag v*    │
   └─────────────────────────┘         └────────┬─────────┘
                                                │ git tag v*
                                                ▼
                                  ┌─────────────────────────────┐
                                  │  release-binaries.yml       │
                                  │  on: push tag 'v*'          │
                                  │                             │
                                  │  Job1 build (matrix x5):    │
                                  │    cargo build --release    │
                                  │    → tar.gz (zip on win)    │
                                  │    → upload-artifact        │
                                  │                             │
                                  │  Job2 release (needs Job1): │
                                  │    download all artifacts   │
                                  │    softprops/action-gh-     │
                                  │      release v2 で attach   │
                                  │                             │
                                  │  Job3 npm-publish (needs    │
                                  │      Job2):                 │
                                  │    download all artifacts   │
                                  │    binaries/ に展開         │
                                  │    package.json 生成        │
                                  │    npm publish (OIDC)       │
                                  └─────────────────────────────┘
```

## Repository Layout（Phase 2 で追加するファイル）

```
precommit-check/
├── release-plz.toml              # 単一 crate 設定、tag 形式 v{version}
├── .github/
│   └── workflows/
│       ├── ci.yml                # PR/push: fmt+clippy+test (matrix 5 OS)
│       ├── release-plz.yml       # main push: release PR + crates.io publish
│       └── release-binaries.yml  # tag push: build + GitHub Release + npm publish
├── npm/
│   ├── package.json              # name=precommit-check, bin=lib/index.js
│   ├── lib/
│   │   └── index.js              # 起動 launcher（process.platform/arch で binary 選択）
│   ├── README.md                 # ルート README.md への symlink ではなく短い install 案内（npm 表示用）
│   └── .gitignore                # binaries/（CI で生成、commit しない）
├── README.md                     # 既存。npm wrapper の install 説明を追記
└── README-ja.md                  # 同上
```

`npm/binaries/` は `release-binaries.yml` 内で動的に作る（GitHub Release から download → 展開 → npm publish に含めるが repo には commit しない）。

## ファイル詳細

### `release-plz.toml`（単一 crate 用、公式 single-tag pattern）

公式 docs (https://release-plz.dev/docs/extra/single-tag) の patternに準拠:

```toml
[workspace]
# 単一 crate でも明示的に default を無効化（将来 [[workspace]] dependency 追加時の事故防止）
git_release_enable = false
git_tag_enable = false

[[package]]
name = "precommit-check"
git_tag_name = "v{{ version }}"     # crate 名 prefix を消して `v0.1.2` 形式に
git_tag_enable = true
git_release_enable = true            # release-plz が GitHub Release を生成（changelog 入り）
changelog_update = true
```

- changelog: デフォルト (Keep-a-Changelog 形式) → `CHANGELOG.md` 自動生成
- conventional commits (`feat:` `fix:` `chore:` ...) から semver 自動判定
- `publish` field: 既定 true、明示不要
- **release-plz が GitHub Release を作る** → `release-binaries.yml` の `softprops/action-gh-release` は **既存 release への asset 添付のみ**（`body` を上書きしない）として動作させる

### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  fmt-clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt -- --check
      - run: cargo clippy --all-targets -- -D warnings

  test:
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            git_config_global: /dev/null
          - os: macos-latest
            git_config_global: /dev/null
          - os: windows-latest
            git_config_global: NUL          # Windows は /dev/null 不在
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --release
        env:
          GIT_CONFIG_GLOBAL: ${{ matrix.git_config_global }}

  publish-dryrun:
    name: cargo publish dry-run
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo publish --dry-run
        # crates.io 規約 (keywords 5 個・各 20 char ASCII、categories valid slug 等) を PR 時点で検知

  release-plz-dryrun:
    name: release-plz dry-run
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: release-plz/action@v0.5
        with:
          command: release-pr
          dry_run: true
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

**設計判断**:
- fmt/clippy は ubuntu のみ（OS 非依存）
- test は 3 OS（linux/mac/win）。Windows では `GIT_CONFIG_GLOBAL=NUL`（`/dev/null` 不在）
- MSRV (1.85) で固定。stable 追っかけは YAGNI（README に MSRV ポリシー明記 → 「依存 crate が MSRV を上げたら同調するが、定常的な stable 追従はしない」）
- `cargo publish --dry-run`: keywords/categories 等の crates.io 規約違反を**PR 時点**で検知
- `release-plz dry-run`: PR でしか走らせない（main 上は本物の release-plz job が走る）。 conventional commits 違反や changelog 生成失敗を早期検知

### `.github/workflows/release-plz.yml`

公式 quickstart (https://release-plz.dev/docs/github/quickstart) に準拠した two-job 並列構成。`release` 側で PAT を使い tag push が `release-binaries.yml` を起動できるようにする:

```yaml
name: Release-plz

on:
  push:
    branches: [main]

jobs:
  release-plz-release:
    name: Release-plz release
    runs-on: ubuntu-latest
    permissions:
      contents: write       # tag push
      pull-requests: read
      id-token: write       # crates.io OIDC (Phase 2-C 以降に有効化)
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          persist-credentials: false
          token: ${{ secrets.RELEASE_PLZ_TOKEN }}    # tag push が release-binaries.yml を triggers
      - uses: dtolnay/rust-toolchain@stable
      - uses: release-plz/action@v0.5
        with:
          command: release
        env:
          GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}
          # Phase 2-A (初回): CARGO_REGISTRY_TOKEN 必要
          # Phase 2-C (OIDC 化後): 下行を削除して trusted publishing 有効化
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}

  release-plz-pr:
    name: Release-plz PR
    runs-on: ubuntu-latest
    permissions:
      contents: write
      pull-requests: write
    concurrency:
      group: release-plz-pr-${{ github.ref }}
      cancel-in-progress: false
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
          persist-credentials: false
          token: ${{ secrets.RELEASE_PLZ_TOKEN }}
      - uses: dtolnay/rust-toolchain@stable
      - uses: release-plz/action@v0.5
        with:
          command: release-pr
        env:
          GITHUB_TOKEN: ${{ secrets.RELEASE_PLZ_TOKEN }}
```

**設計判断（公式準拠 + reviewer 指摘反映）**:
- **2 job 並列構成は公式設計**（release-pr が release PR を作る、release が「未 publish の version を検知したら publish」を独立判定）。1 job 化はしない
- `release-plz-release` の `token` / `GITHUB_TOKEN` に `RELEASE_PLZ_TOKEN`（PAT）使用 → release-plz が push する tag が `release-binaries.yml` を起動できる（GITHUB_TOKEN ではループ防止で起動しない）
- `release-plz-pr` 側も同 PAT → release PR 上で CI が走る
- `persist-credentials: false`: checkout 後の git 操作で token を流用しない（公式推奨）
- Phase 2-A: `CARGO_REGISTRY_TOKEN` env を設定。Phase 2-C で削除して OIDC 化（README に手順記載）
- `release-plz-release` 側に `id-token: write` を入れておくことで OIDC 化時の workflow 変更は env 1 行削除のみ

### `.github/workflows/release-binaries.yml`

```yaml
name: Release Binaries

on:
  push:
    tags: ['v*']

permissions:
  contents: write     # GitHub Release への upload
  id-token: write     # npm OIDC

jobs:
  build:
    name: Build (${{ matrix.target }})
    strategy:
      fail-fast: true
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-22.04
            archive: tar.gz
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-22.04-arm        # GitHub 公式 ARM runner（public repo は無料）
            archive: tar.gz
          - target: x86_64-apple-darwin
            os: macos-13
            archive: tar.gz
          - target: aarch64-apple-darwin
            os: macos-14
            archive: tar.gz
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            archive: zip
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.85
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
      - name: Package (Unix)
        if: matrix.archive == 'tar.gz'
        shell: bash
        run: |
          cd target/${{ matrix.target }}/release
          tar czf ../../../precommit-check-${{ matrix.target }}.tar.gz precommit-check
      - name: Package (Windows)
        if: matrix.archive == 'zip'
        shell: pwsh
        run: |
          Compress-Archive -Path target\${{ matrix.target }}\release\precommit-check.exe `
            -DestinationPath precommit-check-${{ matrix.target }}.zip
      - uses: actions/upload-artifact@v4
        with:
          name: binary-${{ matrix.target }}
          path: precommit-check-${{ matrix.target }}.*
          retention-days: 1

  github-release:
    name: Attach binaries to GitHub Release
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          pattern: binary-*
          merge-multiple: true
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            precommit-check-*.tar.gz
            precommit-check-*.zip
          fail_on_unmatched_files: true
          # body / body_path は指定しない → release-plz が作った既存 release notes を保護

  npm-publish:
    name: Publish npm package
    needs: build       # github-release を待つ必要はない（artifact 経由で取れる）
    runs-on: ubuntu-latest
    permissions:
      contents: read
      id-token: write   # npm OIDC (Phase 2-C 以降に有効化)
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '22'           # npm OIDC 要件: Node 22.14+, npm 11.5.1+
          registry-url: 'https://registry.npmjs.org'
      - uses: actions/download-artifact@v4
        with:
          pattern: binary-*
          merge-multiple: true
          path: artifacts
      - name: Extract binaries into npm/binaries/
        shell: bash
        run: |
          mkdir -p npm/binaries
          # Unix targets
          for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
                        x86_64-apple-darwin aarch64-apple-darwin; do
            tar -xzf "artifacts/precommit-check-${target}.tar.gz" -C npm/binaries
            mv npm/binaries/precommit-check "npm/binaries/precommit-check-${target}"
            chmod +x "npm/binaries/precommit-check-${target}"
          done
          # Windows
          unzip "artifacts/precommit-check-x86_64-pc-windows-msvc.zip" -d npm/binaries
          mv npm/binaries/precommit-check.exe npm/binaries/precommit-check-x86_64-pc-windows-msvc.exe
      - name: Sync version from git tag → package.json
        shell: bash
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          cd npm
          npm version --no-git-tag-version "$VERSION"
      - name: Smoke test launcher (must succeed before publish)
        shell: bash
        run: |
          # 現環境 = ubuntu-latest (linux-x64) で linux-x64-gnu binary が動くこと
          node npm/lib/index.js --help
          # unsupported platform 分岐の動作確認（process.platform を強制差替え）
          # Node の direct 起動だと process.platform は readonly なので別 process で
          OUTPUT=$(PRECOMMIT_CHECK_FORCE_PLATFORM='aix-x64' node npm/lib/index.js --help 2>&1) || EXIT=$?
          test "${EXIT:-0}" -eq 1 || (echo "expected exit 1 for unsupported platform" && exit 1)
          echo "$OUTPUT" | grep -q 'unsupported platform' || (echo "expected error msg" && exit 1)
      - name: Publish to npm
        run: |
          npm publish --provenance --access public
        working-directory: npm
        env:
          # Phase 2-A (初回): NODE_AUTH_TOKEN 必要
          # Phase 2-C (OIDC 化後): 下行を削除
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}

  verify-publish:
    name: Verify published package (${{ matrix.os }})
    needs: npm-publish
    strategy:
      fail-fast: false   # 1 OS 失敗でも他の結果は見たい
      matrix:
        os: [ubuntu-latest, ubuntu-22.04-arm, macos-13, macos-14, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/setup-node@v4
        with:
          node-version: '22'
      - name: Install and run published package
        shell: bash
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          # 公開直後の registry 反映遅延に備え、3 回まで retry
          for i in 1 2 3; do
            if npx -y "precommit-check@${VERSION}" --help; then exit 0; fi
            sleep 10
          done
          exit 1
```

**設計判断**:
- 5 binary を artifact 経由で 1 つの publish job に集約（job 間 storage は GitHub Actions artifact が標準）
- **Node 22**（OIDC 要件、reviewer 指摘）。LTS でもある
- npm publish は `--provenance` で sigstore 署名（OIDC と一体）
- version: git tag (`v0.1.2`) → `0.1.2` を `npm version` で package.json に反映 → publish 後は package.json 変更を repo に push 戻さない（git tag が source of truth）
- npm/binaries/ は `.gitignore` 済みなので commit されない
- **Smoke test step**（reviewer 指摘）: launcher の `targets` map typo / spawn 失敗を publish 前に検知。unsupported platform 分岐は `PRECOMMIT_CHECK_FORCE_PLATFORM` env で test（launcher 側で `process.platform` を env で override 可能にする小細工）
- **`verify-publish` job**（reviewer 指摘）: 公開後に 5 OS で `npx precommit-check@<version> --help` を実行し、binary 同梱漏れ / shebang 破損 / OS 別の launcher 分岐ミスを即時検知
- `softprops/action-gh-release@v2`: `body` を渡さない → release-plz が作った notes を保護（reviewer 指摘の競合回避）

### `npm/package.json`

```json
{
  "name": "precommit-check",
  "version": "0.0.0",
  "description": "Pre-commit hook helper that records and verifies repository state via blake3 hash.",
  "homepage": "https://github.com/9wick/precommit-check",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/9wick/precommit-check.git"
  },
  "license": "MIT",
  "author": "KoheiKido <kido@9wick.com>",
  "type": "module",
  "bin": {
    "precommit-check": "lib/index.js"
  },
  "files": [
    "lib",
    "binaries"
  ],
  "engines": {
    "node": ">=18"
  },
  "os": [
    "linux",
    "darwin",
    "win32"
  ],
  "cpu": [
    "x64",
    "arm64"
  ]
}
```

- `version: "0.0.0"` は placeholder（CI で `npm version` が上書き）
- `os` / `cpu`: 5 binary が cover する範囲のみ宣言（unsupported な install を npm 側で弾く）
- `type: module` で ESM 採用（CLAUDE.md 方針）
- `files` で publish 対象を絞る（`node_modules/`、ソース、CI ファイル等を含めない）

### `npm/lib/index.js`（launcher）

```javascript
#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));

const targets = {
  'linux-x64': 'precommit-check-x86_64-unknown-linux-gnu',
  'linux-arm64': 'precommit-check-aarch64-unknown-linux-gnu',
  'darwin-x64': 'precommit-check-x86_64-apple-darwin',
  'darwin-arm64': 'precommit-check-aarch64-apple-darwin',
  'win32-x64': 'precommit-check-x86_64-pc-windows-msvc.exe',
};

// CI smoke test 用の env override (本番経路では未使用)
const forcedPlatform = process.env.PRECOMMIT_CHECK_FORCE_PLATFORM;
const key = forcedPlatform ?? `${process.platform}-${process.arch}`;
const binaryName = targets[key];

if (!binaryName) {
  process.stderr.write(`precommit-check: unsupported platform ${key}\n`);
  process.exit(1);
}

const result = spawnSync(
  join(__dirname, '..', 'binaries', binaryName),
  process.argv.slice(2),
  { stdio: 'inherit' },
);

if (result.error) {
  process.stderr.write(`precommit-check: failed to spawn binary: ${result.error.message}\n`);
  process.exit(1);
}

process.exit(result.status ?? 1);
```

- ~35 行の素 ESM、依存ゼロ（Node 標準のみ）
- `spawnSync` で同期実行 → exit code 透過
- shebang `#!/usr/bin/env node` で `bin` field 経由起動
- **`PRECOMMIT_CHECK_FORCE_PLATFORM`** env: CI の smoke test で unsupported platform 分岐を検証する用途のみ。production 利用者向けには undocumented（README に書かない）

### `npm/.gitignore`

```
binaries/
*.tgz
node_modules/
```

### `npm/README.md`

```markdown
# precommit-check

See https://github.com/9wick/precommit-check for full documentation.

## Install

\`\`\`bash
npm install --save-dev precommit-check
# or
npx precommit-check --help
\`\`\`

Supports Linux (x64, arm64), macOS (x64, arm64), Windows (x64).
```

ルート README.md と内容重複させず、npm package ページから upstream に誘導する短い文書に留める。

## ファイル変更（既存ファイル）

### `.gitignore`（root）

`npm/binaries/` を CI 生成として無視:

```diff
+ npm/binaries/
+ npm/*.tgz
```

### `README.md` / `README-ja.md`

Phase 1 で「npm wrapper planned in follow-up release」とした箇所を、実 install 手順に差し替え:

```markdown
## Installation

### npm (recommended for TypeScript / Node projects)

\`\`\`bash
npm install --save-dev precommit-check
\`\`\`

### Cargo (Rust users)

\`\`\`bash
cargo install precommit-check
\`\`\`

### Pre-built binaries

Download from [Releases](https://github.com/9wick/precommit-check/releases).
```

### `Cargo.toml`

変更不要。Phase 1 で metadata は publish 可能な状態。

## 実装タスク順序

実装は **2 段階**:
- **Stage 1（このセッション）**: workflow / config / launcher の作成。CI 緑化までは Phase 2-A の token 経由前提
- **Stage 2（v0.1.0 publish 後、別セッションで OK）**: trusted publisher 設定 → workflow から token 系 env を削除

### Stage 1: 実装

1. **Pre-flight 確認**:
   - `cargo publish --dry-run` を一度ローカルで実行 → keywords/categories 等の crates.io 規約 OK 確認
   - `Cargo.lock` が commit 済みであることの確認（既に commit 済み — Phase 1 の最終 commit `3264b0c` に含まれる）
2. `release-plz.toml` 作成（公式 single-tag pattern）
3. `.github/workflows/ci.yml` 作成（`publish-dryrun` / `release-plz-dryrun` job 含む）
4. `.gitignore` に `npm/binaries/`, `npm/*.tgz` 追加
5. `npm/package.json`, `npm/lib/index.js`, `npm/README.md`, `npm/.gitignore` 作成
6. **ローカルで launcher 動作確認**:
   - `cargo build --release` → `target/release/precommit-check` 生成
   - `mkdir -p npm/binaries && cp target/release/precommit-check npm/binaries/precommit-check-x86_64-unknown-linux-gnu && chmod +x npm/binaries/precommit-check-x86_64-unknown-linux-gnu`
   - `node npm/lib/index.js --help` が動くこと
   - `PRECOMMIT_CHECK_FORCE_PLATFORM=aix-x64 node npm/lib/index.js --help` が exit 1 + 'unsupported platform' を出すこと
   - 確認後 `rm -rf npm/binaries` で掃除（commit しない）
7. `.github/workflows/release-plz.yml` 作成（Phase 2-A: token + OIDC 両対応の env 構成）
8. `.github/workflows/release-binaries.yml` 作成（Phase 2-A: NPM_TOKEN env、smoke test, verify-publish 含む）
9. `README.md` / `README-ja.md` の install セクション更新（npm / cargo / pre-built binary の 3 経路、glibc 2.35+ 注記）
10. **`actionlint` で workflow YAML lint**（インストールされていれば）
11. ユーザーに Stage 1 完了報告 + Stage 2 手順案内

### Stage 2: OIDC 化（Phase 2-A → 2-B → 2-C）

1. ユーザー: token を Secrets に登録（`CARGO_REGISTRY_TOKEN`, `NPM_TOKEN`, `RELEASE_PLZ_TOKEN`）
2. ユーザー: main にマージ → `release-plz-pr` job が release PR 作成 → merge → `release-plz-release` job が v0.1.0 を crates.io に publish + tag push
3. tag push が `release-binaries.yml` を triggers → 5 binary build → GitHub Release 添付 → npm publish (token 経由)
4. v0.1.0 が両 registry に出揃ったら、ユーザー: crates.io / npmjs.com で trusted publisher を設定
5. ユーザー: `release-plz.yml` から `CARGO_REGISTRY_TOKEN` env 削除、`release-binaries.yml` から `NODE_AUTH_TOKEN` env 削除（commit）
6. 次の release（v0.1.1 等）で OIDC 経路の正常動作を検証

**コミット方針**: CLAUDE.md「commit はユーザー指示まで NEVER」に従う。Phase 2 全体を 1 commit にするか分けるかはユーザー指示時に確認。

## 検証方法

### ローカル検証（CI を回さずに確認できる範囲）

```bash
# 1. release-plz の dry-run（実 publish せず changelog/version 計算だけ）
cargo install release-plz
release-plz update --dry-run

# 2. launcher 単体動作
cargo build --release
mkdir -p npm/binaries
cp target/release/precommit-check npm/binaries/precommit-check-x86_64-unknown-linux-gnu
node npm/lib/index.js --help
node npm/lib/index.js compute   # tempdir 不要、リポジトリ自身で OK

# 3. workflow YAML の syntax check
# GitHub Actions の lint は actionlint
go install github.com/rhysd/actionlint/cmd/actionlint@latest
actionlint .github/workflows/*.yml
```

### CI 上の検証（PR / push 後）

- `ci.yml`: 3 OS test + fmt/clippy + cargo publish dry-run + release-plz dry-run 全 green
- `release-plz.yml` (main push 時): release PR が作られる、merge 後に crates.io publish + tag push
- `release-binaries.yml` (tag push 時) のフロー全体:
  1. 5 binary build (matrix: linux x64/arm64, macos x64/arm64, windows x64) 全 green
  2. `github-release`: GitHub Release v0.X.Y に 5 archive (4× tar.gz + 1× zip) が attached
  3. `npm-publish`: smoke test 通過 → `npm publish --provenance` 成功
  4. `verify-publish`: 5 OS で `npx precommit-check@0.X.Y --help` 全 green

### post-publish 自動 sanity（verify-publish job が担当）

5 OS (linux x64/arm64, macos 13/14, windows) で公開直後に `npx precommit-check@<tag> --help` を実行。1 OS でも失敗すれば Actions 上で赤くなり通知される。失敗時の対応は「リスクと既知の制約」の表参照。

## リスクと既知の制約

### 部分失敗時の取扱い（codex 指摘）

非原子的リリース構造を持つため、以下の障害シナリオを認識:

| 失敗シナリオ | 影響 | 復旧手順 |
|---|---|---|
| `release-plz-release` で crates.io publish 成功 → tag push 失敗 | crates.io にだけ version 存在 | tag を手動 push: `git tag v0.X.Y && git push origin v0.X.Y` → `release-binaries.yml` を起動 |
| crates.io publish + tag 成功 → 5 binary build 中に 1 OS だけ失敗 | GitHub Release に asset 不足、npm publish も走らない | Actions UI で当該 job だけ rerun。fail-fast: true なので他 OS 分も再 build |
| GitHub Release 添付成功 → npm publish 失敗 | crates.io / GitHub Release は完了、npm だけ古いまま | `release-binaries.yml` を tag 起点で rerun（npm-publish job のみで OK）。同一 version の `npm publish` は 24h 以内なら unpublish 可能 |
| npm publish 成功 → verify-publish で 1 OS 失敗 | 公開済み package が壊れている可能性 | `npm deprecate precommit-check@<version> "broken on <os>"` → patch bump で再リリース |

**設計判断**: 完全な原子性は GitHub Actions + 2 registry の構造的制約で得られない。代わりに (1) build を crates.io publish より先に走らせる構造（`release-binaries.yml` は tag 起点なので publish 後だが、検証は smoke test で publish 前に行う）、(2) verify-publish で公開直後に 5 OS sanity、(3) 上記 runbook の README 化、で運用上のリスクを下げる。

### glibc 互換性

`ubuntu-22.04` で build → glibc 2.35 以降の Linux でのみ動作:
- ✅ Ubuntu 22.04+, Debian 12+, RHEL 9+, Fedora 36+, Amazon Linux 2023
- ❌ Debian 11 (glibc 2.31), CentOS 7/8, Alpine (musl)

README に明記。Alpine / 古い distro 対応が必要になったら Phase 2.x で `cargo-zigbuild` 化。

### MSRV ポリシー

- crate MSRV: 1.85（`rust-toolchain.toml` + `Cargo.toml` `rust-version`）
- 依存 crate が MSRV を上げたら同調 bump
- 定常的な stable 追従はしない（hooks 用 CLI で安定性優先、cutting edge 機能不要）
- README に明記

## Out of Scope

### Phase 3: eslint-strict-type-rules 側整理（別セッション、**npm publish 完了後**）

Phase 1 計画通り。`bin/prepush-hash.mjs` 削除 / `package.json` 更新 / README 誘導など。

### Phase 2.x で後回しにする選択肢

- **musl Linux 対応**: gnu のみで開始。Alpine ユーザーから要望が来たら追加（cargo-zigbuild で対応容易）
- **glibc 下限拡張**: `ubuntu-22.04` の glibc 2.35 → `cargo-zigbuild --target x86_64-unknown-linux-gnu.2.17` で 2.17 まで下げ可能。需要次第で
- **cargo-binstall サポート**: tarball 形式は binstall 互換だが、`Cargo.toml` への `[package.metadata.binstall]` 明示は YAGNI。binstall 利用者が増えたら追加
- **GPG / SHA256 署名**: npm の `--provenance` (sigstore) で OIDC ベース署名は得られる。crates.io / GitHub Release 用の追加署名は YAGNI
- **Windows arm64 / Linux ARMv7 等の追加 target**: 5 主要 platform のみ。需要が出てから
- **stable Rust matrix での CI**: MSRV 1.85 固定で運用、stable 追従は依存更新時のみ
- **`cargo-deny` (license/advisory check)**: YAGNI（依存 4 つのみ、いずれも MIT/Apache-2.0）

## Files to Create

```
release-plz.toml
.github/workflows/ci.yml
.github/workflows/release-plz.yml
.github/workflows/release-binaries.yml
npm/package.json
npm/lib/index.js
npm/README.md
npm/.gitignore
```

## Files to Modify

```
.gitignore           # npm/binaries/, npm/*.tgz 追加
README.md            # install セクション差し替え
README-ja.md         # 同上
```

`Cargo.toml` は変更不要（Phase 1 で publish-ready）。
