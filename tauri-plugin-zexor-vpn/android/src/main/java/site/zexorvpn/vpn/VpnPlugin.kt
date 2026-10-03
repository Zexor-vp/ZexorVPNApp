package site.zexorvpn.vpn

import android.app.Activity
import android.net.VpnService
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.util.concurrent.TimeUnit

@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {

    /** geoip.dat / geosite.dat лежат в ресурсах APK; xray читает их с диска, поэтому копируем в папку приложения. */
    override fun load(webView: WebView) {
        try {
            val dir = File(activity.filesDir, "geo").apply { mkdirs() }
            for (name in listOf("geoip.dat", "geosite.dat")) {
                val target = File(dir, name)
                val size = activity.assets.openFd("geo/$name").use { it.length }
                if (!target.exists() || target.length() != size) {
                    activity.assets.open("geo/$name").use { input ->
                        target.outputStream().use { input.copyTo(it) }
                    }
                }
            }
        } catch (_: Exception) {
            // Без баз geosite/geoip часть правил не заработает, но VPN поднимется.
        }
    }

    @Command
    fun info(invoke: Invoke) {
        val result = JSObject()
        result.put("libDir", activity.applicationInfo.nativeLibraryDir)
        result.put("filesDir", activity.filesDir.absolutePath)
        result.put("running", ZexorVpnService.running)
        invoke.resolve(result)
    }

    @Command
    fun prepare(invoke: Invoke) {
        val intent = VpnService.prepare(activity)
        if (intent == null) {
            invoke.resolve(JSObject().put("granted", true))
        } else {
            startActivityForResult(invoke, intent, "prepareResult")
        }
    }

    @ActivityCallback
    fun prepareResult(invoke: Invoke, result: ActivityResult) {
        invoke.resolve(JSObject().put("granted", result.resultCode == Activity.RESULT_OK))
    }

    @Command
    fun establish(invoke: Invoke) {
        Thread {
            try {
                val ok = ZexorVpnService.start(activity).get(15, TimeUnit.SECONDS)
                val descriptor = ZexorVpnService.tun
                if (!ok || descriptor == null) {
                    invoke.reject("Не удалось создать VPN-интерфейс")
                    return@Thread
                }
                // Rust получает собственную копию дескриптора и закроет её сам; оригинал остаётся у сервиса.
                val copy = descriptor.dup().detachFd()
                invoke.resolve(JSObject().put("fd", copy))
            } catch (error: Exception) {
                invoke.reject("Не удалось создать VPN-интерфейс: ${error.message}")
            }
        }.start()
    }

    @Command
    fun stop(invoke: Invoke) {
        ZexorVpnService.stop(activity)
        invoke.resolve()
    }
}
