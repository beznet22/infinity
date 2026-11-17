package com.plugin.infinity_android

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.SystemClock
import android.provider.ContactsContract
import android.view.MotionEvent
import android.view.ViewGroup
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import org.json.JSONTokener

/** Shizuku's provider authority; also its package name. */
private const val SHIZUKU_PACKAGE = "moe.shizuku.privileged.api"

/** web.whatsapp.com gates WebView user agents; a desktop UA gets the real client. */
private const val DESKTOP_USER_AGENT =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) " +
        "Chrome/120.0.0.0 Safari/537.36"

private const val WHATSAPP_URL = "https://web.whatsapp.com"

/** No arguments: the parameterless commands. */
@InvokeArg
class EmptyArgs

/** Phone number in E.164 form, e.g. `+4915112345678`. */
@InvokeArg
class PairArgs {
    var phone: String? = null
}

/** A chat name to open, and how many recent messages to return. */
@InvokeArg
class ReadArgs {
    var chat: String? = null
    var limit: Int? = null
}

/** Message text; `dryRun` leaves it in the composer instead of sending. */
@InvokeArg
class SendArgs {
    var chat: String? = null
    var text: String? = null
    var dryRun: Boolean = false
}

@TauriPlugin(
    permissions = [
        Permission(
            alias = "contacts",
            strings = [Manifest.permission.READ_CONTACTS]
        )
    ]
)
class InfinityAndroidPlugin(private val activity: Activity) : Plugin(activity) {

    private var whatsAppView: WebView? = null

    /** Runs once the WhatsApp page finishes loading; the overlay is created lazily. */
    private var pendingWhatsAppAction: ((WebView) -> Unit)? = null

    @Command
    fun countContacts(invoke: Invoke) {
        if (!hasContactsPermission()) {
            requestPermissionForAlias("contacts", invoke, "onContactsPermission")
            return
        }
        resolveContactCount(invoke)
    }

    /**
     * Runs after the system prompt. Resolves with the count, or rejects when the
     * user declined, so callers can distinguish "zero contacts" from "no access".
     */
    @PermissionCallback
    fun onContactsPermission(invoke: Invoke) {
        if (!hasContactsPermission()) {
            invoke.reject("READ_CONTACTS was denied")
            return
        }
        resolveContactCount(invoke)
    }

    /** Start the foreground service that keeps a running turn alive. */
    @Command
    fun startForegroundService(invoke: Invoke) {
        try {
            val intent = android.content.Intent(activity, InfinityForegroundService::class.java)
            androidx.core.content.ContextCompat.startForegroundService(activity, intent)
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject("could not start the foreground service: ${error.message}")
        }
    }

    /** Stop it once the turn is finished. */
    @Command
    fun stopForegroundService(invoke: Invoke) {
        try {
            activity.stopService(android.content.Intent(activity, InfinityForegroundService::class.java))
            invoke.resolve()
        } catch (error: Exception) {
            invoke.reject("could not stop the foreground service: ${error.message}")
        }
    }

    @Command
    fun shizukuStatus(invoke: Invoke) {
        val result = JSObject()
        result.put("installed", isShizukuInstalled())
        result.put("running", isShizukuRunning())
        result.put("sdkInt", Build.VERSION.SDK_INT)
        invoke.resolve(result)
    }

    /**
     * Loads web.whatsapp.com in an overlay WebView and reports what the client
     * needs in order to boot (QR canvas, service worker, IndexedDB, WebCrypto),
     * then switches the login screen to the "link with phone number" flow.
     */
    @Command
    fun whatsappWebProbe(invoke: Invoke) {
        activity.runOnUiThread {
            try {
                openWhatsApp { view -> reportWhatsAppState(view, invoke) }
            } catch (error: Exception) {
                invoke.reject("whatsapp probe failed: ${error.message}")
            }
        }
    }

