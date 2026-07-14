package io.github.bownlux.fabricrust;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Boundary 3: the upcall surface for Rust. Static methods only, so the native side can
 * call them with nothing but a cached {@code jclass} from any attached thread.
 *
 * <p>In v0.1 that means logging and nothing else. Anything added later (registries, events,
 * commands) has to keep the static-method style and be versioned via
 * {@link NativeBridge#ABI_VERSION}.
 */
public final class RustBridge {
	@FunctionalInterface
	interface TriConsumer {
		void accept(int level, String tag, String message);
	}

	/** Test hook: when non-null, also receives (level, tag, message). Used by the JUnit harness. */
	static volatile TriConsumer testSink;

	private RustBridge() {
	}

	/**
	 * Logging upcall from Rust ({@code error!}/{@code warn!}/{@code info!}/{@code debug!}/{@code trace!}).
	 *
	 * @param level   0=error 1=warn 2=info 3=debug 4=trace
	 * @param tag     SLF4J logger name (the SDK defaults this to the crate name)
	 * @param message the formatted message
	 */
	public static void log(int level, String tag, String message) {
		String safeTag = (tag == null || tag.isEmpty()) ? "fabric-language-rust" : tag;
		String safeMessage = (message == null) ? "" : message;
		Logger logger = LoggerFactory.getLogger(safeTag);

		switch (level) {
			case 0 -> logger.error(safeMessage);
			case 1 -> logger.warn(safeMessage);
			case 2 -> logger.info(safeMessage);
			case 3 -> logger.debug(safeMessage);
			case 4 -> logger.trace(safeMessage);
			default -> logger.info("[unknown level {}] {}", level, safeMessage);
		}

		TriConsumer sink = testSink;

		if (sink != null) {
			sink.accept(level, safeTag, safeMessage);
		}
	}
}
