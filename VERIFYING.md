# Verifying a build

This mod ships compiled native libraries (`.dll`, `.so`, `.dylib`) inside its jar,
which you cannot read the way you can read Java bytecode. This page is how you check
that those binaries came from the source in this repository and nothing else.

Written for Modrinth moderation, but anyone can run every command here.

## Short version

Every file published to Modrinth is built by GitHub Actions from a tagged commit, in a
public log, and signed by GitHub. Nothing is ever uploaded from a developer machine.
To check a jar you downloaded:

```sh
gh attestation verify fabric-language-rust-0.1.0.jar --repo Bownlux/Fabric-Language-Rust
```

That either prints the commit, workflow, and run that produced the exact bytes you
have, or it fails. It is a cryptographic signature from GitHub's signing infrastructure,
so it cannot be forged by the repository owner. One attestation covers all the jars in
a release, so verifying any one of them returns the same statement.

Each release also carries `SHA256SUMS.txt`. Download the assets into one directory and:

```sh
sha256sum -c SHA256SUMS.txt      # shasum -a 256 -c SHA256SUMS.txt on macOS
```

## What is in the jar and why

```
fabric-language-rust-<version>.jar
├── META-INF/MANIFEST.MF
├── fabric.mod.json                  mod metadata, registers the "rust" language adapter
├── assets/…/icon.png                mod icon
├── io/github/bownlux/fabricrust/    the adapter itself: 5 source files, 7 .class
│                                    entries (two are nested classes)
├── LICENSE_fabric-language-rust
└── natives/<platform>/              the part you cannot read: one small Rust library
    ├── windows-x64/fabric_language_rust.dll        per supported platform
    ├── linux-x64/libfabric_language_rust.so
    ├── linux-arm64/libfabric_language_rust.so
    ├── macos-x64/libfabric_language_rust.dylib
    └── macos-arm64/libfabric_language_rust.dylib
```

Those native libraries are built from [`rust/native/`](rust/native/) and nothing else.
The crate is small and its whole job is JNI plumbing: it `dlopen`s a Rust mod's library,
looks up one symbol in it, and calls function pointers. It exports exactly four symbols,
which you can check on any of the shipped binaries:

```sh
unzip -p fabric-language-rust-0.1.0.jar natives/linux-x64/libfabric_language_rust.so > /tmp/flr.so
nm -D --defined-only /tmp/flr.so | grep -E 'JNI_OnLoad|Java_'
```

Expect exactly these, matching the table in [DESIGN.md](DESIGN.md):

```
JNI_OnLoad
Java_io_github_bownlux_fabricrust_NativeBridge_openLibrary
Java_io_github_bownlux_fabricrust_NativeBridge_registerMod
Java_io_github_bownlux_fabricrust_NativeBridge_invokeEntrypoint
```

On macOS the equivalent is `nm -gU libfabric_language_rust.dylib`, where the same four
symbols appear with a leading underscore.

There is no networking, no file writing outside the extraction cache, and no process
spawning in that crate. `rust/native/src/lib.rs` is 227 lines; reading it end to end is
faster than auditing most Java mods.

## How the binaries are built

[`.github/workflows/build.yml`](.github/workflows/build.yml) is the only thing that
produces a release. On a pushed tag it:

1. builds the native libraries for all five platforms on GitHub-hosted runners, one job
   per platform, each printing its own checksums into the public log;
2. merges them into the jars in a single packaging job;
3. lists the full contents of every jar and writes `SHA256SUMS.txt`, again in the log;
4. signs a build provenance attestation binding each jar's digest to the commit and
   workflow that made it;
5. attaches the jars and `SHA256SUMS.txt` to the GitHub release.

The build is pinned so it cannot drift: the Rust compiler version is fixed in
[`rust/rust-toolchain.toml`](rust/rust-toolchain.toml), Rust dependencies are locked to
`rust/Cargo.lock` and built with `--locked` (which fails rather than silently resolving
a different version), and Java dependency versions are pinned in `gradle.properties`.

## Reproducing the jar yourself

Archives are built with timestamps stripped and stable entry ordering, and the native
libraries are compiled with absolute build paths remapped away, so the same commit
produces the same bytes rather than embedding who built it and where.

```sh
git clone https://github.com/Bownlux/Fabric-Language-Rust
cd Fabric-Language-Rust
git checkout v0.1.0
./gradlew clean build
shasum -a 256 build/libs/fabric-language-rust-0.1.0.jar
```

Two clean builds on the same machine and toolchain give byte-identical jars; that is
verified before each release.

One honest caveat: a jar you build locally will **not** hash the same as the release
jar, because the release bundles native libraries cross-built on five different runners
while your local build contains only your own platform's. To compare like with like,
extract a single native library from the release jar and compare it against the one your
machine produced:

```sh
unzip -p fabric-language-rust-0.1.0.jar natives/macos-arm64/libfabric_language_rust.dylib | shasum -a 256
shasum -a 256 rust/target/release/libfabric_language_rust.dylib
```

Cross-platform binary reproduction also depends on the linker and system libraries of
the machine doing the build, so treat a mismatch there as a reason to look at the
attestation and the build log rather than as proof of tampering. The attestation is the
authoritative check; reproducibility is a supporting one.

## Source of truth

- Source: <https://github.com/Bownlux/Fabric-Language-Rust>
- Releases and build logs: <https://github.com/Bownlux/Fabric-Language-Rust/actions>
- Architecture and the full FFI contract: [DESIGN.md](DESIGN.md)

If anything here does not check out, please open an issue rather than assuming a
packaging mistake is malice, and I will fix it.
