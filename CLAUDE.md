# CLAUDE.md

このリポジトリで作業する Claude Code 向けのガイダンス。

## プロジェクト概要

izumi は **複数の Minecraft サーバー間でインベントリを共有する** Fabric mod
（コンセプト）。現状で動くのは基盤部分 —— **Java/Gradle を使わず Rust だけで
`.class` と mod jar を組み立てる** ツールチェーンと、Rust↔JVM 経路を検証する
デモペイロード。インベントリ共有レイヤ自体は未実装（README のロードマップ参照）。

ユーザーは日本語話者。応答・コミットメッセージ・コメントは日本語で（コード識別子
やコマンドは原語のまま）。

## よく使うコマンド

```bash
# mod jar をビルド（ホスト platform のみ）→ out/izumi.jar
cargo run -p builder

# 集約モード: ローカルビルドをスキップし各 platform の成果物を取り込む
NATIVE_LIB_DIRS=linux-x86_64=staging/linux-x86_64,windows-x86_64=staging/windows-x86_64 \
    cargo run -p builder

# cdylib 単体ビルド（クロスは --target を付ける）
cargo build -p native-payloads --release --examples

# CI と同じ検証一式（コミット前に通すこと）
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

# JNI エクスポートシンボルの確認
objdump -T target/release/examples/libminecraft_server.so | grep Java_
```

`taplo`（Cargo.toml の整形）と `cargo-machete`（未使用依存）も CI で回る。
Cargo.toml を編集したら `taplo format` を、依存を変えたら machete を意識すること。

## アーキテクチャ

ワークスペースは 4 crate。実行時コード（native-payloads）とビルド時コード
（builder/src/mixins）は **ペイロードごとに 1:1 対応**。

| crate | 役割 |
| --- | --- |
| `crates/inject-macro` | proc-macro。`#[inject]` 関数を JNI シムでラップして export |
| `crates/api` | JNI ランタイムヘルパ（`CallbackInfo`, `println`, `EnvGuard`） |
| `crates/native-payloads` | `examples/<name>.rs` が 1 ファイル = 1 cdylib（`[[example]]`） |
| `crates/builder` | crustf で Mixin / NativeLoader / NativePayloads class と jar を生成する host bin |

`builder` の生成物（`out/izumi.jar`）:
`fabric.mod.json`、`izumi.mixins.json`、`com/izumi/mixin/<Name>.class`、
`com/izumi/runtime/NativePayloads.class`、`com/izumi/runtime/NativeLoader.class`、
`native/<platform>/<libname>`。

ペイロード追加の手順は **README の「ペイロード（フック）の追加」** が正本。
要点: `examples/<name>.rs` に `#[inject]` 関数 → `Cargo.toml` に `[[example]]`
→ `builder/src/mixins/<name>.rs` に `MixinClass` impl → `mixins/mod.rs` で
re-export → `main.rs` の `const MIXINS` に追加。

## 壊しやすい不変条件（編集時に注意）

- **owner 名の同期**: `inject-macro` の `JNI_NATIVE_OWNER`
  (`"com_izumi_runtime_NativePayloads"`) と `builder` の `NATIVE_PAYLOADS_OWNER`
  (`"com/izumi/runtime/NativePayloads"`) は必ず一致させる。パッケージをリネーム
  するなら両方を同時に直し、再ビルド後に `objdump` でシンボルが holder クラスの
  内部名と一致することを確認する。
- **native メソッドは Mixin に置かない**。Mixin プロセッサがターゲットクラスへ
  マージし、JVM が `Java_net_minecraft_..._<fn>` を探して `UnsatisfiedLinkError`
  になる。必ず `NativePayloads` holder に集約する（`build_native_payloads_class`）。
- **class file version をむやみに上げない**。Mixin は 52 (JAVA_8)。`NativeLoader`
  は crustf default の 49 (Java 5) を使い、分岐コードでも `StackMapTable` を不要に
  している（`build_native_loader_class` で `.version` を呼んでいないのは意図的）。
- `native_lib_name()` は `[[example]]` の `name` と一致。`native_name` は Rust の
  `#[inject]` 関数名と一致（JNI 規約で `_` → `_1`）。
- 引数型は `JavaType` enum で表現し、descriptor / slot / load opcode を一元化する。
  ハンドラと native の descriptor を手で二重定義しない。

## ビルド環境・規約

- Rust 1.95+ / edition 2024 / stable（`rust-toolchain.toml`）。
- `crustf` は **git 依存**（`https://github.com/topi-banana/crustf`）。初回ビルド
  はネットワークが必要。
- release profile は `opt-level = "s"`, `lto = true`, `codegen-units = 1`,
  `panic = "abort"`, `strip = "debuginfo"`（jar 内ネイティブを小さく保つため）。
- `out/`, `target/`, `staging/` は `.gitignore` 済み。コミットに混入させない。

## CI（`.github/workflows/ci.yml`）

- `fmt` / `clippy` / `test` / `taplo` / `machete` のあと、`build-natives`
  （6 platform matrix）で cdylib をネイティブビルド → `package` が全部集約して
  cross-platform な `out/izumi.jar` を生成。
- `package` は成果物レポートを `$GITHUB_STEP_SUMMARY`（Job Summary）に出力し、
  **同一の `summary.md`** を `marocchino/sticky-pull-request-comment@v3` で PR に
  投稿する（`pull_request` 時のみ、fork PR 対策で `continue-on-error`）。
- workflow を編集したら YAML の妥当性と、`summarize build` ステップのシェルを
  ローカルの実 jar で試してから push すると安全。
- CI・dependabot・PR コメントは **GitHub に push して初めて動作** する。

## やらないこと

- ユーザーの明示依頼なしに push / リモート作成 / リリースを行わない。
- `git config` を変更しない、`--force` / hard reset を勝手に使わない。
