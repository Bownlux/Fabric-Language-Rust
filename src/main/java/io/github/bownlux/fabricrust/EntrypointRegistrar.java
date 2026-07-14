package io.github.bownlux.fabricrust;

import java.util.Collections;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Per-(mod, library) registration callback object, passed to the consumer library's
 * {@code fabric_rust_register} export through {@link NativeBridge#registerMod}.
 *
 * <p>The native side resolves {@link #register} via {@code GetObjectClass(registrar)} +
 * {@code GetMethodID("register", "(Ljava/lang/String;J)V")}, never via {@code FindClass}.
 */
public final class EntrypointRegistrar {
	private final String libraryName;
	private final Map<String, Long> entrypoints = new ConcurrentHashMap<>();

	EntrypointRegistrar(String libraryName) {
		this.libraryName = libraryName;
	}

	/**
	 * Upcalled from native code once per declared entrypoint.
	 *
	 * @param name  the entrypoint name (as declared in {@code register_entrypoints!})
	 * @param fnPtr the entrypoint function pointer ({@code unsafe extern "system" fn(*mut JNIEnv)})
	 */
	public void register(String name, long fnPtr) {
		if (name == null || name.isEmpty()) {
			throw new IllegalArgumentException(
					"library '" + libraryName + "' tried to register an entrypoint with an empty name");
		}

		if (fnPtr == 0) {
			throw new IllegalArgumentException(
					"library '" + libraryName + "' tried to register a null function pointer for entrypoint '" + name + "'");
		}

		Long previous = entrypoints.putIfAbsent(name, fnPtr);

		if (previous != null) {
			throw new IllegalStateException(
					"library '" + libraryName + "' registered entrypoint '" + name + "' twice");
		}
	}

	String libraryName() {
		return libraryName;
	}

	/** Names registered so far (unmodifiable live view). */
	Set<String> names() {
		return Collections.unmodifiableSet(entrypoints.keySet());
	}

	/**
	 * Looks up a registered entrypoint.
	 *
	 * @throws IllegalArgumentException naming the missing entrypoint and listing the ones
	 *         that were actually registered
	 */
	long requireEntrypoint(String name) {
		Long fnPtr = entrypoints.get(name);

		if (fnPtr == null) {
			throw new IllegalArgumentException(
					"library '" + libraryName + "' has no entrypoint named '" + name
							+ "'; registered entrypoints: " + new TreeSet<>(entrypoints.keySet()));
		}

		return fnPtr;
	}
}
