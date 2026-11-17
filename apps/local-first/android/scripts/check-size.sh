#!/usr/bin/env bash
# Release-size budget for the Android app.
#
# The debug APK includes symbols and every ABI, so it is never a size signal;
# this script builds release (arm64 only) and fails past the budget.
set -euo pipefail

cd "$(dirname "$0")/.."
apk="gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
budget=33000000

./scripts/build-apk.sh aarch64 release >/dev/null
size=$(stat -c%s "$apk")
printf 'release APK: %s bytes (budget %s)\n' "$size" "$budget"
if [ "$size" -gt "$budget" ]; then
  printf 'error: APK exceeds the size budget (D58)\n' >&2
  exit 1
fi
