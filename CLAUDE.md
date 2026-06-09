# CLAUDE.md

Guidance for Claude Code working in this repository.

## Project overview

izumi is a Fabric mod (concept) for **sharing one inventory across multiple
Minecraft servers**. What works today is the foundation: a toolchain that
**assembles `.class` files and the mod jar using only Rust, with no Java/Gradle**,
plus demo payloads that exercise the Rust↔JVM path. The inventory-sharing layer
itself is not implemented yet (see the README roadmap).

The user is a Japanese speaker — reply to them in Japanese. Everything committed
to this repository is written in English (code comments, documentation, commit
messages); the only Japanese file is `README_jp.md`. Code identifiers and
commands stay in their original form.

## Common commands

```bash
# Build the mod jar (host platform only) → out/izumi.jar
cargo run -p builder

# Aggregate mode: skip the local build and ingest each platform's artifacts
NATIVE_LIB_DIRS=linux-x86_64=staging/linux-x86_64,windows-x86_64=staging/windows-x86_64 \
    cargo run -p builder

# Build the cdylib on its own (add --target to cross-compile)
cargo build -p native-payloads --release --examples

# The same verification suite as CI (run it before committing)
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

# Check the exported JNI symbols
objdump -T target/release/examples/libminecraft_server.so | grep Java_
```

`taplo` (Cargo.toml formatting) and `cargo-machete` (unused deps) also run in CI.
After editing a Cargo.toml run `taplo format`, and keep machete in mind whenever
you change dependencies.

## Architecture

The workspace has 4 crates. Runtime code (native-payloads) and build-time code
(builder/src/mixins) are in **1:1 correspondence per payload**.

| crate | role |
| --- | --- |
| `crates/inject-macro` | proc-macro: wraps an `#[inject]` function in a JNI shim and exports it |
| `crates/api` | JNI runtime helpers (`CallbackInfo`, `println`, `EnvGuard`) |
| `crates/native-payloads` | each `examples/<name>.rs` is one file = one cdylib (`[[example]]`) |
| `crates/builder` | host bin that generates the Mixin / NativeLoader / NativePayloads classes and the jar via crustf |

`builder` outputs (`out/izumi.jar`):
`fabric.mod.json`, `izumi.mixins.json`, `com/izumi/mixin/<Name>.class`,
`com/izumi/runtime/NativePayloads.class`, `com/izumi/runtime/NativeLoader.class`,
`native/<platform>/<libname>`.

The canonical procedure for adding a payload is **"Writing a payload (hook)" in
the README**. In short: add an `#[inject]` function to `examples/<name>.rs` → add
an `[[example]]` to `Cargo.toml` → add a `MixinClass` impl to
`builder/src/mixins/<name>.rs` → re-export it in `mixins/mod.rs` → add it to
`const MIXINS` in `main.rs`.

## Invariants that are easy to break (watch out when editing)

- **Keep the owner name in sync**: `inject-macro`'s `JNI_NATIVE_OWNER`
  (`"com_izumi_runtime_NativePayloads"`) and `builder`'s `NATIVE_PAYLOADS_OWNER`
  (`"com/izumi/runtime/NativePayloads"`) must always match. If you rename the
  package, fix both at once and, after rebuilding, confirm with `objdump` that
  the symbol matches the holder class's internal name.
- **Do not place native methods on a Mixin**. The Mixin processor merges them
  into the target class, and the JVM then looks up `Java_net_minecraft_..._<fn>`
  and fails with `UnsatisfiedLinkError`. Always collect them into the
  `NativePayloads` holder (`build_native_payloads_class`).
- **Do not raise the class file version needlessly**. Mixins are 52 (JAVA_8).
  `NativeLoader` uses the crustf default 49 (Java 5) and keeps `StackMapTable`
  unnecessary even in branching code (not calling `.version` in
  `build_native_loader_class` is intentional).
- `native_lib_name()` matches the `[[example]]` `name`. `native_name` matches the
  Rust `#[inject]` function name (with `_` → `_1` per the JNI convention).
- Argument types are expressed with the `JavaType` enum, which centralizes the
  descriptor / slot / load opcode. Do not hand-duplicate the handler and native
  descriptors.

## Build environment & conventions

- Rust 1.95+ / edition 2024 / stable (`rust-toolchain.toml`).
- `crustf` is a **git dependency** (`https://github.com/topi-banana/crustf`); the
  first build needs network access.
- The release profile is `opt-level = "s"`, `lto = true`, `codegen-units = 1`,
  `panic = "abort"`, `strip = "debuginfo"` (to keep the in-jar natives small).
- `out/`, `target/`, and `staging/` are already in `.gitignore`. Do not let them
  slip into a commit.

## CI (`.github/workflows/ci.yml`)

- After `fmt` / `clippy` / `test` / `taplo` / `machete`, `build-natives`
  (a 6-platform matrix) builds the cdylib natively → `package` aggregates them all
  into a cross-platform `out/izumi.jar`.
- `package` writes the artifact report to `$GITHUB_STEP_SUMMARY` (the Job Summary)
  and posts the **same `summary.md`** to the PR via
  `marocchino/sticky-pull-request-comment@v3` (only on `pull_request`, with
  `continue-on-error` to guard against fork PRs).
- After editing the workflow, it is safer to check the YAML validity and to try
  the `summarize build` step's shell against a real local jar before pushing.
- CI, dependabot, and PR comments only run **once pushed to GitHub**.

## Things not to do

- Do not push / create remotes / release without the user's explicit request.
- Do not change `git config`, and do not use `--force` / hard reset on your own.
