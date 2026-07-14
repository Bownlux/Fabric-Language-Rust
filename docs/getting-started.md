# Getting started

This walks a new mod called `my-mod` from `cargo new` to a hello-world log line on a
real Minecraft 26.2 dev server. Shell syntax is macOS/Linux; on Windows the steps are
the same, only paths and library file names differ (see the platform table below).

You'll need a Rust toolchain with `cargo` on PATH (edition 2024, so Rust 1.85+),
JDK 25+, and this repository cloned and building (`./gradlew build` green).

Throughout, the two directories are assumed to be siblings:

```
~/mods/
├── fabric-language-rust/    ← this repo
└── my-mod/                  ← your mod (created below)
```

## 1. Build fabric-language-rust from source

Nothing is published yet. The adapter jar and the `fabric-rust` SDK crate both come
from this repo.

```sh
cd ~/mods/fabric-language-rust
./gradlew build
```

You now have the adapter mod at `build/libs/fabric-language-rust-0.1.0.jar` and a
working reference mod at `example/build/libs/flr-example-0.1.0.jar`.

## 2. Create the crate

```sh
cd ~/mods
cargo new --lib my-mod
```

Replace `my-mod/Cargo.toml` with:

```toml
[package]
name = "my-mod"
version = "0.1.0"
edition = "2024"

[lib]
name = "my_mod"
crate-type = ["cdylib"]

[dependencies]
fabric-rust = { path = "../fabric-language-rust/rust/fabric-rust" }
```

`crate-type = ["cdylib"]` is required because the adapter `dlopen`s your library. The
`[lib] name` (`my_mod`) is the name you'll use everywhere: in the entrypoint `value`
and in the library file name inside the jar. Keep the default `panic = "unwind"`;
panics are caught at every FFI boundary and rethrown as Java exceptions, and
`panic = "abort"` would kill the JVM instead.

## 3. Write the entrypoint

`my-mod/src/lib.rs`:

```rust
use fabric_rust::prelude::*;

fn init() {
    info!("Hello from Rust! \u{1F980}");
}

fabric_rust::register_entrypoints! {
    "init" => init,
}
```

`register_entrypoints!` generates the one well-known `fabric_rust_register` export
that the adapter calls after loading your library, plus a panic-catching shim per
entry. Invoke it exactly once per cdylib, at module (item) position; duplicate or
empty names are compile errors. Entrypoint functions are plain `fn()` with no
arguments and no return value (the v0.1 adapter only implements zero-arg void
entrypoint interfaces such as `ModInitializer`).

The prelude gives you `error!`, `warn!`, `info!`, `debug!`, `trace!`. They log through
Minecraft's SLF4J with your crate name as the logger tag. Called before registration
(from a unit test, say) they fall back to stderr and never panic.

You can register several entrypoints from one library:

```rust
fabric_rust::register_entrypoints! {
    "init" => init,
    "client_init" => client_init,
}
```

## 4. Build the cdylib

```sh
cd ~/mods/my-mod
cargo build --release
```

The artifact lands in `target/release/`, named per platform:

| Your machine | Platform id | Artifact |
|---|---|---|
| Windows x64 | `windows-x64` | `target/release/my_mod.dll` |
| Linux x64 | `linux-x64` | `target/release/libmy_mod.so` |
| Linux arm64 | `linux-arm64` | `target/release/libmy_mod.so` |
| macOS Intel | `macos-x64` | `target/release/libmy_mod.dylib` |
| macOS Apple Silicon | `macos-arm64` | `target/release/libmy_mod.dylib` |

A local build targets your host only, which is fine for development. Shipping to
other platforms means cross-compiling for each target and packaging every resulting
library. This repo's release workflow
([.github/workflows/build.yml](../.github/workflows/build.yml)) does exactly that for
its own jars and is a decent template to crib from.

## 5. Write fabric.mod.json

`my-mod/fabric.mod.json`:

```json
{
  "schemaVersion": 1,
  "id": "my-mod",
  "version": "0.1.0",
  "name": "My Mod",
  "environment": "*",
  "entrypoints": {
    "main": [ { "adapter": "rust", "value": "my_mod::init" } ]
  },
  "depends": {
    "fabric-language-rust": ">=0.1.0",
    "fabricloader": ">=0.19.0"
  }
}
```

