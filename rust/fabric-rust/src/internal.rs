//! Implementation details behind `register_entrypoints!` and the log macros.
//!
//! Everything here is `#[doc(hidden)]` public only so macro-generated code in
//! consumer crates can reach it through `::fabric_rust::__internal::...`.
//! None of it is stable API; don't call it by hand.

use std::any::Any;
use std::sync::OnceLock;

use jni::errors::{ErrorPolicy, ThrowRuntimeExAndDefault};
use jni::ids::JStaticMethodID;
use jni::objects::{JClass, JObject};
use jni::refs::Global;
use jni::signature::{Primitive, ReturnType};
use jni::{Env, EnvUnowned, jni_sig, jni_str, sys};

use crate::ABI_VERSION;

/// Fixed signature of every registered entrypoint shim (FFI Boundary 2).
pub type EntrypointFn = unsafe extern "system" fn(*mut sys::JNIEnv);

/// JNI name of the Java upcall surface (FFI Boundary 3).
const RUST_BRIDGE_CLASS: &jni::strings::JNIStr = jni_str!("io/github/bownlux/fabricrust/RustBridge");

/// Log levels understood by `RustBridge.log(int, String, String)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl Level {
    /// Human-readable name, used by the `eprintln!` fallback.
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// Inverse of `level as i32`; `None` for out-of-range values.
    pub fn from_i32(value: i32) -> Option<Level> {
        match value {
            0 => Some(Level::Error),
            1 => Some(Level::Warn),
            2 => Some(Level::Info),
            3 => Some(Level::Debug),
            4 => Some(Level::Trace),
            _ => None,
        }
    }
}

/// JNI references cached once, inside the `fabric_rust_register` native frame,
/// where `FindClass` still resolves against Fabric's Knot classloader. A global
/// class ref plus a static method id can be used later from any attached
/// thread; a `FindClass` call from one of those threads could not.
struct LogCache {
    rust_bridge: Global<JClass<'static>>,
    log_method: JStaticMethodID,
}

static LOG_CACHE: OnceLock<LogCache> = OnceLock::new();

/// Error type for SDK-internal failures; surfaced to Java as a
/// `RuntimeException` message.
#[derive(Debug)]
pub enum SdkError {
    Jni(jni::errors::Error),
    Msg(String),
}

impl std::fmt::Display for SdkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SdkError::Jni(e) => write!(f, "{e}"),
            SdkError::Msg(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for SdkError {}

impl From<jni::errors::Error> for SdkError {
    fn from(e: jni::errors::Error) -> Self {
        SdkError::Jni(e)
    }
}

/// Best-effort extraction of a panic payload's message.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Returns the first name that appears more than once, if any.
/// (The proc macro rejects duplicates at compile time; this is the runtime
/// backstop for hand-rolled entry tables.)
pub fn find_duplicate<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if seen.contains(&name) {
            return Some(name);
        }
        seen.push(name);
    }
    None
}

/// Error policy for `fabric_rust_register`: throw a `RuntimeException`
/// (unless one is already pending) and return `-1` (< 0 = failure per the
/// Boundary-2 contract, where success returns the positive ABI version).
struct ThrowAndMinusOne;

impl<E: std::error::Error> ErrorPolicy<sys::jint, E> for ThrowAndMinusOne {
    type Captures<'unowned_env_local: 'native_method, 'native_method> = ();

    fn on_error<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _cap: &mut Self::Captures<'unowned_env_local, 'native_method>,
        err: E,
    ) -> jni::errors::Result<sys::jint> {
        if !env.exception_check() {
            let _ = env.throw(format!("fabric-rust registration failed: {err}"));
        }
        Ok(-1)
    }

    fn on_panic<'unowned_env_local: 'native_method, 'native_method>(
        env: &mut Env<'unowned_env_local>,
        _cap: &mut Self::Captures<'unowned_env_local, 'native_method>,
        payload: Box<dyn Any + Send + 'static>,
    ) -> jni::errors::Result<sys::jint> {
        let message = panic_message(payload.as_ref());
        if !env.exception_check() {
            let _ = env.throw(format!("fabric-rust registration panicked: {message}"));
        }
        Ok(-1)
    }

    fn on_internal_jni_error<'unowned_env_local: 'native_method, 'native_method>(
        _cap: &mut Self::Captures<'unowned_env_local, 'native_method>,
        _err: jni::errors::Error,
    ) -> sys::jint {
        -1
    }

    fn on_internal_panic<'unowned_env_local: 'native_method, 'native_method>(
        _cap: &mut Self::Captures<'unowned_env_local, 'native_method>,
        _payload: Box<dyn Any + Send + 'static>,
    ) -> sys::jint {
        -1
    }
}

