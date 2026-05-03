# precommit-check

[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

[English README](./README.md)

Git リポジトリの「検証済みの状態」をハッシュファイルとして記録し、コミット時に再検証してコミットメッセージに `Verified:` フッターを追記する小さな Rust 製 CLI。[lefthook](https://github.com/evilmartians/lefthook) などの Git hooks runner と組み合わせて使う。

ねらい: 重いチェック（lint / build / test）は **一度だけ**走らせ、その時点のハッシュを保存しておく。以降のコミットでは、そのハッシュと現在の状態を照合するだけで「あれから何も変わっていない」ことを高速に確認できる（untracked ファイルも対象）。チェックとコミットの間で何かを変更すれば footer が ✕ に切り替わり、レビュアーがそれを認識できる。

## インストール

```sh
cargo install --locked precommit-check
```

> npm wrapper（`npx precommit-check` で動く配布形式）は次のリリースで対応予定。

## クイックスタート

`package.json` に「実チェック → save」をまとめたスクリプトを追加：

```jsonc
// package.json
{
  "scripts": {
    "precommit-check": "pnpm lint && pnpm build && pnpm test && precommit-check save"
  }
}
```

lefthook で Git hook に組み込む：

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

ワークフロー：

1. 何か意味のある変更を加えた後に `pnpm precommit-check` を実行。重いチェックを走らせ、最後にリポジトリのハッシュを `.git/precommit-check-hash` に保存。
2. `git commit` で pre-commit hook が起動し、`precommit-check check` が走る。これは高速 — 単に working tree を再ハッシュして比較するだけ。
3. prepare-commit-msg hook が `Verified: precommit-check ✓ (16桁hash)` のような footer を追記し、コミットメッセージにどの状態を検証したかが残る。

最後の `save` 以降に何かを編集していれば check が落ちて commit がブロックされる。check は通ったが unstaged 変更がある場合は footer が ` △` になる（staged + working state は検証済みだが、working tree が index から drift しているという soft warning）。

## サブコマンド

| コマンド | 動作 |
|---|---|
| `precommit-check save` | リポジトリのハッシュを計算し、`.git/precommit-check-hash` に書き込む。**パッケージマネージャ経由のスクリプトから呼ばれる必要あり**（npm/pnpm/yarn/bun）。直接呼び出しは拒否、ワークフローの明示性を担保するため。 |
| `precommit-check check` | ハッシュを再計算し、保存済みのものと比較。不一致または保存ファイルがなければ exit 1。 |
| `precommit-check compute` | 現在のハッシュを stdout に出力。デバッグやスクリプト用。 |
| `precommit-check verify-footer <msg-file> [source]` | コミットメッセージファイルに `Verified: precommit-check ...` フッターを追記（既存があれば置換）。`source` が `merge` か `squash` のときは何もしない。 |

### Footer の状態

| マーカー | 意味 |
|---|---|
| ✓ | ハッシュ一致、working tree もクリーン。 |
| △ | ハッシュ一致だが unstaged 編集あり（staged commit は検証済み、working tree が drift）。 |
| ✕ | ハッシュ未保存、または現状と不一致。 |

## ハッシュ計算の仕組み

- 入力: `git -c core.quotepath=false ls-files -z -c -o --exclude-standard`（tracked + untracked、NUL 区切りで改行や非 UTF-8 を含むパスにも対応）。
- `.md` で終わるファイルは除外（ドキュメントの churn で検証状態が無効化されないように）。
- ファイルをバイト順で sort し、各ファイルについて `<filename> \0 <file content> \0` を [blake3](https://github.com/BLAKE3-team/BLAKE3) に流し込んで単一ダイジェストを得る。NUL delimiter で `(filename, content)` の境界を明示し、バイト境界がずれただけで衝突するケースを防ぐ。
- open に失敗するファイル（broken symlink、permission denied 等）は静かに skip。
- ハッシュファイルのパスは `git rev-parse --git-path precommit-check-hash` で解決するので、[worktree](https://git-scm.com/docs/git-worktree) や submodule でも適切な場所に保存される。

## FAQ

**`save` を直接実行するとなぜ拒否されるのか？**
重いチェックを通さずにハッシュが更新されるのを防ぐため。「チェックを走らせるスクリプトと同じ場所で save を呼ぶ」という意図を、パッケージマネージャがセットする `npm_lifecycle_event` / `npm_execpath` 環境変数の存在で担保している。

**Git worktree でも動く？**
動く — ハッシュファイルは worktree ごとに保存される（`.git/worktrees/<name>/precommit-check-hash`）。各 worktree が独立した状態を持つ。

**なぜ blake3 で SHA-256 ではない？**
パフォーマンス。バイナリ全体が commit のたびに 30ms 以下で起動 → 完了する必要があり、blake3 の SIMD 実装は SHA-256 を余裕で上回る。hex digest は同じ 64 文字なので footer 表示は変わらない。

## prepush-hash からの移行

この CLI は元々 [`@9wick/eslint-plugin-strict-type-rules`](https://github.com/9wick/eslint-strict-type-rules) パッケージの中に `prepush-hash` として同居していたもの。独立リポジトリに切り出し、Rust で書き直し、実際のライフサイクル（pre-push ではなく pre-commit）に合わせて改名した。

差分：

- ハッシュファイル: `.git/prepush-hash` → `.git/precommit-check-hash`（旧ファイルは無視されるので削除して構わない）
- footer 文言: `Verified: prepush ...` → `Verified: precommit-check ...`（過去の commit に残った旧 footer はそのまま、新規 commit から新 prefix に切り替わる）
- ハッシュアルゴリズム: SHA-256 → blake3。旧ツールのハッシュとは互換性なし。アップグレード後に一度 `save` を走らせること。

## License

MIT — [LICENSE](./LICENSE) を参照。