    /**
     * Submits a phone number to the linking form and returns the 8-character
     * pairing code the user must type into WhatsApp on this phone.
     *
     * Pairing itself cannot be automated: WhatsApp on the phone approves the
     * link. See decision D25 — this whole path is unofficial and can get the
     * account banned.
     */
    @Command
    fun whatsappPair(invoke: Invoke) {
        val args = invoke.parseArgs(PairArgs::class.java)
        val phone = args.phone?.trim().orEmpty()
        if (phone.isEmpty()) {
            invoke.reject("whatsappPair needs a phone number in E.164 form")
            return
        }
        activity.runOnUiThread {
            try {
                openWhatsApp { view -> runPairSteps(view, phone, invoke) }
            } catch (error: Exception) {
                invoke.reject("whatsapp pairing failed: ${error.message}")
            }
        }
    }

    /**
     * Reports whether the linked session is ready. Polls for up to ~25 s because
     * a freshly linked device spends that long downloading history.
     */
    @Command
    fun whatsappState(invoke: Invoke) {
        activity.runOnUiThread {
            try {
                openWhatsApp { view -> pollWhatsAppState(view, invoke, 0) }
            } catch (error: Exception) {
                invoke.reject("whatsapp state failed: ${error.message}")
            }
        }
    }

    /**
     * Opens a chat by name and returns its most recent messages.
     *
     * The chat list ignores synthetic `click()` calls, so rows are tapped with
     * real `MotionEvent`s at coordinates computed in the page.
     */
    @Command
    fun whatsappRead(invoke: Invoke) {
        val args = invoke.parseArgs(ReadArgs::class.java)
        val chat = args.chat?.trim().orEmpty()
        val limit = (args.limit ?: 20).coerceIn(1, 100)
        activity.runOnUiThread {
            openWhatsApp { view ->
                tapFirstChat(view, chat) {
                    // The conversation panel renders after the row tap; retry
                    // until messages exist instead of reading an empty pane.
                    view.postDelayed({ readMessages(view, invoke, limit, 0) }, 2_500)
                }
            }
        }
    }

    /** Opens a chat and types a message; `dryRun` leaves it in the composer. */
    @Command
    fun whatsappSend(invoke: Invoke) {
        val args = invoke.parseArgs(SendArgs::class.java)
        val chat = args.chat?.trim().orEmpty()
        val text = args.text?.trim().orEmpty()
        if (chat.isEmpty() || text.isEmpty()) {
            invoke.reject("whatsappSend needs a chat name and text")
            return
        }
        activity.runOnUiThread {
            openWhatsApp { view ->
                tapFirstChat(view, chat) {
                    view.postDelayed({
                        view.evaluateJavascript(COMPOSER_RECT_JS) { raw ->
                            val rect = readJson(raw)
                            if (!rect.has("x")) {
                                invoke.reject("no message composer on screen")
                                return@evaluateJavascript
                            }
                            tap(view, rect.optDouble("x").toFloat(), rect.optDouble("y").toFloat())
                            view.postDelayed({
                                val type = TYPE_JS.replace("\$TEXT", JSONObject.quote(text))
                                view.evaluateJavascript(type) { typedRaw ->
                                    val typed = readJson(typedRaw).optInt("chars", 0)
                                    if (args.dryRun) {
                                        invoke.resolve(
                                            jsonObjectOf(
                                                "sent" to false,
                                                "drafted" to true,
                                                "chars" to typed
                                            )
                                        )
                                        return@evaluateJavascript
                                    }
                                    view.postDelayed({
                                        view.evaluateJavascript(SEND_RECT_JS) { sendRaw ->
                                            val sendRect = readJson(sendRaw)
                                            if (!sendRect.has("x")) {
                                                invoke.resolve(
                                                    jsonObjectOf(
                                                        "sent" to false,
                                                        "drafted" to true,
                                                        "reason" to "no send button"
                                                    )
                                                )
                                                return@evaluateJavascript
                                            }
                                            tap(
                                                view,
                                                sendRect.optDouble("x").toFloat(),
                                                sendRect.optDouble("y").toFloat()
                                            )
                                            invoke.resolve(
                                                jsonObjectOf(
                                                    "sent" to true,
                                                    "drafted" to false,
                                                    "chars" to typed
                                                )
                                            )
                                        }
                                    }, 400)
                                }
                            }, 400)
                        }
                    }, 1_500)
                }
            }
        }
    }

