# Fabric Language Rust — Architecture & FFI Contract

**Status:** v0.1 contract — FROZEN. Every component below is built against this document.
Deviations require updating this file in the same change.

## What this is

`fabric-language-rust` is a Fabric language module in the spirit of
[fabric-language-kotlin](https://github.com/FabricMC/fabric-language-kotlin): a mod that
registers a custom `LanguageAdapter` so *other* mods can write their entrypoints in Rust.

Kotlin compiles to JVM bytecode, so FLK only has to reflect on classes. Rust compiles to
native code, so this project bridges through JNI instead:

- A consumer mod ships one or more Rust **cdylibs** inside its jar under
  `natives/<os>-<arch>/`.
- Our adapter extracts the right library for the running platform, loads it, and asks it
  to register its entrypoint functions.
- Fabric entrypoint interfaces (`ModInitializer`, …) are implemented with
  `java.lang.reflect.Proxy` objects that call back into the registered Rust functions.

Target platform: **Minecraft 26.1+** (unobfuscated, official Mojang names — no mappings
anywhere), Fabric Loader **≥ 0.19**, Java **25**.

## Repository layout

```
Fabric-Language-Rust/
├── DESIGN.md                  ← this file
├── README.md
├── LICENSE                    (MIT, © Bownlux)
├── settings.gradle / build.gradle / gradle.properties / gradlew*
├── src/main/java/io/github/bownlux/fabricrust/
│   ├── RustLanguageAdapter.java      LanguageAdapter impl (public no-arg ctor!)
│   ├── NativeBridge.java             all Java-visible `native` methods + System.load
│   ├── EntrypointRegistrar.java      per-(mod,lib) registration callback object
│   ├── RustBridge.java               static upcall surface for Rust (logging)
│   └── NativeLibraryLocator.java     platform detection + jar extraction
├── src/main/resources/fabric.mod.json
├── src/test/java/io/github/bownlux/fabricrust/
│   └── NativeBridgeHarnessTest.java  JUnit harness: full FFI chain, no Minecraft
├── example/                          Gradle subproject packaging the example mod jar
│   ├── build.gradle
│   └── src/main/resources/fabric.mod.json
├── rust/                             Cargo workspace (edition 2024)
│   ├── Cargo.toml
│   ├── native/                       crate fabric-language-rust-native
│   │                                 → [lib] name = "fabric_language_rust", cdylib
│   ├── fabric-rust/                  SDK crate consumed by mod authors
│   ├── fabric-rust-macros/           proc-macro crate (register_entrypoints!)
│   └── example-mod/                  crate example-mod → [lib] name = "example_mod", cdylib
└── .github/workflows/build.yml       CI (host build + cross-compile matrix)
```

Gradle root project **is** the adapter mod (like FLK). `:example` only packages the
example jar (no Loom, no Java sources).

## Version pins (verified 2026-07-13)

| Thing | Version |
|---|---|
| Minecraft (dev runtime) | 26.2 |
| Fabric Loader | 0.19.3 |
| fabric-loom | 1.17.14 (needs Gradle ≥ 9.4; no mappings for MC 26.x) |
| Gradle wrapper | 9.6.1 |
| Java (compile release + runtime) | 25 |
| `jni` crate | 0.22.4 (0.22.0/0.22.1 are yanked) |
| `libloading` crate | 0.9.0 |
| `syn` / `quote` / `proc-macro2` | 2.x / 1.x / 1.x (latest) |

`gradle.properties` sets `org.gradle.configuration-cache=false` (Loom issue #1349).
Base the Gradle/Loom setup on the current `FabricMC/fabric-example-mod` template — the
MC 26.x template uses plain `implementation` for loader deps and **no** mappings line.

## The three FFI boundaries

There is exactly **one** native library with Java-visible symbols (the *trampoline*,
crate `native/`). Consumer cdylibs never export `Java_*` symbols — with several loaded
libraries, JVM symbol resolution order is unspecified, so all Java `native` methods live
on one class backed by one library. Consumer libs are opened with `libloading`
(dlopen), not `System.load`, and expose one well-known `extern "C"` symbol.

### Boundary 1: Java → trampoline (JNI native methods)

Class `io.github.bownlux.fabricrust.NativeBridge` (no underscores anywhere in package,
class, or method names — keeps JNI symbol mangling trivial):

```java
final class NativeBridge {
    static final int ABI_VERSION = 1;
    // Loads the trampoline itself. Called once with an absolute path
    // (extracted from our own jar in production; cargo target dir in tests).
    static synchronized void initialize(Path trampolineLibrary);  // does System.load

    static native long openLibrary(String absolutePath);
    // dlsym "fabric_rust_register" in `handle`, call it with `registrar`.
    // Returns the ABI version the library reported.
    static native int registerMod(long handle, EntrypointRegistrar registrar);
    static native void invokeEntrypoint(long fnPtr);
}
```

Trampoline exports (crate `native/`, exact symbol names, all `extern "system"`):

| Symbol | Rust signature (raw `jni::sys` types) |
|---|---|
| `JNI_OnLoad` | `fn(*mut sys::JavaVM, *mut c_void) -> sys::jint` — registers the `JavaVM` singleton, returns `JNI_VERSION_1_8` |
| `Java_io_github_bownlux_fabricrust_NativeBridge_openLibrary` | `fn(env, jclass, jstring) -> jlong` |
| `Java_io_github_bownlux_fabricrust_NativeBridge_registerMod` | `fn(env, jclass, jlong, jobject) -> jint` |
| `Java_io_github_bownlux_fabricrust_NativeBridge_invokeEntrypoint` | `fn(env, jclass, jlong)` |

Rules for every export:
- Body wrapped so that **no panic unwinds across the boundary** (jni 0.22's
  `EnvUnowned::with_env` + `resolve::<ThrowRuntimeExAndDefault>()`, or explicit
  `catch_unwind` → `ThrowNew(java/lang/RuntimeException)`).
- `openLibrary`: `libloading::Library::new(path)`; the `Library` is **leaked**
  (`Box::into_raw`) — fn pointers handed to Java must never dangle; handle = pointer as
  `jlong`. Errors → throw, return 0.
- `registerMod`: dlsym `b"fabric_rust_register\0"` as
  `unsafe extern "system" fn(*mut sys::JNIEnv, sys::jobject) -> sys::jint`; missing
  symbol → throw `"…is not a fabric-rust library (missing fabric_rust_register)"`.
- `invokeEntrypoint`: reject 0; `fnPtr as usize` → transmute to
  `unsafe extern "system" fn(*mut sys::JNIEnv)` and call with the current raw env.
  (fn-ptr ↔ usize ↔ jlong round-trips are well-defined; provenance rules only
  constrain data pointers.)

### Boundary 2: trampoline → consumer cdylib (plain C ABI, versioned)

Every fabric-rust mod cdylib exports exactly one symbol:

```rust
#[unsafe(no_mangle)]
pub extern "system" fn fabric_rust_register(
    env: *mut jni::sys::JNIEnv,
    registrar: jni::sys::jobject,
) -> jni::sys::jint   // = ABI_VERSION (1) on success, < 0 on failure
```

Generated by the SDK macro — mod authors never write it. Its duties (SDK internal):
1. Initialize the **consumer crate's own** `jni` statics: each cdylib carries its own
   copy of the `jni` crate, so the trampoline's `JavaVM` singleton is *not* shared.
   Derive the `JavaVM` from `env` (`GetJavaVM`) and register it as this library's
   singleton.
2. Cache global refs needed later (the `RustBridge` class — `FindClass` works *here*
   because we are inside a native frame whose declaring class (`NativeBridge`) was
   loaded by Fabric's Knot classloader; it does **not** work from Rust-spawned threads,
   so cache now, use forever).
3. For each declared entrypoint, upcall
   `registrar.register(name, fnPtr as jlong)` (resolve the method with
   `GetObjectClass(registrar)` + `GetMethodID("register", "(Ljava/lang/String;J)V")` —
   never `FindClass` for this).
4. Return `1` (ABI version). Panics are caught and reported as a thrown
   `RuntimeException` + negative return.

Entrypoint function pointers registered here have the fixed signature
`unsafe extern "system" fn(*mut jni::sys::JNIEnv)`. The SDK generates a shim per
entrypoint that catches panics, then calls the author's plain `fn foo()`.

### Boundary 3: consumer cdylib → Java (upcalls)

`io.github.bownlux.fabricrust.RustBridge`, static methods only (callable with just a
cached `jclass` from any attached thread):

```java
public final class RustBridge {
    // level: 0=error 1=warn 2=info 3=debug 4=trace; tag → SLF4J logger name
    public static void log(int level, String tag, String message);
    // test hook: when non-null, also receives (level, tag, message)
    static volatile TriConsumer testSink;   // package-private, used by the JUnit harness
}
```

The v0.1 upcall surface is deliberately tiny (logging). Registries/events/commands are
roadmap items and must extend `RustBridge`-style static surfaces, versioned via
`ABI_VERSION`.

## Adapter semantics (`RustLanguageAdapter`)

Registered in our `fabric.mod.json`:

```json
"languageAdapters": { "rust": "io.github.bownlux.fabricrust.RustLanguageAdapter" }
```

Consumer entrypoint declaration:

```json
"entrypoints": {
  "main": [ { "adapter": "rust", "value": "example_mod::init" } ]
}
```

`value` grammar (mirrors FLK's `Class::member` shape):
- `lib_name::fn_name` — library `lib_name` (the cargo `[lib] name`, underscores as
  cargo writes them), entrypoint registered under `fn_name`.
- `lib_name` — shorthand for `lib_name::init`.
- Anything else (3+ `::` parts, empty parts) → `LanguageAdapterException`.

`create(mod, value, type)` flow:
1. Ensure trampoline loaded (extract from own jar via
   `FabricLoader.getInstance().getModContainer("fabric-language-rust")` → `findPath`).
2. Per (mod id, lib name), once: locate `natives/<os>-<arch>/<mapped>` via
   `mod.findPath(...)`, extract to
   `<gameDir>/.fabric-language-rust/<modid>/<sha256[0..8] of the lib>/<mapped>`,
   `openLibrary`, `registerMod` with a fresh `EntrypointRegistrar`, verify reported ABI
   == `ABI_VERSION` else `LanguageAdapterException` with both numbers.
3. Look up `fn_name` in that registrar's map; missing → `LanguageAdapterException`
   listing the names that *were* registered.
4. `type` must be an interface with exactly one zero-arg, void abstract method
   (`ModInitializer`, `ClientModInitializer`, `DedicatedServerModInitializer`, …).
   Otherwise `LanguageAdapterException` explaining the v0.1 limitation.
5. Return a `Proxy` for `type`: SAM method → `NativeBridge.invokeEntrypoint(fnPtr)`;
   `toString`/`hashCode`/`equals` handled locally.

Platform ids: `windows|linux|macos` × `x64|arm64` from `os.name`/`os.arch`
(`amd64|x86_64→x64`, `aarch64→arm64`). Mapped file names: `lib<name>.dylib` /
`lib<name>.so` / `<name>.dll` (= `System.mapLibraryName` behavior).

Our own `fabric.mod.json` depends **only** on `"fabricloader": ">=0.19.0"` and
`"java": ">=25"` — never on `minecraft` (FLK convention; one jar spans game versions).

## Rust SDK (`fabric-rust` + `fabric-rust-macros`)

Mod author experience (the whole point):

```rust
use fabric_rust::prelude::*;

fn init() {
    info!("Hello from Rust! \u{1F980}");
}

fabric_rust::register_entrypoints! {
    "init" => init,
}
```

- `register_entrypoints!` is a proc macro: generates the `fabric_rust_register` export
  (Boundary 2), one panic-catching shim per entry, and compile-fails on duplicate names.
- `prelude` re-exports the log macros `error! warn! info! debug! trace!` which upcall
  `RustBridge.log` through refs cached at register time; `tag` defaults to the crate
  name. Before registration (or if caching failed) they fall back to eprintln! rather
  than panicking.
- The SDK re-exports `jni` so authors can drop to raw JNI (`fabric_rust::jni`).
- Crate names are provisional until published to crates.io.

## Example mod (`rust/example-mod` + `example/`)

- Crate: cdylib `example_mod`, depends on `fabric-rust`, logs a hello line from `init`.
- Jar (`:example` Gradle subproject): `fabric.mod.json` (id `flr-example`, entrypoint
  `main` → `{"adapter": "rust", "value": "example_mod::init"}`, depends on
  `fabric-language-rust` + `fabricloader`) + `natives/<host-platform>/libexample_mod.*`.
- A Gradle task copies the example jar into `run/mods/` and `runClient`/`runServer`
  depend on it, so dev runs exercise the full adapter path.

## Build wiring

- Gradle `Exec` tasks run `cargo build --release` for `native/` and `example-mod/`
  (host target only; cross-compilation happens in CI). Jar packaging picks the artifact
  from `rust/target/release/` and places it at `natives/<host-platform>/…`.
- The `test` task depends on both cargo builds and passes the artifact paths as system
  properties (`flr.test.trampoline`, `flr.test.exampleLib`).
- JDK: compile with `options.release = 25`. Gradle itself may run on JDK 25 or 26.
- Java 25 emits JEP 472 native-access warnings: pass
  `--enable-native-access=ALL-UNNAMED` to loom run configs and the test JVM; document
  it for servers.

## Verification gates

1. `cargo build --workspace` and `cargo test --workspace` green in `rust/`.
2. `./gradlew build` green (adapter jar + example jar, natives inside).
3. `./gradlew test` green — `NativeBridgeHarnessTest` runs the **entire chain without
   Minecraft**: `initialize(trampoline)` → `openLibrary(example_mod)` → `registerMod`
   → assert ABI + registered names → `invokeEntrypoint("init")` → assert the hello
   line arrived at `RustBridge.testSink`. Also asserts error paths (bogus path, bogus
   fn ptr name lookup) throw cleanly instead of crashing the JVM.
4. Manual/optional: `./gradlew runServer` (or `runClient`) shows
   `Hello from Rust!` during mod init on a real 26.2 instance.

## Known limits (v0.1 — document in README)

- No Mixins from Rust (native code can't participate in bytecode transformation);
  pair with a Java/Kotlin side or wait for event-style APIs here.
- Entrypoint interfaces with arguments (e.g. datagen) are not yet adaptable.
- Rust-spawned threads must not use JNI `FindClass` (system classloader ≠ Knot);
  SDK-cached refs are safe. A proper thread/attach helper is roadmap.
- A hard native crash (segfault) takes the JVM with it — panics are caught, UB is not.
- Ships host-platform natives from a local build; the CI matrix
  (windows-x64, linux-x64, linux-arm64, macos-x64, macos-arm64) covers releases.
