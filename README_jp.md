# izumi

[![CI](https://github.com/topi-banana/izumi/actions/workflows/ci.yml/badge.svg)](https://github.com/topi-banana/izumi/actions/workflows/ci.yml)

[English README](./README.md)

**izumi** は、**複数の Minecraft サーバー間でインベントリを共有する** ための
Fabric mod です。あるサーバーで預けたアイテムを、同じプールに繋がった別の
サーバーから取り出せる、というコンセプトです。

名前の由来は「泉」(izumi) — どのサーバーもそこから汲み、そこへ流し込む共有の
**ストレージプール (storage pool)** を表します。*（同語反復で知られる、とある
名にもそっと掛けています。）*

> **ステータス — 初期 / コンセプト段階。** 現時点で動作するのは「基盤」です:
> mod jar を **Rust だけ** で組み立てます（Java ツールチェーン・Gradle・Mixin
> Gradle plugin は一切不要）。インベントリ共有レイヤそのものは
> [ロードマップ](#ロードマップ) 段階で、リポジトリには現状 Rust↔JVM の経路を
> 端から端まで検証する小さなデモペイロードが入っています。

## コンセプト

ひとつの論理的なインベントリ —「プール」— が複数の Minecraft サーバーを支え
ます。想定している形:

- インベントリ・コンテナ操作をサーバー側で Mixin フック (`@Inject`) で横取り
  します。これらは本プロジェクトが `.class` として生成します。
- 各フックは JNI 経由でネイティブの Rust ライブラリを呼びます。プールを保持
  するのは Rust 側で、アイテム状態を読み書きし、サーバー間（ネットワーク経由、
  または共有バックエンド）で整合させます。
- JVM 側の繋ぎ込みは生成物なので、フック点を増やすのは両側の小さな Rust 編集
  だけ。Gradle プロジェクトも、保守すべき Java ソースもありません。

現状のデモペイロードはサーバーログへの出力のみです（[現状のデモ](#現状のデモ)
参照）。永続化・同期レイヤはまだ実装していません。以降は、それらのペイロードを
成立させる「ツールチェーン」の説明です。

## 仕組み

実行時コードとビルド時コードは **1:1 で対応** します。1 ペイロード = 両側で
1 ファイルずつです。

```
native-payloads/examples/minecraft_server.rs          builder/src/mixins/minecraft_server.rs
 ┌─────────────────────────────────────────┐          ┌────────────────────────────────────────────┐
 │ #[inject_macro::inject]                  │          │ impl MixinClass for MinecraftServerMixin {   │
 │ fn hello(_ci: CallbackInfo) {            │   1:1    │   target_class() / native_lib_name() /       │
 │     println("Hello from native!").ok();  │ <──────> │   methods() -> &[MixinMethod { … }]          │
 │ }                                        │          │ }                                            │
 └─────────────────────────────────────────┘          └────────────────────────────────────────────┘
        │ cargo build -p native-payloads                                │ cargo run -p builder
        ▼    --release --examples                                       ▼
 target/release/examples/libminecraft_server.{so,dll,dylib}        out/izumi.jar
   └─ JNI エクスポートシンボル                                       ├─ fabric.mod.json
      Java_com_izumi_runtime_NativePayloads_hello                   ├─ izumi.mixins.json
                                                                    ├─ com/izumi/mixin/MinecraftServerMixin.class
                                                                    ├─ com/izumi/runtime/NativePayloads.class
                                                                    ├─ com/izumi/runtime/NativeLoader.class
                                                                    └─ native/<platform>/<libname>
```

1. `#[inject]` proc-macro は対象関数を `Java_com_izumi_runtime_NativePayloads_<fn>`
   という JNI シムでラップするだけ。cdylib にそれ以外のメタ情報は埋めません。
2. `builder` はコンパイル時のリスト `const MIXINS: &[&dyn MixinClass]` を持ち、
   各エントリから以下を生成します:
   - `com/izumi/mixin/<MixinName>.class` — Mixin 本体。`MixinMethod` ごとに
     `@Inject` ハンドラ 1 つ。
   - `com/izumi/runtime/NativePayloads.class` — `public static native <fn>(…)`
     を集める普通の holder クラス。Mixin に native メソッドを置くと Mixin
     プロセッサがターゲットクラスにマージして JNI 静的バインドが壊れるので、
     別 holder に逃がしています。
   - `com/izumi/runtime/NativeLoader.class` — cdylib ごとの `ensure_<lib>()` と、
     `os.name` / `os.arch` から jar 内パスを解決する `resourcePath(...)` ヘルパ。
3. 実行時、各ハンドラはまず `NativeLoader.ensure_<lib>()` を呼びます。これが
   現在の OS/アーキに対応する `.so` / `.dll` / `.dylib` を jar から temp file に
   展開し（`deleteOnExit`）、`System.load`。以降 JVM が `NativePayloads.<fn>()`
   を Rust 側 JNI シムに静的バインドします。

## 必要な物

- Rust 1.95+（edition 2024）
- 実機検証には Minecraft 1.20+ + [Fabric Loader] 0.15+

[Fabric Loader]: https://fabricmc.net/

## ビルド

### ホスト platform 1 つだけ

```
cargo run -p builder
```

builder が内部で `cargo build -p native-payloads --release --examples` を呼び、
`out/izumi.jar` を生成します。Fabric Loader を入れた Minecraft の
`<minecraft>/mods/` に投入してください。

### Linux + Windows を 1 つの Linux / WSL2 ホストで

WSL2 から MSVC ターゲットは使えませんが、mingw-w64 経由の `gnu` ターゲットは
クロスコンパイルできます:

```
rustup target add x86_64-pc-windows-gnu
sudo apt install -y mingw-w64

cargo build -p native-payloads --release --examples
cargo build -p native-payloads --release --examples --target x86_64-pc-windows-gnu

mkdir -p staging/linux-x86_64 staging/windows-x86_64
cp target/release/examples/libminecraft_server.so                     staging/linux-x86_64/
cp target/x86_64-pc-windows-gnu/release/examples/minecraft_server.dll staging/windows-x86_64/

NATIVE_LIB_DIRS=linux-x86_64=staging/linux-x86_64,windows-x86_64=staging/windows-x86_64 \
    cargo run -p builder
```

`NATIVE_LIB_DIRS` を設定すると builder は **集約モード** に切り替わり、ローカル
の cargo build をスキップして各 `<platform>=<dir>` のディレクトリにある `.so` /
`.dll` / `.dylib` を全部取り込みます。

### CI（全 6 platform）

[`.github/workflows/ci.yml`](./.github/workflows/ci.yml) が
`{linux, windows, macos} × {x86_64, aarch64}` の GitHub-hosted ランナーで
cdylib をネイティブビルドし、6 つを集約して 1 つの `out/izumi.jar` にします。
詳細は [CI](#ci) を参照。

## ワークスペース構成

```
crates/
├── inject-macro/     proc-macro: #[inject] 関数を JNI シムでラップ
├── api/              JNI ランタイムヘルパ (CallbackInfo, println, EnvGuard)
├── native-payloads/  examples/<name>.rs が 1 ファイル = 1 cdylib ([[example]])
└── builder/          src/mixins/<name>.rs ごとに MixinClass impl 1 つ。
                      main.rs の `const MIXINS` に列挙し jar を生成する host bin
```

## ペイロード（フック）の追加

ペイロードを 1 つ増やすには、両側で `.rs` を 1 ファイルずつ + 3 箇所の配線です。

**1. 実行時コード** — `crates/native-payloads/examples/greet.rs`:

```rust
use api::{CallbackInfo, println};

#[inject_macro::inject]
fn greet(_ci: CallbackInfo) {
    println("hello from another payload").ok();
}
```

**2. Cargo エントリ** — `crates/native-payloads/Cargo.toml`:

```toml
[[example]]
name = "greet"
path = "examples/greet.rs"
crate-type = ["cdylib"]
```

**3. ビルド時コード** — `crates/builder/src/mixins/greet.rs`。ハンドラの `code`
クロージャは対象メソッド引数を load して native メソッドを呼ぶだけです。最も
簡単なのは `minecraft_server.rs` の `emit_call_native` ヘルパパターンの再利用:

```rust
use super::{JavaType, MixinAt, MixinClass, MixinMethod};

pub struct GreetMixin;

impl MixinClass for GreetMixin {
    fn target_class(&self) -> &'static str { "net/minecraft/server/MinecraftServer" }
    fn mixin_class_simple_name(&self) -> &'static str { "GreetMixin" }
    fn native_lib_name(&self) -> &'static str { "greet" } // = [[example]] の name

    fn methods(&self) -> &'static [MixinMethod] {
        &[MixinMethod {
            name: "onRun",
            target_method: "runServer",
            target_args: &[],          // 引数があれば &[JavaType::Object("…"), …]
            at: MixinAt::Head,
            cancellable: false,
            exceptions: &["java/io/IOException"],
            native_name: "greet",      // = Rust 側 #[inject] 関数名
            code: |mm, owner, c| emit_call_native(owner, c, mm.native_name, mm.target_args),
        }]
    }
}
```

**4. re-export** — `crates/builder/src/mixins/mod.rs`:

```rust
pub mod greet;
pub use greet::GreetMixin;
```

**5. main の MIXINS に追加** — `crates/builder/src/main.rs`:

```rust
const MIXINS: &[&dyn MixinClass] = &[&MinecraftServerMixin, &GreetMixin];
```

同じ Mixin に `@Inject` ハンドラを複数置きたいときは `methods()` にエントリを
足すだけです。`MinecraftServerMixin` はデモとして 4 つ載せています。

## `#[inject]` の仕様

`#[inject_macro::inject]` は引数なしの attribute マクロ。対象関数は:

- 0 個以上の引数（JNI 呼び出しから転送される）+ **末尾に任意で
  `api::CallbackInfo`** を取れます。`self` は不可、返り値は `()`。
- 引数列は Mixin ハンドラの descriptor と一致させます: 対象メソッドの
  `target_args` の後ろに `CallbackInfo`。Rust の型は JNI 表現を使います
  （プリミティブは `jint`/`jlong`/…、オブジェクトは `jni::objects::JObject`）。

マクロは `Java_com_izumi_runtime_NativePayloads_<jni エスケープ名>` を export し、
本体呼び出し前に `EnvGuard` を張ります。これにより `api::println` や
`CallbackInfo::cancel` などが暗黙の `JNIEnv` を取得できます。builder 側の
`native_name` は Rust 関数名と一致します（JNI 規約で `_` は `_1` 等にエスケープ）。

`target_args` は `JavaType` enum
（`Int`, `Long`, `Object("java/util/function/BooleanSupplier")`, `Array("I")` …）
で記述し、descriptor・slot サイズ・load opcode が両側で一意に決まります。cdylib
と生成クラスの間で情報が二重化することはありません。

## ランタイムの動作

- 各ハンドラはまず `NativeLoader.ensure_<lib>()` を呼びます。jar 内の
  `/native/<os>-<arch>/<mapLibraryName(lib)>` を解決し、temp file へコピー
  （`deleteOnExit`）して `System.load`。`synchronized` メソッドと `loaded_<lib>`
  フラグで一度きりに制御します。
- `os.arch` は正規化（`amd64`→`x86_64`、`arm64`→`aarch64`）、`os.name` は
  `windows` / `macos` / `linux` にマップします。
- native メソッドは Mixin ではなく `com/izumi/runtime/NativePayloads` に置く
  ため、JVM は JNI 規約どおり `Java_com_izumi_runtime_NativePayloads_<fn>` に
  バインドします。

## 現状のデモ

`crates/native-payloads/examples/minecraft_server.rs` は 4 つのペイロードを
export し、`MinecraftServerMixin` が `net/minecraft/server/MinecraftServer` に
配線します:

| ペイロード    | 対象 / `@At`            | 挙動                                          |
| ------------- | ----------------------- | --------------------------------------------- |
| `hello`       | `runServer` HEAD        | サーバー起動時に出力                          |
| `goodbye`     | `runServer` RETURN      | サーバー停止時に出力                          |
| `cancel_demo` | `runServer` HEAD（cancellable） | `CallbackInfo::cancel()` のデモ       |
| `on_tick`     | `tickServer` HEAD       | 100 tick ごとに出力（`BooleanSupplier` 引数） |

## CI

`ci.yml` は `fmt` / `clippy -D warnings` / `test` / `taplo` / `cargo-machete`
を回したうえで:

- **`build-natives`** — 6 種のランナー（`ubuntu-latest`, `ubuntu-22.04-arm`,
  `windows-latest`, `windows-11-arm`, `macos-15-intel`, `macos-latest`）で
  platform ごとに cdylib をビルドして artifact 化（クロスコンパイル不要）。
- **`package`** — 6 つの artifact を集約し、全 platform を含む `NATIVE_LIB_DIRS`
  付きで `cargo run -p builder` を再実行して cross-platform な `out/izumi.jar`
  を生成。続けてビルドレポートを **Job Summary**（`$GITHUB_STEP_SUMMARY`）に
  出力し、**同一内容** を
  [`marocchino/sticky-pull-request-comment@v3`](https://github.com/marocchino/sticky-pull-request-comment)
  で pull request にも投稿します。

[`.github/dependabot.yml`](./.github/dependabot.yml) が `cargo` と
`github-actions` の依存を daily で更新します。

## ロードマップ

- [ ] 共有 **ストレージプール** の永続化レイヤ（Rust native）
- [ ] サーバー間同期（ネットワーク or 共有バックエンド）
- [ ] インベントリ / コンテナ操作の Mixin フック
- [ ] 競合解決（サーバー間でのアイテム二重取得を防ぐ）
- [ ] プレイヤー向けの導線（コマンド / ステータス）

## ライセンス

MIT（`fabric.mod.json` 参照）。トップレベルの `LICENSE` ファイルは未追加です。

[`crustf`]: https://github.com/topi-banana/crustf
