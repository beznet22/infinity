# infinity-android

A small Android add-on for the Infinity Hybrid app.

The app is a web page inside a native shell. A web page cannot read the phone's
contacts, and it cannot ask whether Shizuku is running. This plugin adds exactly
those two abilities — nothing else.

## What it adds

| Command | What it does | What the user sees |
| --- | --- | --- |
| `count_contacts` | Counts the contacts the app is allowed to read | The first call shows Android's "allow access to contacts?" prompt |
| `shizuku_status` | Reports whether Shizuku is installed and whether its server is running | Nothing — it only reports |
| `whatsapp_web_probe` | Loads `web.whatsapp.com` and reports whether the real client booted (QR canvas, service worker, IndexedDB, WebCrypto) | Nothing — the view is hidden behind the app |
| `whatsapp_pair` | Submits a phone number to the linking form and returns the 8-character pairing code | The user types that code into WhatsApp on the phone |
| `whatsapp_state` | Polls until the chat list renders; reports `stage`, `chatList`, row count | Nothing |
| `whatsapp_read` | Opens a chat by name and returns its most recent messages | The chat is marked read, as in a browser |
| `whatsapp_send` | Types a message; `dry_run` (default) leaves it for the user to send | A drafted message, or a sent one |

`whatsapp_web_probe` is a **prototype**: linking needs a human to scan the QR,
and WhatsApp's terms forbid unofficial clients and automation, so it never sends a
message. It only runs when a `whatsapp-probe` marker file exists in the app data
directory, which keeps the overlay out of normal launches.

The WhatsApp commands drive an **unofficial client** over the DOM. Two things
about them are not obvious:

- **Clicks must be trusted.** `element.click()` does nothing on chat rows; the
  plugin dispatches real `MotionEvent`s at `CSS px × density` coordinates.
- **Never restart during linking.** Rebuilding or force-stopping the app while
  the device is linking cancels the handshake. Verify over CDP instead.

See decision D26 in `docs/docs/WASM/decisions.md` for the selectors and measured
behaviour, and D24/D25 for the terms-of-service risk the user accepted.

## Two halves, one plugin

- **Kotlin half** (`android/src/main/java/InfinityAndroidPlugin.kt`) — the real
  Android code. It queries `ContactsContract` and pings Shizuku's
  ContentProvider. Two methods are marked `@Command`.
- **Rust half** (`src/`) — what the app calls, like any other Tauri command. It
  forwards each call to the Kotlin method over Tauri's mobile bridge.

`build.rs` tells Tauri where the Android project lives, so the app's Gradle build
picks it up automatically (see the generated `gen/android/tauri.settings.gradle`).

On desktop the same commands exist but return an `unsupported` error, so a
desktop build never pretends to have contacts.

## What it deliberately does not do

- **No root.** Everything goes through normal Android APIs.
- **No dependency on Shizuku.** It only reports what it finds; `installed: false`
  is the normal outcome on a phone that has never had Shizuku.
- **No background work.** Commands run only when called.
- **No place in the fork's library crates.** It lives under the app because only
  this app can use it; the root Rust workspace excludes `apps/` (decision D16).

## Permissions

- `READ_CONTACTS` — declared in `android/src/main/AndroidManifest.xml`, requested
  at runtime by `count_contacts`. Denial is reported as an error, so callers can
  tell "no contacts" apart from "no access".

Nothing else is requested. Notifications are handled by the official
`tauri-plugin-notification`, not by this plugin.

## Checking it without touching the screen

```bash
adb shell pm grant run.hydro.infinityhybrid android.permission.READ_CONTACTS
../../scripts/device-probe.sh <serial>
```

The app writes `native.json` into its data directory, readable with
`adb shell run-as run.hydro.infinityhybrid cat native.json`:

```json
{
  "contacts": { "count": 1064, "granted": true },
  "shizuku": { "installed": false, "running": false, "sdkInt": 33 },
  "notification": "shown"
}
```

## Adding another Android capability

1. Add a `@Command` method to `InfinityAndroidPlugin.kt`.
2. Add the same name to `COMMANDS` in `build.rs` (this generates its permission).
3. Add a method in `src/mobile.rs`, a matching stub in `src/desktop.rs`, and a
   `#[command]` in `src/commands.rs`.
4. Register it in `src/lib.rs`, and allow it in `permissions/default.toml`.

If the capability needs an Android permission, declare it in the plugin's
manifest and request it at runtime instead of assuming it was granted.
