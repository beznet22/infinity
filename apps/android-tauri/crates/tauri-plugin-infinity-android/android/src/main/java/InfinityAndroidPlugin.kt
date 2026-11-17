package com.plugin.infinity_android

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.ContactsContract
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

/** No arguments: both commands are parameterless probes. */
@InvokeArg
class EmptyArgs

@TauriPlugin(
    permissions = [
        Permission(
            alias = "contacts",
            strings = [Manifest.permission.READ_CONTACTS]
        )
    ]
)
class InfinityAndroidPlugin(private val activity: Activity) : Plugin(activity) {

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
     * actually needs (canvas QR, service worker, IndexedDB, WebCrypto).
     *
     * Prototype only: pairing needs a human to scan the QR with the phone, so
     * this can verify that the client boots, never that a session works. Using
     * an unofficial client or automating it violates WhatsApp's terms and can
     * get the account banned, so nothing here sends messages.
     */
    @Command
    fun whatsappWebProbe(invoke: Invoke) {
        activity.runOnUiThread {
            try {
                val webView = WebView(activity)
                webView.settings.javaScriptEnabled = true
                webView.settings.domStorageEnabled = true
                webView.settings.databaseEnabled = true
                webView.settings.userAgentString = DESKTOP_USER_AGENT
                webView.settings.mediaPlaybackRequiresUserGesture = false
                webView.webViewClient = object : WebViewClient() {
                    override fun onPageFinished(view: WebView?, url: String?) {
                        view?.postDelayed({ reportWhatsAppState(view, invoke) }, 4_000)
                    }
                }
                val root = activity.findViewById<ViewGroup>(android.R.id.content)
                root.addView(
                    webView,
                    ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.MATCH_PARENT
                    )
                )
                webView.loadUrl("https://web.whatsapp.com")
            } catch (error: Exception) {
                invoke.reject("whatsapp probe failed: ${error.message}")
            }
        }
    }

    private fun reportWhatsAppState(view: WebView, invoke: Invoke) {
        val script = """
            (function () {
              try {
                var canvas = document.querySelectorAll('canvas');
                return JSON.stringify({
                  url: location.href,
                  title: document.title,
                  readyState: document.readyState,
                  qrCanvas: canvas.length > 0,
                  canvasCount: canvas.length,
                  serviceWorker: !!navigator.serviceWorker,
                  indexedDb: !!window.indexedDB,
                  subtleCrypto: !!(window.crypto && window.crypto.subtle),
                  bodySnippet: (document.body ? document.body.innerText : '').slice(0, 300)
                });
              } catch (e) {
                return JSON.stringify({ error: String(e) });
              }
            })()
        """.trimIndent()

        view.evaluateJavascript(script) { raw ->
            try {
                val decoded = JSONTokener(raw).nextValue() as? String ?: "{}"
                val parsed = JSONObject(decoded)
                val result = JSObject()
                for (key in parsed.keys()) {
                    result.put(key, parsed.get(key))
                }
                invoke.resolve(result)
            } catch (error: Exception) {
                invoke.reject("could not read the whatsapp page state: ${error.message}")
            }
        }
    }

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
}
