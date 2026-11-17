#!/usr/bin/env bash
# Build a debug APK. Encapsulates the NDK environment so callers never have to
# reproduce the env-var set (and never silently fall back to `cc`).
#
#   scripts/build-apk.sh [aarch64|x86_64]
set -euo pipefail

# Run from the app directory regardless of where the script is invoked from:
# `pnpm` resolves the package manifest from the current directory.
cd "$(dirname "$0")/.."

TARGET="${1:-aarch64}"
NDK="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/29.0.13113456}"
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
API=24
CC_BIN="$TC/bin/${TARGET}-linux-android${API}-clang"
[[ "$TARGET" == "armv7" ]] && CC_BIN="$TC/bin/armv7a-linux-androideabi${API}-clang"

# A stale JAVA_HOME (e.g. an uninstalled Android Studio) must not win over a
# working JDK: Gradle fails with "JAVA_HOME is set to an invalid directory".
if [[ ! -x "${JAVA_HOME:-}/bin/java" ]]; then
  export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
fi
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export NDK_HOME="$NDK"
export PATH="$TC/bin:$PATH"

# Dashes make these invalid shell identifiers, so they must go through `env`.
exec env \
  "CC_${TARGET}-linux-android=$CC_BIN" \
  "CXX_${TARGET}-linux-android=$CC_BIN++" \
  "AR_${TARGET}-linux-android=$TC/bin/llvm-ar" \
  "CARGO_TARGET_$(echo "$TARGET" | tr '[:lower:]' '[:upper:]')_LINUX_ANDROID_LINKER=$CC_BIN" \
  pnpm tauri android build --debug --apk --target "$TARGET" --ci
