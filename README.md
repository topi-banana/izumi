# izumi

[![CI](https://github.com/topi-banana/izumi/actions/workflows/ci.yml/badge.svg)](https://github.com/topi-banana/izumi/actions/workflows/ci.yml)

[日本語版 README](./README_jp.md)

**izumi** is a Fabric Minecraft mod for **sharing one inventory across multiple
servers** — stash items on one server and pull them back on any other server
wired to the same pool.

The name is 泉 *izumi*, "a spring": a shared **storage pool** that every server
draws from and feeds back into. *(The name also tips its hat to a certain
tautological turn of phrase.)*

> **Status — early / concept.** What works today is the build foundation: the
> mod jar is assembled **entirely from Rust**, with no Java toolchain, no
> Gradle, and no Mixin Gradle plugin. The cross-server inventory layer itself is
> on the [roadmap](#roadmap); the repo currently ships small demo payloads that
> exercise the Rust↔JVM path end to end.

## Concept

One logical inventory — the *pool* — backs many Minecraft servers. The intended
shape:

- Inventory and container operations are intercepted server-side via Mixin
  hooks (`@Inject`), generated here as plain `.class` files.
- Each hook calls into a native Rust library over JNI. The Rust side owns the
  pool: it reads and writes item state and reconciles it across servers (over a
  network link or a shared backend).
- Because the JVM-facing glue is generated, adding a new hook point is a small
  Rust edit on each side — no Gradle project, no Java sources to maintain.

Today the demo payloads only print to the server log (see
[Current demo](#current-demo)); the persistence and sync layers are not built
yet. The rest of this document describes the toolchain that makes those payloads
possible.

## How the toolchain works

Runtime code and build code sit in **1:1 correspondence** — one payload is one
`.rs` file on each side.

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
   └─ exported JNI symbol                                            ├─ fabric.mod.json
      Java_com_izumi_runtime_NativePayloads_hello                   ├─ izumi.mixins.json
                                                                    ├─ com/izumi/mixin/MinecraftServerMixin.class
                                                                    ├─ com/izumi/runtime/NativePayloads.class
                                                                    ├─ com/izumi/runtime/NativeLoader.class
                                                                    └─ native/<platform>/<libname>
```

1. The `#[inject]` proc-macro wraps each annotated Rust function in a JNI shim
   exported as `Java_com_izumi_runtime_NativePayloads_<fn>`. Nothing else is
   embedded in the cdylib.
2. `builder` keeps a compile-time list — `const MIXINS: &[&dyn MixinClass]` — and
   for each entry emits:
   - `com/izumi/mixin/<MixinName>.class` — the Mixin, one `@Inject` handler per
     `MixinMethod`.
   - `com/izumi/runtime/NativePayloads.class` — a plain holder class declaring
     every `public static native <fn>(…)`. (Mixins can't host native methods:
     the Mixin processor would merge them into the target class and break JNI
     static binding, so they live in a separate holder.)
   - `com/izumi/runtime/NativeLoader.class` — an `ensure_<lib>()` method per
     cdylib plus a `resourcePath(...)` helper that resolves the in-jar path from
     `os.name` / `os.arch`.
3. At runtime each handler calls `NativeLoader.ensure_<lib>()` once, which
   extracts the right `.so` / `.dll` / `.dylib` from the jar to a temp file
   (`deleteOnExit`) and `System.load`s it. From there the JVM binds
   `NativePayloads.<fn>()` to the Rust JNI shim.

## Prerequisites

- Rust 1.95+ (edition 2024)
- For an end-to-end run: Minecraft 1.20+ with [Fabric Loader] 0.15+

[Fabric Loader]: https://fabricmc.net/

## Build

### Host platform only

```
cargo run -p builder
```

The builder runs `cargo build -p native-payloads --release --examples` itself,
then writes `out/izumi.jar`. Drop the jar into `<minecraft>/mods/` next to a
Fabric Loader install.

### Linux + Windows from one Linux/WSL2 host

The Windows MSVC toolchain isn't usable from WSL2, but the `gnu` target
cross-compiles via mingw-w64:

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

`NATIVE_LIB_DIRS` switches the builder into **aggregate mode**: it skips the
local cargo build and bundles every `.so` / `.dll` / `.dylib` found under each
`<platform>=<dir>` mapping.

### CI (all 6 platforms)

[`.github/workflows/ci.yml`](./.github/workflows/ci.yml) builds the cdylibs
natively across `{linux, windows, macos} × {x86_64, aarch64}` GitHub-hosted
runners, then aggregates all six into one `out/izumi.jar`. See
[Continuous integration](#continuous-integration).

## Project layout

```
crates/
├── inject-macro/     proc-macro: wraps an #[inject] fn in a JNI shim
├── api/              JNI runtime helpers (CallbackInfo, println, EnvGuard)
├── native-payloads/  one cdylib per examples/<name>.rs ([[example]])
└── builder/          one MixinClass impl per src/mixins/<name>.rs;
                      main.rs lists them in `const MIXINS` and emits the jar
```

## Writing a payload (hook)

A new hook is one `.rs` file on each side plus three small wiring edits.

**1. Runtime code** — `crates/native-payloads/examples/greet.rs`:

```rust
use api::{CallbackInfo, println};

#[inject_macro::inject]
fn greet(_ci: CallbackInfo) {
    println("hello from another payload").ok();
}
```

**2. Cargo entry** — `crates/native-payloads/Cargo.toml`:

```toml
[[example]]
name = "greet"
path = "examples/greet.rs"
crate-type = ["cdylib"]
```

**3. Build code** — `crates/builder/src/mixins/greet.rs`. The `code` closure for
a handler just loads the target-method arguments and calls the native method;
the easiest path is to reuse the `emit_call_native` helper pattern from
`minecraft_server.rs`:

```rust
use super::{JavaType, MixinAt, MixinClass, MixinMethod};

pub struct GreetMixin;

impl MixinClass for GreetMixin {
    fn target_class(&self) -> &'static str { "net/minecraft/server/MinecraftServer" }
    fn mixin_class_simple_name(&self) -> &'static str { "GreetMixin" }
    fn native_lib_name(&self) -> &'static str { "greet" } // = the [[example]] name

    fn methods(&self) -> &'static [MixinMethod] {
        &[MixinMethod {
            name: "onRun",
            target_method: "runServer",
            target_args: &[],          // &[JavaType::Object("…"), …] for non-empty signatures
            at: MixinAt::Head,
            cancellable: false,
            exceptions: &["java/io/IOException"],
            native_name: "greet",      // = the Rust #[inject] fn name
            code: |mm, owner, c| emit_call_native(owner, c, mm.native_name, mm.target_args),
        }]
    }
}
```

**4. Re-export** — `crates/builder/src/mixins/mod.rs`:

```rust
pub mod greet;
pub use greet::GreetMixin;
```

**5. List in main** — `crates/builder/src/main.rs`:

```rust
const MIXINS: &[&dyn MixinClass] = &[&MinecraftServerMixin, &GreetMixin];
```

Multiple `@Inject` handlers on one Mixin are just more entries in `methods()` —
`MinecraftServerMixin` ships four as a demo.

## The `#[inject]` contract

`#[inject_macro::inject]` takes no arguments. The annotated function:

- may take zero or more arguments (forwarded from the JNI call), with an
  **optional trailing `api::CallbackInfo`**; it must not take `self`, and
  returns `()`.
- has its parameter list mirror the Mixin handler descriptor: the target
  method's `target_args` followed by `CallbackInfo`. Rust types use their JNI
  representation (primitives as `jint`/`jlong`/…, objects as `jni::objects::JObject`).

The macro exports `Java_com_izumi_runtime_NativePayloads_<jni-escaped-name>` and
installs an `EnvGuard` before calling the body, so `api::println`,
`CallbackInfo::cancel`, and friends can reach the current `JNIEnv` implicitly.
`native_name` on the builder side equals the Rust function name (JNI escapes
`_` to `_1`, etc.).

`target_args` are described with the `JavaType` enum
(`Int`, `Long`, `Object("java/util/function/BooleanSupplier")`, `Array("I")`, …),
which drives the descriptor, slot sizes, and load opcodes on both sides — one
source of truth, no duplication between cdylib and generated classes.

## Runtime model

- Each handler calls `NativeLoader.ensure_<lib>()` first. It resolves
  `/native/<os>-<arch>/<mapLibraryName(lib)>` inside the jar, copies it to a
  temp file (`deleteOnExit`), and `System.load`s it — guarded by a
  `synchronized` method and a `loaded_<lib>` flag so it runs once.
- `os.arch` is normalized (`amd64`→`x86_64`, `arm64`→`aarch64`) and `os.name`
  mapped to `windows` / `macos` / `linux`.
- Native methods live on `com/izumi/runtime/NativePayloads`, **not** on the
  Mixin, so the JVM binds them to `Java_com_izumi_runtime_NativePayloads_<fn>`
  as the JNI spec expects.

## Current demo

`crates/native-payloads/examples/minecraft_server.rs` exports four payloads,
wired to `net/minecraft/server/MinecraftServer` by `MinecraftServerMixin`:

| Payload       | Target / `@At`          | Behavior                                    |
| ------------- | ----------------------- | ------------------------------------------- |
| `hello`       | `runServer` HEAD        | prints on server start                      |
| `goodbye`     | `runServer` RETURN      | prints on server stop                       |
| `cancel_demo` | `runServer` HEAD (cancellable) | `CallbackInfo::cancel()` demo        |
| `on_tick`     | `tickServer` HEAD       | prints every 100th tick (`BooleanSupplier` arg) |

## Continuous integration

`ci.yml` runs `fmt`, `clippy -D warnings`, `test`, `taplo`, and `cargo-machete`,
then:

- **`build-natives`** — a 6-way matrix (`ubuntu-latest`, `ubuntu-22.04-arm`,
  `windows-latest`, `windows-11-arm`, `macos-15-intel`, `macos-latest`) builds a
  cdylib per platform and uploads it as an artifact. No cross-compilation.
- **`package`** — downloads all six artifacts and reruns `cargo run -p builder`
  with `NATIVE_LIB_DIRS` covering every platform, producing one cross-platform
  `out/izumi.jar`. It then writes a build report to the **job summary**
  (`$GITHUB_STEP_SUMMARY`) and posts the **same** report to the pull request via
  [`marocchino/sticky-pull-request-comment@v3`](https://github.com/marocchino/sticky-pull-request-comment).

[`.github/dependabot.yml`](./.github/dependabot.yml) keeps `cargo` and
`github-actions` dependencies up to date daily.

## Roadmap

- [ ] Shared **storage pool** persistence layer (Rust native)
- [ ] Cross-server synchronization (network link or shared backend)
- [ ] Mixin hooks for inventory / container operations
- [ ] Conflict resolution (no item duplication across servers)
- [ ] Player-facing surface (commands / status)

## License

MIT (see `fabric.mod.json`). A top-level `LICENSE` file is still to be added.

[`crustf`]: https://github.com/topi-banana/crustf
