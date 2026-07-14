# fabric-language-rust

<img src="src/main/resources/assets/fabric-language-rust/icon.png" alt="" width="120" align="right">

A [Fabric](https://fabricmc.net/) language module, like
[fabric-language-kotlin](https://github.com/FabricMC/fabric-language-kotlin) but for
Rust: mods can write their entrypoints in Rust.

Kotlin compiles to JVM bytecode, so FLK only has to reflect on classes. Rust doesn't,
so this bridges through JNI instead. A consumer mod ships Rust cdylibs inside its jar
under `natives/<os>-<arch>/`, the adapter extracts and loads the right one for the
running platform, and Fabric entrypoint interfaces (`ModInitializer` and friends) are
implemented with `java.lang.reflect.Proxy` objects that call back into the registered
Rust functions.

Targets Minecraft 26.1+, Fabric Loader 0.19+, Java 25. [DESIGN.md](DESIGN.md) has the
full architecture and FFI contract.

This is v0.1 and nothing is published yet, not on crates.io, Modrinth, or any maven.
You build everything from this repo. The FFI contract (ABI version 1) is frozen; crate
names and the APIs above it might still change. Fair warning: this won't get major
updates that often since I'm busy working on
[RevivalSMP.net](https://RevivalSMP.net) and Retromod.

## Quickstart for mod authors

### 1. Build the adapter from source

```sh
git clone https://github.com/bownlux/fabric-language-rust
cd fabric-language-rust
./gradlew build
```

You get `build/libs/fabric-language-rust-0.1.0.jar` (the adapter mod, with the
host-platform native trampoline inside) and `example/build/libs/flr-example-0.1.0.jar`
(a complete Rust mod you can crib from). You need a Rust toolchain (`cargo` on PATH,
edition 2024 so Rust 1.85+) and JDK 25+.

### 2. Write the Rust side

`Cargo.toml` is a cdylib with a path dependency on the SDK crate in this repo:

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

Don't set `panic = "abort"`. Panics get caught at every FFI boundary, which needs the
default `panic = "unwind"`.

`src/lib.rs` (this is the actual [`rust/example-mod`](rust/example-mod/src/lib.rs)
crate, renamed):

```rust
use fabric_rust::prelude::*;

fn init() {
    info!("Hello from Rust! \u{1F980}");
}

fabric_rust::register_entrypoints! {
    "init" => init,
}
```

`register_entrypoints!` generates the single `fabric_rust_register` export the adapter
looks for, plus a panic-catching shim per entry. It must appear exactly once per
cdylib, and duplicate or empty names are compile errors. The `error!`/`warn!`/`info!`/
`debug!`/`trace!` macros log through Minecraft's SLF4J (logger name = your crate
name); before registration they fall back to stderr instead of panicking. Raw JNI is
available as `fabric_rust::jni` (the `jni` 0.22 crate, re-exported).

### 3. Declare the entrypoint

In your mod's `fabric.mod.json`:

```json
{
  "schemaVersion": 1,
  "id": "my-mod",
  "version": "0.1.0",
  "entrypoints": {
    "main": [ { "adapter": "rust", "value": "my_mod::init" } ]
  },
  "depends": {
    "fabric-language-rust": ">=0.1.0",
    "fabricloader": ">=0.19.0"
  }
}
```

The `value` grammar is the same shape as FLK's `Class::member`:

| `value` | Meaning |
|---|---|
| `lib_name::fn_name` | library `lib_name` (the cargo `[lib] name`, underscores as cargo writes them), entrypoint registered under `fn_name` |
| `lib_name` | shorthand for `lib_name::init` |
| anything else (3+ `::` parts, empty parts) | `LanguageAdapterException` |

The entrypoint type must be an interface with exactly one zero-argument `void` method.
`ModInitializer`, `ClientModInitializer`, and `DedicatedServerModInitializer` all
qualify.

[docs/getting-started.md](docs/getting-started.md) walks through the whole thing,
including jar assembly and testing in a dev run.

## How natives are packaged

Consumer jars carry one cdylib per supported platform at
`natives/<os>-<arch>/<mapped file name>`, where the platform id comes from
`os.name`/`os.arch` and the file name follows `System.mapLibraryName`:

| Platform id | Rust target triple | File in jar (lib name `my_mod`) |
|---|---|---|
| `windows-x64` | `x86_64-pc-windows-msvc` | `natives/windows-x64/my_mod.dll` |
| `linux-x64` | `x86_64-unknown-linux-gnu` | `natives/linux-x64/libmy_mod.so` |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | `natives/linux-arm64/libmy_mod.so` |
| `macos-x64` | `x86_64-apple-darwin` | `natives/macos-x64/libmy_mod.dylib` |
| `macos-arm64` | `aarch64-apple-darwin` | `natives/macos-arm64/libmy_mod.dylib` |

Those five targets are the release matrix. Local builds bundle host-platform natives
only, since Gradle runs plain `cargo build --release` with no cross-compilation. So a
jar built on an Apple Silicon Mac contains only `natives/macos-arm64/` and refuses to
load anywhere else, with an error saying so. On tag pushes, the CI cross-build matrix
in [.github/workflows/build.yml](.github/workflows/build.yml) builds all five targets
and the release-package job merges them into the jars attached to the GitHub release
(`./gradlew build -PextraNativesDir=…`).

At runtime the adapter extracts the matching library to
`<gameDir>/.fabric-language-rust/<modid>/<sha256[0..8]>/` and `dlopen`s it from there.
The directory is content-addressed, so re-extraction across runs and versions is
idempotent.

## Java 25 native access

Java 25 emits [JEP 472](https://openjdk.org/jeps/472) warnings for JNI use unless the
JVM is started with:

```
--enable-native-access=ALL-UNNAMED
```

The dev run configurations and the test task in this repo already pass it; add it to
your server launch flags to silence the warning.

## Building and testing this repo

```sh
./gradlew build        # adapter jar + example jar (host natives inside)
./gradlew test         # full FFI harness, no Minecraft needed
./gradlew runServer    # boots a real MC 26.2 server with the example mod in run/mods
cd rust && cargo test  # Rust-side unit tests (macro expansion, log fallback, ABI)
```

The Gradle root project is the adapter mod itself (same setup as FLK); `:example` only
packages the example jar. `rust/` is a cargo workspace with four crates: `native/`
(the JNI trampoline, `[lib] name = "fabric_language_rust"`), `fabric-rust` (the
author-facing SDK), `fabric-rust-macros` (`register_entrypoints!`), and `example-mod`.
Gradle drives cargo via `cargoBuildNative` / `cargoBuildExample` `Exec` tasks and
wires the artifacts into the jars and the test JVM.

`./gradlew test` runs `NativeBridgeHarnessTest`, which exercises the entire FFI chain
without Minecraft: load the trampoline, `openLibrary(example_mod)`, `registerMod`,
assert the reported ABI version and registered names, `invokeEntrypoint("init")`,
assert the hello line arrived at a Java-side test sink. It also checks the error paths
(nonexistent library path, unknown entrypoint name, null function pointer) throw clean
Java exceptions instead of crashing the JVM.

## Known limits (v0.1)

- No Mixins from Rust. Native code can't participate in bytecode transformation; pair
  with a Java/Kotlin side, or wait for event-style APIs here.
- Entrypoint interfaces with arguments (datagen, for example) aren't adaptable yet.
- Rust-spawned threads must not use JNI `FindClass`, because they resolve against the
  system classloader rather than Fabric's Knot classloader. The SDK's cached refs are
  safe from any attached thread. A proper thread/attach helper is on the roadmap.
- A hard native crash (segfault) takes the JVM with it. Rust panics are caught at
  every boundary; undefined behavior is not.
- Local builds ship host-platform natives only; tagged releases bundle all five
  platforms via the CI cross-build matrix.

## Roadmap

A real upcall surface beyond logging (registries, events, commands, as static
`RustBridge`-style APIs versioned via the ABI number), published `fabric-rust` /
`fabric-rust-macros` crates on crates.io plus a maven artifact for the adapter,
entrypoints with arguments, and a thread/attach helper for safe JNI from Rust-spawned
threads.

## Non-affiliation

This is a community project by Bownlux, not affiliated with, endorsed by, or supported
by FabricMC or Mojang. MIT licensed, see [LICENSE](LICENSE), © Bownlux.