/// Body of the macro-generated `fabric_rust_register` export (Boundary 2).
///
/// A few things have to happen in this exact native frame. `EnvUnowned::with_env`
/// registers this cdylib's own `JavaVM` singleton (each consumer cdylib carries
/// its own copy of the `jni` crate, so the trampoline's singleton doesn't help
/// us). We then cache a global ref to the `RustBridge` class and its static
/// `log` method id: `FindClass` works here because the declaring class of this
/// frame (`NativeBridge`) was loaded by Fabric's Knot classloader, and it does
/// not work from Rust-spawned threads. After that we upcall
/// `registrar.register(name, fnPtr)` for each declared entrypoint, resolved via
/// `GetObjectClass(registrar)` + `GetMethodID` rather than `FindClass`, and
/// return 1 (the ABI version). Panics are caught and reported as a thrown
/// `RuntimeException` plus a negative return value.
///
/// # Safety
///
/// `env` must be the valid, attached `JNIEnv` pointer of the current native
/// frame and `registrar` a valid local reference belonging to that frame.
pub unsafe fn register(
    env: *mut sys::JNIEnv,
    registrar: sys::jobject,
    entries: &[(&str, EntrypointFn)],
) -> sys::jint {
    if env.is_null() {
        return -1;
    }
    // Safety: caller contract, a valid attached env pointer for this frame.
    let mut unowned = unsafe { EnvUnowned::from_raw(env) };
    unowned
        .with_env(|env| -> Result<sys::jint, SdkError> {
            // with_env has already registered this cdylib's JavaVM singleton
            // (derived from `env` via GetJavaVM). Verify it stuck so the log
            // macros can rely on `JavaVM::singleton()` later.
            jni::JavaVM::singleton()?;

            if registrar.is_null() {
                return Err(SdkError::Msg(
                    "fabric_rust_register called with a null registrar".to_string(),
                ));
            }
            if let Some(dup) = find_duplicate(entries.iter().map(|(name, _)| *name)) {
                return Err(SdkError::Msg(format!(
                    "duplicate entrypoint name `{dup}` in register_entrypoints!"
                )));
            }

            // Cache the RustBridge class + static log method now, while
            // FindClass still resolves against the Knot classloader.
            init_log_cache(env)?;

            // Upcall registrar.register(name, fnPtr) for every entry.
            // Safety: `registrar` is a valid local ref of this native frame.
            let registrar = unsafe { JObject::from_raw(env, registrar) };
            let registrar_class = env.get_object_class(&registrar)?;
            let register_method = env.get_method_id(
                &registrar_class,
                jni_str!("register"),
                jni_sig!("(Ljava/lang/String;J)V"),
            )?;
            for (name, entrypoint) in entries {
                let jname = env.new_string(name)?;
                let args = [
                    sys::jvalue { l: jname.as_raw() },
                    sys::jvalue {
                        j: *entrypoint as usize as sys::jlong,
                    },
                ];
                // Safety: method id was resolved from this registrar's class
                // with the exact signature (Ljava/lang/String;J)V matching
                // `args`.
                unsafe {
                    env.call_method_unchecked(
                        &registrar,
                        register_method,
                        ReturnType::Primitive(Primitive::Void),
                        &args,
                    )?;
                }
            }

            Ok(ABI_VERSION)
        })
        .resolve::<ThrowAndMinusOne>()
}

