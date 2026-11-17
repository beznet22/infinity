# Android Tauri archetype

A **consumer-side** reference app (not a workspace member — see decision D16). It shows the smallest end-to-end integration of the fork's crates on Android: assemble [`HostCtx`](../../crates/infinity-host), run a sandbox command, and expose the result to the webview.

## What it proves

- `infinity-host`'s `HostCtx` is assembled from real implementations: in-memory stores, `SimpleHttpClient` (reqwest → rustls), `InMemoryToolsetCache`, `ProcessSandboxExecutor`.
- `sandbox-core`'s `ProcessSandboxExecutor` spawns a real process inside the Android app (`sh -c echo …`).
- Tauri v2 mobile wiring: `#[tauri::mobile_entry_point]`, `staticlib`/`cdylib` crate types, `generate_context!`.

The app also runs the probe once at startup and writes the result to its data dir, so a device run can be verified without touching the UI:

```bash
adb shell run-as run.hydro.infinityhybrid cat probe.json
```

## Verified

On a real device (Android 13, arm64-v8a), installed and launched headlessly:

```json
{ "app_version": "0.1.0", "target": "aarch64", "sandbox_status": 0,
  "sandbox_stdout": "infinity-on-android" }
```

The device run also caught a real bug: `ProcessSandboxExecutor` spawned with a
cwd that did not exist yet (`ENOENT`). The executor now creates its root (and an
explicit `cwd`) before spawning, with a regression test.

## Headless debugging (no clicking)

```bash
S=<serial>
adb -s $S shell am start -n run.hydro.infinityhybrid/.MainActivity
adb -s $S logcat --pid=$(adb -s $S shell pidof -s run.hydro.infinityhybrid)
adb -s $S exec-out screencap -p > shot.png
adb -s $S exec-out uiautomator dump /dev/tty       # parse bounds for input tap
```

Debug builds enable WebView remote debugging (wry compiles
`setWebContentsDebuggingEnabled` under `debug_assertions`), so JS can be driven
over CDP without touching the screen:

```bash
pid=$(adb -s $S shell pidof -s run.hydro.infinityhybrid)
adb -s $S forward tcp:9301 localabstract:webview_devtools_remote_$pid
curl -s localhost:9301/json/list                  # targets + webSocketDebuggerUrl
```

Multiple apps coexist on one device as long as their `applicationId`s differ
(ours is `run.hydro.infinityhybrid`); only host-side `adb forward` ports can
collide, so pick a free one (9301 here).

## Build

Prerequisites: Rust Android target, Android SDK + NDK, JDK 17, Node + pnpm.

```bash
# 1. Toolchain
rustup target add aarch64-linux-android x86_64-linux-android

# 2. The NDK must be discoverable: aws-lc-sys (rustls' provider) compiles C.
NDK=$ANDROID_HOME/ndk/<version>
TC=$NDK/toolchains/llvm/prebuilt/linux-x86_64

# 3. Tauri CLI + Android project
pnpm install
pnpm tauri android init --ci --skip-targets-install

# 4. Build a signed debug APK (release builds are unsigned by default)
PATH="$TC/bin:$PATH" env \
  JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64 \
  "CC_x86_64-linux-android=$TC/bin/x86_64-linux-android24-clang" \
  "CXX_x86_64-linux-android=$TC/bin/x86_64-linux-android24-clang++" \
  "AR_x86_64-linux-android=$TC/bin/llvm-ar" \
  "CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER=$TC/bin/x86_64-linux-android24-clang" \
  ANDROID_HOME="$ANDROID_HOME" NDK_HOME="$NDK" \
  pnpm tauri android build --debug --apk --target x86_64 --ci
```

Swap `x86_64` for `aarch64` for a physical device (use `aarch64-linux-android24-clang` for `CC`/`CXX`/linker).

## Notes

- `frontendDist` is resolved relative to `tauri.conf.json` (`dist`, not `../dist`).
- `bundle.targets` must be `"all"`; the APK comes from the `--apk` flag, not from `bundle.targets`.
- Tauri requires **RGBA** PNG icons (`icons/*.png`, color type 6).
- The NDK linker/`CC` env vars are only needed because Cargo cannot read dashed names from a shell `export` — `env "NAME=value"` works.
- `gen/` is generated and git-ignored; re-run `pnpm tauri android init` after cloning.
- For a real conversation you need a provider key. Keep it runtime-supplied (environment or a settings screen), never baked into the APK — see decision D17 and the BYOK note in `docs/docs/WASM/overview.md` §2.5.
