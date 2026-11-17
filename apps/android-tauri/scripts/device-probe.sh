#!/usr/bin/env bash
# Rebuild-free device verification for the Android archetype.
#
# Installs the debug APK, grants the runtime permissions the probes need, starts
# the app, waits for the probe files, and prints them. Everything is done through
# adb, so it works on a headless or remote device.
#
#   scripts/device-probe.sh <serial> [path/to/app-universal-debug.apk]
#
# The API key for the chat probe is never passed here: push it separately
# (see the README's "Model turn" section) so it stays out of shell history and
# out of the APK.

set -euo pipefail

serial="${1:?usage: device-probe.sh <serial> [apk]}"
apk="${2:-gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk}"
package=run.hydro.infinityhybrid
data_dir="/data/user/0/$package"

adb -s "$serial" install -r "$apk"
adb -s "$serial" shell pm grant "$package" android.permission.READ_CONTACTS
adb -s "$serial" shell pm grant "$package" android.permission.POST_NOTIFICATIONS

for file in probe.json chat.json native.json; do
  adb -s "$serial" shell run-as "$package" rm -f "$data_dir/$file" 2>/dev/null || true
done

adb -s "$serial" shell am force-stop "$package"
adb -s "$serial" shell am start -n "$package/.MainActivity" >/dev/null

# The native probe waits 3 s so the UI exists before the Kotlin bridge is used.
for _ in $(seq 1 12); do
  if adb -s "$serial" shell run-as "$package" test -f "$data_dir/native.json" 2>/dev/null; then
    break
  fi
  sleep 5
done

for file in probe.json native.json chat.json; do
  echo "=== $file ==="
  adb -s "$serial" shell run-as "$package" cat "$data_dir/$file" 2>&1 || true
done

echo "=== notification (dumpsys) ==="
adb -s "$serial" shell dumpsys notification --noredact 2>/dev/null \
  | grep -A 2 "pkg=$package" | head -12 || echo "(no notification record for $package)"