/// Body of every macro-generated entrypoint shim: call the author's plain
/// `fn()`, catching panics and rethrowing them as `RuntimeException` instead
/// of unwinding into the JVM.
///
/// # Safety
///
/// `env` must be the valid, attached `JNIEnv` pointer of the current native
/// frame (`NativeBridge.invokeEntrypoint`).
pub unsafe fn invoke_entrypoint(env: *mut sys::JNIEnv, entrypoint: fn(), name: &str) {
    if env.is_null() {
        // No way to throw without an env; refuse to run rather than crash.
        eprintln!("[fabric-rust] invoke_entrypoint(`{name}`) called with a null JNIEnv");
        return;
    }
    // Safety: caller contract, a valid attached env pointer for this frame.
    let mut unowned = unsafe { EnvUnowned::from_raw(env) };
    unowned
        .with_env(|_env| -> Result<(), SdkError> {
            // Catch here (in addition to with_env's own catch_unwind) so the
            // thrown message can name the entrypoint.
            std::panic::catch_unwind(entrypoint).map_err(|payload| {
                SdkError::Msg(format!(
                    "entrypoint `{name}` panicked: {}",
                    panic_message(payload.as_ref())
                ))
            })?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

/// Resolve and cache the `RustBridge` global class ref + static `log` method
/// id (idempotent).
fn init_log_cache(env: &mut Env<'_>) -> Result<(), SdkError> {
    if LOG_CACHE.get().is_some() {
        return Ok(());
    }
    let class = env.find_class(RUST_BRIDGE_CLASS)?;
    let log_method = env.get_static_method_id(
        &class,
        jni_str!("log"),
        jni_sig!("(ILjava/lang/String;Ljava/lang/String;)V"),
    )?;
    let rust_bridge = env.new_global_ref(&class)?;
    // Lost race with another thread is fine: same class, same method id.
    let _ = LOG_CACHE.set(LogCache {
        rust_bridge,
        log_method,
    });
    Ok(())
}

/// Backend of the `error!`/`warn!`/`info!`/`debug!`/`trace!` macros.
///
/// Upcalls `RustBridge.log(level, tag, message)` through the refs cached by
/// [`register`]. If the cache is not initialized, the JavaVM singleton is
/// missing, or the current thread is not attached to the JVM, falls back to
/// `eprintln!`. Never panics.
pub fn log_message(level: Level, tag: &str, message: &str) {
    let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        try_log_via_jni(level, tag, message).is_ok()
    }))
    .unwrap_or(false);
    if !delivered {
        eprintln!("[{}] [{}] {}", level.name(), tag, message);
    }
}

fn try_log_via_jni(level: Level, tag: &str, message: &str) -> Result<(), ()> {
    let cache = LOG_CACHE.get().ok_or(())?;
    let vm = jni::JavaVM::singleton().map_err(|_| ())?;
    if !vm.is_thread_attached().map_err(|_| ())? {
        return Err(());
    }
    vm.with_local_frame(4, |env| -> Result<(), jni::errors::Error> {
        let jtag = env.new_string(tag)?;
        let jmessage = env.new_string(message)?;
        let args = [
            sys::jvalue { i: level as i32 },
            sys::jvalue { l: jtag.as_raw() },
            sys::jvalue {
                l: jmessage.as_raw(),
            },
        ];
        // Safety: cached static method id matches the cached class and the
        // exact signature (ILjava/lang/String;Ljava/lang/String;)V of `args`.
        let result = unsafe {
            env.call_static_method_unchecked(
                &cache.rust_bridge,
                cache.log_method,
                ReturnType::Primitive(Primitive::Void),
                &args,
            )
        };
        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                // A logging failure must not leave a pending exception in the
                // caller's frame.
                if env.exception_check() {
                    env.exception_describe();
                    env.exception_clear();
                }
                Err(e)
            }
        }
    })
    .map_err(|_: jni::errors::Error| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_values_match_rust_bridge_contract() {
        assert_eq!(Level::Error as i32, 0);
        assert_eq!(Level::Warn as i32, 1);
        assert_eq!(Level::Info as i32, 2);
        assert_eq!(Level::Debug as i32, 3);
        assert_eq!(Level::Trace as i32, 4);
    }

    #[test]
    fn level_round_trips_through_i32() {
        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            assert_eq!(Level::from_i32(level as i32), Some(level));
        }
        assert_eq!(Level::from_i32(-1), None);
        assert_eq!(Level::from_i32(5), None);
    }

    #[test]
    fn level_names() {
        assert_eq!(Level::Error.name(), "ERROR");
        assert_eq!(Level::Trace.name(), "TRACE");
    }

    #[test]
    fn find_duplicate_detects_repeats() {
        assert_eq!(find_duplicate(["a", "b", "a"]), Some("a"));
        assert_eq!(find_duplicate(["a", "b", "c"]), None);
        assert_eq!(find_duplicate(std::iter::empty::<&str>()), None);
        assert_eq!(find_duplicate(["x", "x"]), Some("x"));
    }

    #[test]
    fn panic_message_handles_common_payloads() {
        let str_payload: Box<dyn Any + Send> = Box::new("static str panic");
        assert_eq!(panic_message(str_payload.as_ref()), "static str panic");

        let string_payload: Box<dyn Any + Send> = Box::new(String::from("owned panic"));
        assert_eq!(panic_message(string_payload.as_ref()), "owned panic");

        let weird_payload: Box<dyn Any + Send> = Box::new(42_u32);
        assert_eq!(panic_message(weird_payload.as_ref()), "non-string panic payload");
    }

    #[test]
    fn log_message_falls_back_to_stderr_without_jvm() {
        // No JVM in unit tests: must not panic, must take the eprintln path.
        log_message(Level::Info, "fabric_rust_test", "fallback works");
    }

    #[test]
    fn abi_version_is_one() {
        assert_eq!(ABI_VERSION, 1);
    }
}