The `value` is `lib_name::fn_name`: the cargo `[lib] name`, then the name you
registered. A bare `my_mod` would be shorthand for `my_mod::init`. Anything else
(empty parts, three or more `::` segments) is rejected at load time.

## 6. Assemble the jar

A fabric-language-rust mod jar is `fabric.mod.json` plus your natives, no class files
needed. Layout inside the jar:

```
my-mod-0.1.0.jar
├── fabric.mod.json
└── natives/
    └── macos-arm64/              ← your host platform id
        └── libmy_mod.dylib       ← the artifact from step 4
```

Build it with the JDK's `jar` tool (macOS arm64 shown; substitute your platform id and
file name from the table above):

```sh
cd ~/mods/my-mod
mkdir -p staging/natives/macos-arm64
cp fabric.mod.json staging/
cp target/release/libmy_mod.dylib staging/natives/macos-arm64/
jar --create --file my-mod-0.1.0.jar -C staging .
```

The path inside the jar must be exactly `natives/<os>-<arch>/<mapped name>`; the
adapter looks nowhere else. For a multi-platform release you'd add one
`natives/<platform>/` directory per cross-compiled target to the same jar.

## 7. Test in a dev run

The easiest dev environment is this repo's own run configuration. It already has the
adapter on the mod path and copies the example mod into `run/mods/`:

```sh
cp ~/mods/my-mod/my-mod-0.1.0.jar ~/mods/fabric-language-rust/run/mods/
cd ~/mods/fabric-language-rust
./gradlew runServer      # or runClient
```

First `runServer` stops at the EULA; edit `run/eula.txt` to `eula=true` and rerun.
The run configs already pass `--enable-native-access=ALL-UNNAMED`, so there are no
JEP 472 warnings.

During mod initialization you should see both the example mod's and your mod's lines:

```
[main/INFO] (example_mod) Hello from Rust! 🦀
[main/INFO] (my_mod) Hello from Rust! 🦀
```

For a production-style test, put `fabric-language-rust-0.1.0.jar` and your jar into
the `mods/` folder of any Fabric Loader 0.19+ / Minecraft 26.1+ / Java 25
installation.

What happens under the hood on first load: the adapter extracts your library from the
jar to `<gameDir>/.fabric-language-rust/my-mod/<sha256[0..8]>/`, `dlopen`s it, calls
`fabric_rust_register` (which registers your entrypoint names and reports its ABI
version for the adapter to verify), and hands Fabric a `ModInitializer` proxy whose
`onInitialize()` calls your `init`. The extraction directory is content-addressed, so
stale copies are never reused after you rebuild.

## Troubleshooting

All failures surface as `LanguageAdapterException` during mod load, with messages that
say what to fix.

`does not bundle native library 'my_mod' for platform 'macos-arm64'` means the jar
path is wrong (must be exactly `natives/<platform id>/<mapped file name>`), or the jar
was built on/for a different platform.

`the loaded library is not a fabric-rust library (missing fabric_rust_register)` means
the cdylib was built without `register_entrypoints!`, or you packaged the wrong file.

`reported ABI version N, but this build of fabric-language-rust requires ABI version 1`
means the SDK and adapter are out of sync; rebuild your crate against this repo's
`fabric-rust`.

`has no entrypoint named 'foo'; registered entrypoints: [init]` means the `value` in
`fabric.mod.json` names an entrypoint you didn't register; the message lists what the
library actually registered.

A Rust panic inside your `init` doesn't crash the game. It's caught, rethrown as a
`RuntimeException` naming the entrypoint, and reported by Fabric like any other failed
mod initializer.

## Next steps

If you need to touch Java objects directly, the SDK re-exports the `jni` crate (0.22)
as `fabric_rust::jni`. Be careful with threads: code on Rust-spawned threads must not
call JNI `FindClass` (wrong classloader). The SDK's log macros are safe from any
JVM-attached thread and fall back to stderr from unattached ones.

Read [DESIGN.md](../DESIGN.md) for the full FFI contract, and
[`rust/example-mod`](../rust/example-mod) plus [`example/`](../example) for the
maintained reference mod and its jar packaging.