    /** Reads the open conversation, retrying while the panel is still empty. */
    private fun readMessages(view: WebView, invoke: Invoke, limit: Int, attempt: Int) {
        view.evaluateJavascript(READ_JS.replace("\$LIMIT", limit.toString())) { raw ->
            val parsed = readJson(raw)
            if (parsed.optInt("count", 0) < 2 && attempt < 8) {
                view.postDelayed({ readMessages(view, invoke, limit, attempt + 1) }, 700)
                return@evaluateJavascript
            }
            resolveJson(invoke, raw)
        }
    }

    /** Searches (when a name is given) and taps the first conversation row. */
    private fun tapFirstChat(view: WebView, chat: String, then: () -> Unit) {
        val tapRow = {
            view.evaluateJavascript(ROW_RECT_JS) { raw ->
                val rect = readJson(raw)
                if (rect.has("x")) {
                    tap(view, rect.optDouble("x").toFloat(), rect.optDouble("y").toFloat())
                    view.postDelayed(then, 1_500)
                }
            }
        }
        if (chat.isEmpty()) {
            tapRow()
            return
        }
        view.evaluateJavascript(SEARCH_JS.replace("\$NAME", JSONObject.quote(chat))) {
            view.postDelayed(tapRow, 1_500)
        }
    }

