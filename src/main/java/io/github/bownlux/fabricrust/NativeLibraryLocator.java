package io.github.bownlux.fabricrust;

import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.Locale;

import net.fabricmc.loader.api.FabricLoader;
import net.fabricmc.loader.api.ModContainer;

/**
 * Platform detection and jar extraction for bundled native libraries.
 *
 * <p>Platform ids are {@code windows|linux|macos} × {@code x64|arm64}, mapped library file
 * names follow {@link System#mapLibraryName} behavior ({@code lib<name>.dylib} /
 * {@code lib<name>.so} / {@code <name>.dll}). These rules are mirrored by the Gradle build
 * (root {@code build.gradle}) when it packages natives — keep the two in sync.
 */
final class NativeLibraryLocator {
	static final String CACHE_DIR_NAME = ".fabric-language-rust";

	private NativeLibraryLocator() {
	}

	/** @return e.g. {@code "macos-arm64"}; throws for architectures we ship no natives for */
	static String platformId() {
		String osName = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
		String osArch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);

		String os;

		if (osName.contains("win")) {
			os = "windows";
		} else if (osName.contains("mac") || osName.contains("darwin")) {
			os = "macos";
		} else {
			os = "linux";
		}

		String arch = switch (osArch) {
			case "amd64", "x86_64" -> "x64";
			case "aarch64", "arm64" -> "arm64";
			default -> throw new UnsupportedOperationException(
					"fabric-language-rust does not support the host architecture '" + osArch + "'"
							+ " (supported: x64, arm64)");
		};

		return os + "-" + arch;
	}

	/** Maps a cargo {@code [lib] name} to the platform's shared-library file name. */
	static String mappedLibraryName(String platformId, String libName) {
		if (platformId.startsWith("windows-")) {
			return libName + ".dll";
		}

		if (platformId.startsWith("macos-")) {
			return "lib" + libName + ".dylib";
		}

		return "lib" + libName + ".so";
	}

	/**
	 * Locates {@code natives/<os>-<arch>/<mapped>} inside {@code mod}'s jar and extracts it to
	 * {@code <gameDir>/.fabric-language-rust/<modid>/<sha256[0..8] of the lib>/<mapped>}.
	 * The sha-addressed directory makes extraction idempotent across runs and versions.
	 *
	 * @return the absolute path of the extracted library on disk
	 * @throws IOException if the library is missing from the jar or cannot be extracted
	 */
	static Path extract(ModContainer mod, String libName) throws IOException {
		String modId = mod.getMetadata().getId();
		String platform = platformId();
		String mapped = mappedLibraryName(platform, libName);
		String resourcePath = "natives/" + platform + "/" + mapped;

		Path source = mod.findPath(resourcePath).orElseThrow(() -> new NoSuchFileException(
				resourcePath, null,
				"mod '" + modId + "' does not bundle native library '" + libName
						+ "' for platform '" + platform + "'"));

		byte[] bytes = Files.readAllBytes(source);
		String sha8 = sha256Hex(bytes).substring(0, 8);

		Path targetDir = FabricLoader.getInstance().getGameDir()
				.resolve(CACHE_DIR_NAME)
				.resolve(modId)
				.resolve(sha8);
		Path target = targetDir.resolve(mapped);

		if (!Files.isRegularFile(target)) {
			Files.createDirectories(targetDir);
			Path tmp = Files.createTempFile(targetDir, mapped + ".", ".tmp");

			try {
				Files.write(tmp, bytes);

				try {
					Files.move(tmp, target, StandardCopyOption.ATOMIC_MOVE);
				} catch (FileAlreadyExistsException | AtomicMoveNotSupportedException e) {
					// Lost a race with another process, or the FS cannot do atomic moves.
					// The sha-addressed path guarantees identical content, so an existing
					// file is fine; otherwise fall back to a plain replace.
					if (!Files.isRegularFile(target)) {
						Files.move(tmp, target, StandardCopyOption.REPLACE_EXISTING);
					}
				}
			} finally {
				Files.deleteIfExists(tmp);
			}
		}

		return target.toAbsolutePath();
	}

	private static String sha256Hex(byte[] bytes) {
		try {
			return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
		} catch (NoSuchAlgorithmException e) {
			throw new AssertionError("SHA-256 is guaranteed to be available", e);
		}
	}
}
