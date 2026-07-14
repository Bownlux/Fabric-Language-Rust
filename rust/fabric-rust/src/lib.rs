//! # fabric-rust
//!
//! SDK for writing [Fabric](https://fabricmc.net/) mod entrypoints in Rust,
//! loaded by the `fabric-language-rust` language adapter.
//!
//! ```ignore
//! use fabric_rust::prelude::*;
//!
//! fn init() {
//!     info!("Hello from Rust! \u{1F980}");
//! }
//!
//! fabric_rust::register_entrypoints! {
//!     "init" => init,
//! }
//! ```
//!
//! [`register_entrypoints!`] generates the well-known `fabric_rust_register`
//! export (FFI Boundary 2) plus one panic-catching shim per entrypoint. The
//! log macros ([`error!`], [`warn!`], [`info!`], [`debug!`], [`trace!`]) upcall
//! `io.github.bownlux.fabricrust.RustBridge.log(int, String, String)` through
//! JNI references cached at registration time; before registration (or from a
//! thread that is not attached to the JVM) they fall back to `eprintln!` and
//! never panic.
//!
//! Authors who need raw JNI access can use the re-exported [`jni`] crate
//! (`fabric_rust::jni`).

/// Re-export of the `jni` crate (version 0.22) for mod authors who want to
/// drop down to raw JNI.
pub use jni;

/// Generates the `fabric_rust_register` export and per-entrypoint shims.
///
/// ```ignore
/// fabric_rust::register_entrypoints! {
///     "init" => init,
///     "client_init" => client::init,
/// }
/// ```
///
/// Compile-fails on duplicate names or an empty list. Must be invoked exactly
/// once per cdylib, at module (item) position.
pub use fabric_rust_macros::register_entrypoints;

/// The Boundary-2 ABI version this SDK implements. `fabric_rust_register`
/// returns this value on success and the Java side verifies it matches its own
/// `NativeBridge.ABI_VERSION`.
pub const ABI_VERSION: jni::sys::jint = 1;

#[doc(hidden)]
#[path = "internal.rs"]
pub mod __internal;

/// The usual imports for a fabric-rust mod: the log macros and
/// [`register_entrypoints!`].
pub mod prelude {
    pub use crate::register_entrypoints;
    pub use crate::{debug, error, info, trace, warn};
}

/// Logs at ERROR level (0) via `RustBridge.log`; the tag defaults to the
/// calling crate's name. Falls back to `eprintln!` when JNI is unavailable.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::__internal::log_message(
            $crate::__internal::Level::Error,
            ::core::env!("CARGO_CRATE_NAME"),
            &::std::format!($($arg)*),
        )
    };
}

/// Logs at WARN level (1) via `RustBridge.log`; the tag defaults to the
/// calling crate's name. Falls back to `eprintln!` when JNI is unavailable.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::__internal::log_message(
            $crate::__internal::Level::Warn,
            ::core::env!("CARGO_CRATE_NAME"),
            &::std::format!($($arg)*),
        )
    };
}

/// Logs at INFO level (2) via `RustBridge.log`; the tag defaults to the
/// calling crate's name. Falls back to `eprintln!` when JNI is unavailable.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::__internal::log_message(
            $crate::__internal::Level::Info,
            ::core::env!("CARGO_CRATE_NAME"),
            &::std::format!($($arg)*),
        )
    };
}

/// Logs at DEBUG level (3) via `RustBridge.log`; the tag defaults to the
/// calling crate's name. Falls back to `eprintln!` when JNI is unavailable.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::__internal::log_message(
            $crate::__internal::Level::Debug,
            ::core::env!("CARGO_CRATE_NAME"),
            &::std::format!($($arg)*),
        )
    };
}

/// Logs at TRACE level (4) via `RustBridge.log`; the tag defaults to the
/// calling crate's name. Falls back to `eprintln!` when JNI is unavailable.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::__internal::log_message(
            $crate::__internal::Level::Trace,
            ::core::env!("CARGO_CRATE_NAME"),
            &::std::format!($($arg)*),
        )
    };
}