    /**
     * Dispatches a real touch at a CSS-pixel coordinate. `evaluateJavascript`
     * cannot produce trusted input, which WhatsApp's list rows require.
     */
    private fun tap(view: WebView, cssX: Float, cssY: Float) {
        val density = view.resources.displayMetrics.density
        val x = cssX * density
        val y = cssY * density
        val downTime = SystemClock.uptimeMillis()
        val down = MotionEvent.obtain(downTime, downTime, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(downTime, downTime + 40, MotionEvent.ACTION_UP, x, y, 0)
        view.dispatchTouchEvent(down)
        view.dispatchTouchEvent(up)
        down.recycle()
        up.recycle()
    }

    private fun jsonObjectOf(vararg pairs: Pair<String, Any>): JSObject {
        val obj = JSObject()
        for ((key, value) in pairs) {
            obj.put(key, value)
        }
        return obj
    }

    /** Re-checks the page until the chat list renders, or the budget runs out. */
    private fun pollWhatsAppState(view: WebView, invoke: Invoke, attempt: Int) {
        view.evaluateJavascript(STATE_READY_JS) { raw ->
            val state = readJson(raw)
            val ready = state.optBoolean("chatList", false)
            if (ready || attempt >= 12) {
                resolveJson(invoke, raw)
                return@evaluateJavascript
            }
            view.postDelayed({ pollWhatsAppState(view, invoke, attempt + 1) }, 2_000)
        }
    }

    /** Creates the overlay once and queues [action] until the page has loaded. */
    private fun openWhatsApp(action: (WebView) -> Unit) {
        val existing = whatsAppView
        if (existing != null) {
            existing.postDelayed({ action(existing) }, 500)
            return
        }
        pendingWhatsAppAction = action
        createWhatsAppView().loadUrl(WHATSAPP_URL)
    }

    private fun createWhatsAppView(): WebView {
        val webView = WebView(activity)
        webView.settings.javaScriptEnabled = true
        webView.settings.domStorageEnabled = true
        webView.settings.databaseEnabled = true
        webView.settings.userAgentString = DESKTOP_USER_AGENT
        webView.settings.mediaPlaybackRequiresUserGesture = false
        // The desktop user agent gets the real client, which lays itself out for a
        // desktop window (980 CSS px wide) and clips inside its own overflow-hidden
        // containers. Use the wide layout viewport and let WebView zoom the whole
        // page down to fit instead; pinch-zoom stays available for reading.
        webView.settings.useWideViewPort = true
        webView.settings.loadWithOverviewMode = true
        webView.settings.textZoom = 100
        webView.settings.setSupportZoom(true)
        webView.settings.builtInZoomControls = true
        webView.settings.displayZoomControls = false
        webView.webViewClient = object : WebViewClient() {
            override fun onPageFinished(view: WebView?, url: String?) {
                // Desktop layouts ignore device width unless told otherwise.
                view?.evaluateJavascript(VIEWPORT_JS, null)
                val action = pendingWhatsAppAction
                pendingWhatsAppAction = null
                if (action != null && view != null) {
                    view.postDelayed({ action(view) }, 4_000)
                }
            }
        }
        // Hidden on purpose: automation reads the DOM, so the desktop layout never
        // has to fit a phone screen. It is inserted at index 0 at full size, so
        // Tauri's own webview covers it while Chromium still sees a visible,
        // normally-sized viewport. A 1x1 or detached view gets throttled, and a
        // zero-width viewport makes WhatsApp's virtualized lists render nothing.
        val root = activity.findViewById<ViewGroup>(android.R.id.content)
        root.addView(
            webView,
            0,
            ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        )
        whatsAppView = webView
        return webView
    }

    /** Clicks the phone-number link, then reports the resulting screen. */
    private fun reportWhatsAppState(view: WebView, invoke: Invoke) {
        view.evaluateJavascript(CLICK_PHONE_LINK_JS) { clickRaw ->
            val clicked = readJson(clickRaw).optBoolean("clicked", false)
            view.postDelayed(
                { collectWhatsAppState(view, invoke, clicked) },
                if (clicked) 2_500 else 500
            )
        }
    }

    private fun collectWhatsAppState(view: WebView, invoke: Invoke, phoneLinkClicked: Boolean) {
        val script = STATE_JS.replace("\$CLICKED", phoneLinkClicked.toString())
        view.evaluateJavascript(script) { raw ->
            resolveJson(invoke, raw)
        }
    }

    /** Login → phone entry → submit → pairing code, with a diagnostic at each step. */
    private fun runPairSteps(view: WebView, phone: String, invoke: Invoke) {
        view.evaluateJavascript(CLICK_PHONE_LINK_JS) {
            view.postDelayed({
                val fill = FILL_PHONE_JS.replace("\$PHONE", JSONObject.quote(phone))
                view.evaluateJavascript(fill) {
                    view.postDelayed({
                        view.evaluateJavascript(CLICK_NEXT_JS) {
                            view.postDelayed({
                                view.evaluateJavascript(PAIR_STATE_JS) { raw ->
                                    resolveJson(invoke, raw)
                                }
                            }, 4_000)
                        }
                    }, 800)
                }
            }, 2_000)
        }
    }

    private fun resolveJson(invoke: Invoke, raw: String?) {
        try {
            val parsed = readJson(raw)
            val result = JSObject()
            for (key in parsed.keys()) {
                result.put(key, parsed.get(key))
            }
            invoke.resolve(result)
        } catch (error: Exception) {
            invoke.reject("could not read the whatsapp page state: ${error.message}")
        }
    }

    /** `evaluateJavascript` returns a JSON-encoded string; unwrap it to an object. */
    private fun readJson(raw: String?): JSONObject = runCatching {
        val decoded = JSONTokener(raw ?: "null").nextValue() as? String ?: "{}"
        JSONObject(decoded)
    }.getOrDefault(JSONObject())

    private fun hasContactsPermission(): Boolean =
        ContextCompat.checkSelfPermission(activity, Manifest.permission.READ_CONTACTS) ==
            PackageManager.PERMISSION_GRANTED

    private fun resolveContactCount(invoke: Invoke) {
        val projection = arrayOf(ContactsContract.Contacts._ID)
        val cursor = runCatching {
            activity.contentResolver.query(
                ContactsContract.Contacts.CONTENT_URI,
                projection,
                null,
                null,
                null
            )
        }.getOrNull()

        if (cursor == null) {
            invoke.reject("the contacts provider returned no cursor")
            return
        }

        val count = cursor.use { it.count }
        val result = JSObject()
        result.put("count", count)
        result.put("granted", true)
        invoke.resolve(result)
    }

    private fun isShizukuInstalled(): Boolean = runCatching {
        activity.packageManager.getPackageInfo(SHIZUKU_PACKAGE, 0)
        true
    }.getOrDefault(false)

    /**
     * Shizuku exposes a ContentProvider used to hand out its binder. A null reply
     * means the privileged server is not running; any failure (missing provider,
     * permission denied) counts as "not running" rather than as an error.
     */
    private fun isShizukuRunning(): Boolean = runCatching {
        activity.contentResolver.call(
            Uri.parse("content://$SHIZUKU_PACKAGE"),
            "ping",
            null,
            null
        ) != null
    }.getOrDefault(false)

    private companion object {
        /**
         * Clicks the "link with phone number instead" control.
         *
         * Only real controls are considered first (`a`, `button`, `[role=button]`):
         * matching every `<div>` hits a layout wrapper, and clicking that does
         * nothing because the handler sits on the anchor inside it.
         */
        val CLICK_PHONE_LINK_JS = """
            (function () {
              try {
                function matches(el) {
                  var text = (el.innerText || el.textContent || '').trim();
                  return /link with phone number/i.test(text) && text.length < 60;
                }
                var candidates = document.querySelectorAll(
                  'a, button, [role="button"], [role="link"]'
                );
                var target = null;
                for (var i = 0; i < candidates.length; i++) {
                  if (matches(candidates[i])) { target = candidates[i]; break; }
                }
                var found = !!target;
                if (!target) {
                  var all = document.querySelectorAll('*');
                  for (var j = 0; j < all.length; j++) {
                    if (matches(all[j])) { target = all[j]; }
                  }
                }
                if (!target) { return JSON.stringify({ clicked: false, found: false }); }
                target.click();
                return JSON.stringify({
                  clicked: true,
                  found: found,
                  tag: target.tagName,
                  role: target.getAttribute('role'),
                  text: (target.innerText || '').slice(0, 60)
                });
              } catch (e) {
                return JSON.stringify({ clicked: false, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Fills the phone input the way React expects, not by assigning `.value`. */
        val FILL_PHONE_JS = """
            (function () {
              try {
                var inputs = document.querySelectorAll('input');
                var input = null;
                for (var i = 0; i < inputs.length; i++) {
                  if (inputs[i].offsetParent !== null) { input = inputs[i]; break; }
                }
                if (!input && inputs.length > 0) { input = inputs[0]; }
                if (!input) { return JSON.stringify({ filled: false, reason: 'no input' }); }
                var setter = Object.getOwnPropertyDescriptor(
                  window.HTMLInputElement.prototype, 'value'
                ).set;
                setter.call(input, ${'$'}PHONE);
                input.dispatchEvent(new Event('input', { bubbles: true }));
                input.dispatchEvent(new Event('change', { bubbles: true }));
                return JSON.stringify({ filled: true, inputs: document.querySelectorAll('input').length });
              } catch (e) {
                return JSON.stringify({ filled: false, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Clicks the form's primary action (Next / Continue). */
        val CLICK_NEXT_JS = """
            (function () {
              try {
                var nodes = document.querySelectorAll('button,div[role="button"],a,span');
                for (var i = 0; i < nodes.length; i++) {
                  var text = (nodes[i].innerText || '').trim();
                  if (/^(next|continue)$/i.test(text)) {
                    var clickable = nodes[i];
                    while (clickable && clickable !== document.body) {
                      if (clickable.tagName === 'BUTTON' || clickable.tagName === 'A' ||
                          clickable.getAttribute('role') === 'button') {
                        break;
                      }
                      clickable = clickable.parentElement;
                    }
                    (clickable || nodes[i]).click();
                    return JSON.stringify({ clicked: true, text: text });
                  }
                }
                return JSON.stringify({ clicked: false });
              } catch (e) {
                return JSON.stringify({ clicked: false, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Reads the 8-character pairing code, or the screen that is in the way. */
        val PAIR_STATE_JS = """
            (function () {
              try {
                var body = document.body ? document.body.innerText : '';
                // The client renders each code character in its own element, so the
                // dashes arrive separated by newlines: strip whitespace first.
                var stripped = body.replace(/\s+/g, '');
                var code = stripped.match(/[A-Z0-9]{4}-[A-Z0-9]{4}/);
                var inputs = document.querySelectorAll('input');
                var visible = 0;
                var details = [];
                for (var i = 0; i < inputs.length; i++) {
                  if (inputs[i].offsetParent !== null) { visible++; }
                  details.push((inputs[i].getAttribute('aria-label') || '') + '|' +
                               (inputs[i].getAttribute('placeholder') || '') + '|' +
                               (inputs[i].type || ''));
                }
                var linkPresent = false;
                var nodes = document.querySelectorAll('a, button, [role="button"], [role="link"]');
                for (var k = 0; k < nodes.length; k++) {
                  var nodeText = (nodes[k].innerText || '').trim();
                  if (/link with phone number/i.test(nodeText)) { linkPresent = true; break; }
                }
                var stage = 'unknown';
                if (code) { stage = 'pairing_code'; }
                else if (visible > 0) { stage = 'phone_entry'; }
                else if (document.querySelectorAll('canvas').length > 0) { stage = 'login'; }
                return JSON.stringify({
                  stage: stage,
                  pairingCode: code ? code[0] : null,
                  phoneInputs: inputs.length,
                  visibleInputs: visible,
                  inputDetails: details,
                  phoneLinkPresent: linkPresent,
                  bodySnippet: body.slice(0, 400)
                });
              } catch (e) {
                return JSON.stringify({ stage: 'error', error: String(e) });
              }
            })()
        """.trimIndent()

        /** Reports the layout metrics used to sanity-check that the page fits. */
        val VIEWPORT_JS = """
            (function () {
              try {
                return JSON.stringify({
                  innerWidth: window.innerWidth,
                  visualScale: window.visualViewport ? window.visualViewport.scale : null
                });
              } catch (e) { return 'error'; }
            })()
        """.trimIndent()

        /** Types a name into the chat search box. */
        val SEARCH_JS = """
            (function () {
              try {
                var box = document.querySelector('[aria-label="Search input textbox"]')
                       || document.querySelector('#side div[contenteditable="true"]');
                if (!box) { return JSON.stringify({ ok: false, reason: 'no search box' }); }
                box.focus();
                document.execCommand('insertText', false, ${'$'}NAME);
                return JSON.stringify({ ok: true });
              } catch (e) {
                return JSON.stringify({ ok: false, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Centre of the first conversation row, in CSS pixels. */
        val ROW_RECT_JS = """
            (function () {
              try {
                var row = document.querySelector('#pane-side [role="row"]')
                       || document.querySelector('[data-testid="cell-frame-container"]');
                if (!row) { return JSON.stringify({ reason: 'no chat row' }); }
                var r = row.getBoundingClientRect();
                return JSON.stringify({ x: r.x + r.width / 2, y: r.y + r.height / 2 });
              } catch (e) {
                return JSON.stringify({ reason: String(e) });
              }
            })()
        """.trimIndent()

        /** Centre of the message composer. */
        val COMPOSER_RECT_JS = """
            (function () {
              try {
                var box = document.querySelector('#main div[contenteditable="true"]');
                if (!box) { return JSON.stringify({ reason: 'no composer' }); }
                var r = box.getBoundingClientRect();
                return JSON.stringify({ x: r.x + r.width / 2, y: r.y + r.height / 2 });
              } catch (e) {
                return JSON.stringify({ reason: String(e) });
              }
            })()
        """.trimIndent()

        /** Types into whatever is focused (the composer, after the tap). */
        val TYPE_JS = """
            (function () {
              try {
                document.execCommand('insertText', false, ${'$'}TEXT);
                var box = document.querySelector('#main div[contenteditable="true"]');
                return JSON.stringify({ chars: box ? (box.innerText || '').length : 0 });
              } catch (e) {
                return JSON.stringify({ chars: 0, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Centre of the send button, once text is present. */
        val SEND_RECT_JS = """
            (function () {
              try {
                var button = document.querySelector('button[aria-label="Send"]');
                if (!button) {
                  var icon = document.querySelector('[data-icon="send"]');
                  button = icon ? icon.closest('button, div[role="button"]') : null;
                }
                if (!button) { return JSON.stringify({ reason: 'no send button' }); }
                var r = button.getBoundingClientRect();
                return JSON.stringify({ x: r.x + r.width / 2, y: r.y + r.height / 2 });
              } catch (e) {
                return JSON.stringify({ reason: String(e) });
              }
            })()
        """.trimIndent()

        /** Recent messages of the open conversation. */
        val READ_JS = """
            (function () {
              try {
                // Messages carry their own id, not a row role: `role="row"` in
                // #main only matches empty layout rows.
                var rows = document.querySelectorAll('#main [data-testid^="conv-msg-"]');
                if (rows.length === 0) {
                  rows = document.querySelectorAll('#main [data-testid="msg-container"]');
                }
                if (rows.length === 0) {
                  rows = document.querySelectorAll('#main [role="row"]');
                }
                var start = Math.max(0, rows.length - ${'$'}LIMIT);
                var messages = [];
                for (var i = start; i < rows.length; i++) {
                  messages.push((rows[i].innerText || '').replace(/\s+/g, ' ').trim());
                }
                var header = document.querySelector('#main header');
                return JSON.stringify({
                  count: rows.length,
                  messages: messages,
                  chatHeader: header ? (header.innerText || '').replace(/\s+/g, ' ').slice(0, 80) : null
                });
              } catch (e) {
                return JSON.stringify({ count: 0, messages: [], error: String(e) });
              }
            })()
        """.trimIndent()

        /** Typing (and optionally sending) a message. */
        val SEND_JS = """
            (function () {
              try {
                var box = document.querySelector('#main div[contenteditable="true"]');
                if (!box) { return JSON.stringify({ sent: false, reason: 'no composer' }); }
                box.focus();
                document.execCommand('insertText', false, ${'$'}TEXT);
                var typed = (box.innerText || '').length;
                if (${'$'}DRYRUN) {
                  return JSON.stringify({ sent: false, drafted: true, chars: typed });
                }
                var button = document.querySelector('button[aria-label="Send"]');
                if (!button) {
                  var icon = document.querySelector('[data-icon="send"]');
                  button = icon ? icon.closest('button, div[role="button"]') : null;
                }
                if (!button) {
                  return JSON.stringify({ sent: false, drafted: true, reason: 'no send button' });
                }
                button.click();
                return JSON.stringify({ sent: true, chars: typed });
              } catch (e) {
                return JSON.stringify({ sent: false, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Minimal readiness check used by the polling loop. */
        val STATE_READY_JS = """
            (function () {
              try {
                var chatList = !!(
                  document.querySelector('#pane-side') ||
                  document.querySelector('[aria-label="Chat list"]')
                );
                return JSON.stringify({
                  stage: chatList ? 'chat_list' : 'loading',
                  chatList: chatList,
                  chatRows: document.querySelectorAll('#pane-side [role="row"]').length,
                  bodySnippet: document.body.innerText.replace(/\s+/g, ' ').slice(0, 200)
                });
              } catch (e) {
                return JSON.stringify({ stage: 'error', chatList: false, error: String(e) });
              }
            })()
        """.trimIndent()

        /** Everything the probe reports about the loaded client. */
        val STATE_JS = """
            (function () {
              try {
                var inputs = document.querySelectorAll('input');
                var types = [];
                for (var i = 0; i < inputs.length; i++) { types.push(inputs[i].type || 'text'); }
                var body = document.body ? document.body.innerText : '';
                var stripped = body.replace(/\s+/g, '');
                var code = stripped.match(/[A-Z0-9]{4}-[A-Z0-9]{4}/);
                var chatList = !!(
                  document.querySelector('#pane-side') ||
                  document.querySelector('[aria-label="Chat list"]')
                );
                var stage = 'login';
                if (chatList) { stage = 'chat_list'; }
                else if (code) { stage = 'pairing_code'; }
                else if (inputs.length > 0) { stage = 'phone_entry'; }
                else if (code) { stage = 'pairing_code'; }
                return JSON.stringify({
                  url: location.href,
                  title: document.title,
                  readyState: document.readyState,
                  stage: stage,
                  chatList: chatList,
                  phoneLinkClicked: ${'$'}CLICKED,
                  phoneInputs: inputs.length,
                  inputTypes: types,
                  pairingCode: code ? code[0] : null,
                  qrCanvas: document.querySelectorAll('canvas').length > 0,
                  canvasCount: document.querySelectorAll('canvas').length,
                  serviceWorker: !!navigator.serviceWorker,
                  indexedDb: !!window.indexedDB,
                  subtleCrypto: !!(window.crypto && window.crypto.subtle),
                  innerWidth: window.innerWidth,
                  clientWidth: document.documentElement.clientWidth,
                  scrollWidth: document.documentElement.scrollWidth,
                  devicePixelRatio: window.devicePixelRatio,
                  visualScale: window.visualViewport ? window.visualViewport.scale : null,
                  bodySnippet: body.slice(0, 400)
                });
              } catch (e) {
                return JSON.stringify({ error: String(e) });
              }
            })()
        """.trimIndent()
    }
}
