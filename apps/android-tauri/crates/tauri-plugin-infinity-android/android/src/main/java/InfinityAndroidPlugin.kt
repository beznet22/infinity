package com.plugin.infinity_android

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.ContactsContract
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** Shizuku's provider authority; also its package name. */
private const val SHIZUKU_PACKAGE = "moe.shizuku.privileged.api"

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
