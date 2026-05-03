# precommit-check

[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

[English README](./README.md)

AI / エージェントが commit を打つワークフローのプロジェクト向けの小さな Rust CLI。「チェックを走らせずに commit する」ことを **やりにくくし**、かつ **後から簡単に見抜けるようにする** ためのツール。

Git hook（[lefthook](https://github.com/evilmartians/lefthook) 等）に組み込むと、以下の 2 つを行う：

1. **commit をブロック**: 重いチェック（lint / build / test）が「コミット対象のツリーに対して」実際に走っていない限り、commit を通さない。
2. **コミットメッセージに刻印**: すべての commit メッセージに `Verified: precommit-check ✓ / △ / ✕` という footer を追記。仮に pre-commit hook を bypass されても（`git commit --no-verify`、トリックを知っているエージェント等）、prepare-commit-msg hook の方は走るので footer は書き込まれる — チェックを通していなければ `✕` で残る。bypass された commit は `git log` やコードレビューで一目でわかる。

## インストール

```sh
cargo install --locked precommit-check
```

> npm wrapper（`npx precommit-check` で動く配布形式）は次のリリースで対応予定。

## クイックスタート

`package.json` に「実チェック → 検証済み状態の記録」をまとめたスクリプトを追加：

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

1. 何か意味のある変更を加えた後に `pnpm precommit-check` を実行。重いチェックを走らせ、成功したら「このツリーは検証済み」という状態を記録。
2. `git commit` で pre-commit hook が起動。記録された状態とコミット対象のツリーが一致しなければ commit を **ブロック**（チェック自体は再実行しないので高速）。
3. prepare-commit-msg hook が `Verified: precommit-check ✓ / △ / ✕` を commit メッセージに追記し、commit 時点で「実際に何が検証されたか」を残す。

bypass について: `git commit --no-verify` は手順 2 を skip するが、prepare-commit-msg hook は **skip されない**。結果として footer は書き込まれ — チェックを通していなければ `✕` として残り — bypass された commit は `git log` で明白になる。

## サブコマンド

| コマンド | 動作 |
|---|---|
| `precommit-check save` | 現在のツリーが「検証済み」であることを記録する。**パッケージマネージャ経由のスクリプトから呼ばれる必要あり**（npm/pnpm/yarn/bun）。直接呼び出しは拒否、ワークフローの明示性を担保するため。 |
| `precommit-check check` | コミット対象のツリーが、最後に記録された検証済み状態と一致するか確認。不一致または記録なしなら exit 1。 |
| `precommit-check verify-footer <msg-file> [source]` | コミットメッセージファイルに `Verified: precommit-check ...` フッターを追記（既存があれば置換）。`source` が `merge` か `squash` のときは何もしない。 |
| `precommit-check compute` | 現在のツリーの内部 fingerprint を stdout に出力。デバッグ用 — 通常利用では不要。 |

### Footer の状態

| マーカー | 意味 |
|---|---|
| ✓ | このツリーに対してチェックが通った状態。それ以降に変更なし。 |
| △ | チェックは通ったツリーだが、その後 working tree が drift している（典型: 最後の `precommit-check` 実行後に編集して再検証していない状態）。 |
| ✕ | チェック実行の記録なし、またはチェック後にツリーが変わっている。**未検証として扱うこと。** |

## 何が「変更」とみなされるか

このツールは Git が working tree から見えるすべてのファイルを監視する：

- tracked ファイル全部
- untracked ファイル全部

以下は変更とみなされない：

- `.gitignore`（および `.git/info/exclude`、global gitignore、`core.excludesFile` 等）でマッチするファイル
- Markdown ファイル（`.md`） — ドキュメント編集は lint / build / test の結果に影響しないので、検証状態を無効化しない
- open に失敗するファイル（broken symlink、permission denied 等） — エラー扱いせず静かに無視

### 監視対象のカスタマイズ

現時点では非対応。上記のリストはハードコード。

- 追加で除外したいパターン（snapshot、generated code 等）
- `.md` を除外したくない

といった要望があれば issue を立ててもらえると嬉しい。設定ファイルを足すのは候補として考えているが、誰かが必要としていない時点で config 面を増やしても無駄なので未実装。

## FAQ

**`precommit-check save` が "save must be called via a package manager script" で失敗する**

`save` は直接実行を意図的に拒否する仕様。検証済み状態が実チェックと一緒にしか更新されないようにするため。`package.json` のスクリプト経由（あるいは利用しているパッケージマネージャの相当物）で呼ぶこと：

```jsonc
{ "scripts": { "precommit-check": "pnpm test && precommit-check save" } }
```

その上で `pnpm precommit-check` を実行する（`precommit-check save` を直叩きしない）。

**Git worktree でも動く？**

動く。検証済み状態は worktree ごとに独立して管理されるので、片方の worktree で `save` してももう片方の commit には影響しない。

**README を編集した。`save` を再実行しないとダメ？**

不要。Markdown（`.md`）は変更とみなされないので、編集しても lint / build / test の結果には影響せず、検証状態も無効化されない。

**`git commit --amend` するとどうなる？**

既存の `Verified: precommit-check ...` 行は重複追加ではなく **置換** される。なので amend 時に `save` を走らせなければ、現在の working tree の状態が footer に反映される。

**merge commit や squash commit ではどうなる？**

`verify-footer` は何もしない。これらのコミットメッセージは Git の merge / squash フローが自動生成するもので、利用者が編集したものではないため。

## License

MIT — [LICENSE](./LICENSE) を参照。
