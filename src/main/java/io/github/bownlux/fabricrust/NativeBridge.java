package io.github.bownlux.fabricrust;

import java.nio.file.Files;
import java.nio.file.Path;

/**
 * The single Java-visible surface of the native trampoline (crate {@code native/},
 * {@code [lib] name = "fabric_language_rust"}).
 *
 * <p>All {@code native} methods of this project live on this one class, backed by that one
 * library — consumer cdylibs never export {@code Java_*} symbols, because with several
 * loaded libraries JVM symbol resolution order is unspecified. No underscores appear in the
 * package, class, or method names, which keeps the JNI symbol mangling trivial:
 * {@code Java_io_github_bownlux_fabricrust_NativeBridge_<method>}.
 */
final class NativeBridge {
	/** Boundary-2 ABI version; consumer libraries must report exactly this from registration. */
	static final int ABI_VERSION = 1;

	private static Path loadedFrom;

	private NativeBridge() {
	}

	/**
	 * Loads the trampoline itself. Called once with an absolute path (extracted from our own
	 * jar in production; the cargo target dir in tests). Subsequent calls are no-ops.
	 */
	static synchronized void initialize(Path trampolineLibrary) {
		if (loadedFrom != null) {
			return;
		}

		Path absolute = trampolineLibrary.toAbsolutePath().normalize();

		if (!Files.isRegularFile(absolute)) {
			throw new IllegalArgumentException(
					"fabric-language-rust trampoline library does not exist: " + absolute);
		}

		System.load(absolute.toString());
		loadedFrom = absolute;
	}

	static synchronized boolean isInitialized() {
		return loadedFrom != null;
	}

	/**
	 * dlopens the consumer cdylib at {@code absolutePath} (via {@code libloading}; the library
	 * is intentionally leaked so registered function pointers never dangle).
	 *
	 * @return an opaque non-zero handle
	 * @throws RuntimeException (thrown from native code) if the library cannot be opened
	 */
	static native long openLibrary(String absolutePath);

	/**
	 * dlsyms {@code fabric_rust_register} in {@code handle} and calls it with {@code registrar}.
	 *
	 * @return the ABI version the library reported
	 * @throws RuntimeException (thrown from native code) if the symbol is missing or
	 *         registration fails
	 */
	static native int registerMod(long handle, EntrypointRegistrar registrar);

	/**
	 * Invokes a registered entrypoint function pointer (fixed signature
	 * {@code unsafe extern "system" fn(*mut JNIEnv)}). Rust panics are caught on the other
	 * side and rethrown here as {@link RuntimeException}s.
	 */
	static native void invokeEntrypoint(long fnPtr);
}
