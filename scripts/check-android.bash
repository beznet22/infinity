#!/usr/bin/env bash
# Android (aarch64-linux-android) compile gate for the fork's foundation crates.
#
# `aws-lc-sys` — the rustls provider behind reqwest's `default-tls` — compiles C,
# so the NDK toolchain must be discoverable. Cargo/cc-rs read dashed env names
# that a POSIX shell cannot `export`, so they are passed through `env`.
#
# Usage: scripts/check-android.bash [abi]
#   abi defaults to aarch64-linux-android (also: x86_64-linux-android,
#   armv7-linux-androideabi, i686-linux-android).
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
RESET='\033[0m'

pass() { echo -e "${GREEN}✓${RESET} $1"; }
fail() { echo -e "${RED}✗${RESET} $1"; }
step() { echo -e "\n${BOLD}▸ $1${RESET}"; }

TARGET="${1:-aarch64-linux-android}"

# Crates that must compile for Android.
CRATES=(
    infinity-protocol
    rap-protocol
    infinity-provider-protocol
    rap-client
    infinity-agent-core
    sandbox-core
    infinity-mcp-bridge
    infinity-host
)

# Map the Rust target to the NDK clang prefix and the env-var suffix.
case "$TARGET" in
    aarch64-linux-android)   NDK_PREFIX=aarch64-linux-android;  SUFFIX=aarch64-linux-android ;;
    x86_64-linux-android)    NDK_PREFIX=x86_64-linux-android;   SUFFIX=x86_64-linux-android ;;
    armv7-linux-androideabi) NDK_PREFIX=armv7a-linux-androideabi; SUFFIX=armv7-linux-androideabi ;;
    i686-linux-android)      NDK_PREFIX=i686-linux-android;     SUFFIX=i686-linux-android ;;
    *) fail "unsupported target: $TARGET"; exit 1 ;;
esac

step "Android readiness ($TARGET)"

if ! rustup target list --installed | grep -qx "$TARGET"; then
    fail "target $TARGET is not installed (run: rustup target add $TARGET)"
    exit 1
fi

# Locate the NDK: NDK_HOME wins, otherwise the highest version under $ANDROID_HOME/ndk.
NDK="${NDK_HOME:-}"
if [ -z "$NDK" ]; then
    if [ -z "${ANDROID_HOME:-}" ] || [ ! -d "$ANDROID_HOME/ndk" ]; then
        fail "no NDK found (set NDK_HOME or ANDROID_HOME with an ndk/ directory)"
        exit 1
    fi
    NDK="$ANDROID_HOME/ndk/$(ls -1 "$ANDROID_HOME/ndk" | sort -V | tail -1)"
fi
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
CLANG="$TC/bin/${NDK_PREFIX}24-clang"
[ -x "$CLANG" ] || CLANG=$(ls "$TC"/bin/"${NDK_PREFIX}"*-clang 2>/dev/null | sort -V | head -1)
if [ -z "${CLANG:-}" ] || [ ! -x "$CLANG" ]; then
    fail "no clang wrapper for $NDK_PREFIX in $TC/bin"
    exit 1
fi
pass "NDK toolchain: $CLANG"

LINKER_VAR="CARGO_TARGET_$(echo "$TARGET" | tr '[:lower:]-' '[:upper:]_')_LINKER"

status=0
for crate in "${CRATES[@]}"; do
    if PATH="$TC/bin:$PATH" env \
        "CC_$SUFFIX=$CLANG" \
        "CXX_$SUFFIX=${CLANG}++" \
        "AR_$SUFFIX=$TC/bin/llvm-ar" \
        "$LINKER_VAR=$CLANG" \
        cargo check --quiet --target "$TARGET" -p "$crate" > /dev/null 2>&1; then
        pass "$crate"
    else
        fail "$crate"
        echo "    run: PATH=\"$TC/bin:\$PATH\" env \"CC_$SUFFIX=$CLANG\" \"$LINKER_VAR=$CLANG\" cargo check --target $TARGET -p $crate"
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo
    pass "All foundation crates compile for $TARGET"
fi

exit "$status"
