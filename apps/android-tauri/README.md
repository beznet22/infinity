# Android Tauri archetype

A **consumer-side** reference app (not a workspace member — see decision D16). It shows the smallest end-to-end integration of the fork's crates on Android: assemble [`HostCtx`](../../crates/infinity-host), run a sandbox command, and expose the result to the webview.

## What it proves

- `infinity-host`'s `HostCtx` is assembled from real implementations: in-memory stores, `SimpleHttpClient` (reqwest → rustls), `InMemoryToolsetCache`, `ProcessSandboxExecutor`.
- `sandbox-core`'s `ProcessSandboxExecutor` spawns a real process inside the Android app (`sh -c echo …`).
- One real model turn through `infinity-provider-rig` + `infinity-agent-core` on-device, over HTTPS, with the API key supplied at runtime.
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

A model turn, with `chat-config.json` pushed at runtime (`{base_url, api_key,
model, prompt}`) and the reply written to `chat.json`:

```json
{ "model": "gemma-4-31b", "reply": "infinity-on-android-llm",
  "notes": [], "timed_out": false }
```

```bash
adb shell run-as run.hydro.infinityhybrid cat chat.json
```

## Android TLS: `rustls-platform-verifier`

`reqwest` 0.13's `rustls` feature (enabled by `rig-core`'s `reqwest-rustls`) always
routes certificate verification through `rustls-platform-verifier`. Its Android
backend calls into the JVM and needs two things this archetype does not ship:
`init_with_env`/`init_with_runtime` invoked with a JNI `Env` and the Android
`Context`, and the Kotlin `CertificateVerifier` AAR added to the Gradle build.

Without them the handshake **stalls** — no error, no log line, and no timeout of
its own; the runtime keeps retrying, so a chat probe looks like a hang. Reports:
[reqwest#2966](https://github.com/seanmonstar/reqwest/issues/2966),
[tauri#13267](https://github.com/tauri-apps/tauri/issues/13267).

The archetype sidesteps the JNI bridge: `portable_http_client()` builds a
`rustls::ClientConfig` from Mozilla roots compiled into the binary (`webpki-roots`),
hands it to reqwest with `use_preconfigured_tls`, and injects that client into rig
with `.http_client(...)`.

- **Pro:** pure Rust; no AAR step in a regenerated `gen/android` project; identical
  behaviour on desktop, Android and browsers (where no OS trust store exists).
- **Con:** the platform store is ignored, so user-installed and corporate CAs do
  not apply. Apps that need those must ship the AAR and call
  `rustls_platform_verifier::android::init_with_env()` before the first request.

`CompletionsClient<H>`'s `H` cannot be inferred through `.http_client()`, because
that method replaces the type parameter — pin it:
`CompletionsClient::<reqwest::Client>::builder()`.

## WhatsApp Web prototype

Opt-in: create a `whatsapp-probe` marker in the app data dir and relaunch. The
plugin overlays a WebView with a desktop user agent and writes `whatsapp.json`:

```json
{ "url": "https://web.whatsapp.com/", "title": "WhatsApp", "readyState": "complete",
  "qrCanvas": true, "canvasCount": 1, "serviceWorker": true, "indexedDb": true,
  "subtleCrypto": true }
```

The real client boots and renders its QR; `screencap` shows the genuine login
screen. **Pairing itself needs a human** to scan the QR, and WhatsApp's Help Center
names unofficial clients and automation as ban triggers, so the fork shows the
view and ships no message automation (decision D24). Delete the marker afterwards
— the overlay is full-screen.

## Running tools on Android

Android 10+ denies `exec` on files in the app's writable data directory (for apps
targeting API 29+), so `chmod +x ./tool && ./tool` fails with `EACCES`. Two paths
remain:

1. **Wasm modules (recommended).** Embed an interpreter and read `.wasm` from app
data — no `exec` permission is involved at all, and fuel metering comes for free.
Measured here on aarch64, release, stripped: 308 KB baseline → 1.44 MB with
`wasmi` (**+1.16 MB**). Rationale and comparison in
[`docs/docs/WASM/overview.md` §3.1](../../docs/docs/WASM/overview.md) and decision D23.
2. **Executables from `nativeLibraryDir`.** Name the binary `lib<name>.so`, put it
in `jniLibs/<abi>/`, set `android:extractNativeLibs="true"`, then execute
`"$nativeLibraryDir/lib<name>.so"`. The installer extracts it into a directory
labelled `apk_data_file`, which the app's own domain may execute. For real Rust
binaries add `patchelf --set-rpath '$ORIGIN'` and ship every `.so` dependency;
the payload grows per ABI.

`/system/bin/sh` needs none of this — it lives on the system partition, which is
executable — which is why the sandbox probe works out of the box.

## Native capabilities

The contacts and Shizuku commands come from the in-app plugin at
[`crates/tauri-plugin-infinity-android`](crates/tauri-plugin-infinity-android/README.md).
Notifications use the official `tauri-plugin-notification`. Everything is verified
by [`scripts/device-probe.sh`](scripts/device-probe.sh), which installs, grants
`READ_CONTACTS` + `POST_NOTIFICATIONS`, launches, and prints all probe files plus
the `dumpsys notification` record.

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
