package io.github.bownlux.fabricrust;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;

import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/**
 * Runs the entire FFI chain without Minecraft:
 * {@code initialize(trampoline)} → {@code openLibrary(example_mod)} → {@code registerMod}
 * → assert ABI + registered names → {@code invokeEntrypoint("init")} → assert the hello
 * line arrived at {@link RustBridge#testSink}. Also asserts the error paths throw cleanly
 * instead of crashing the JVM.
 *
 * <p>Requires two system properties (absolute library paths), wired by the Gradle
 * {@code test} task, which builds both cargo crates first:
 * <ul>
 *   <li>{@code flr.test.trampoline} — the trampoline cdylib (crate {@code fabric-language-rust-native})</li>
 *   <li>{@code flr.test.exampleLib} — the example cdylib (crate {@code example-mod})</li>
 * </ul>
 */
class NativeBridgeHarnessTest {
	private static EntrypointRegistrar registrar;

	@BeforeAll
	static void loadFullChain() {
		String trampolineProperty = System.getProperty("flr.test.trampoline");
		String exampleProperty = System.getProperty("flr.test.exampleLib");
		assertNotNull(trampolineProperty, "system property flr.test.trampoline must point at the trampoline cdylib");
		assertNotNull(exampleProperty, "system property flr.test.exampleLib must point at the example_mod cdylib");

		Path trampolineLib = Path.of(trampolineProperty);
		Path exampleLib = Path.of(exampleProperty);
		assertTrue(Files.isRegularFile(trampolineLib),
				"trampoline library missing: " + trampolineLib + " (run ./gradlew cargoBuildNative)");
		assertTrue(Files.isRegularFile(exampleLib),
				"example library missing: " + exampleLib + " (run ./gradlew cargoBuildExample)");

		NativeBridge.initialize(trampolineLib);

		long handle = NativeBridge.openLibrary(exampleLib.toAbsolutePath().toString());
		assertNotEquals(0L, handle, "openLibrary returned a null handle");

		registrar = new EntrypointRegistrar("example_mod");
		int reportedAbi = NativeBridge.registerMod(handle, registrar);
		assertEquals(NativeBridge.ABI_VERSION, reportedAbi, "example_mod reported an unexpected ABI version");
	}

	@AfterEach
	void clearTestSink() {
		RustBridge.testSink = null;
	}

	@Test
	void initEntrypointLogsHello() {
		assertTrue(registrar.names().contains("init"),
				"example_mod did not register 'init'; registered: " + registrar.names());

		List<String[]> events = new CopyOnWriteArrayList<>();
		RustBridge.testSink = (level, tag, message) ->
				events.add(new String[] { Integer.toString(level), tag, message });

		NativeBridge.invokeEntrypoint(registrar.requireEntrypoint("init"));

		assertTrue(events.stream().anyMatch(e -> e[2] != null && e[2].contains("Hello from Rust!")),
				"expected a 'Hello from Rust!' log upcall from example_mod::init; got: "
						+ events.stream().map(e -> String.join(" | ", e)).toList());
	}

	@Test
	void nonexistentLibraryPathThrowsInsteadOfCrashing() {
		String bogus = Path.of(System.getProperty("java.io.tmpdir"), "flr-does-not-exist", "libnope.so")
				.toAbsolutePath().toString();

		RuntimeException e = assertThrows(RuntimeException.class, () -> NativeBridge.openLibrary(bogus));
		assertNotNull(e.getMessage(), "the native error should carry a message");
	}

	@Test
	void unknownEntrypointNameGivesHelpfulError() {
		IllegalArgumentException e = assertThrows(IllegalArgumentException.class,
				() -> registrar.requireEntrypoint("no_such_fn"));

		assertTrue(e.getMessage().contains("no_such_fn"),
				"message should name the missing entrypoint: " + e.getMessage());
		assertTrue(e.getMessage().contains("init"),
				"message should list the entrypoints that were registered: " + e.getMessage());
	}

	@Test
	void invokeEntrypointRejectsNullFunctionPointer() {
		assertThrows(RuntimeException.class, () -> NativeBridge.invokeEntrypoint(0L));
	}
}
