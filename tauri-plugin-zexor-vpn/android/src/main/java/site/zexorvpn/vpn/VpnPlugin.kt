package site.zexorvpn.vpn

import android.app.Activity
import android.net.VpnService
import android.webkit.WebView
import androidx.activity.result.ActivityResult
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
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
        passInsetsToPage(webView)
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

    /**
     * Приложение рисуется под системными панелями (edge-to-edge), а WebView не всегда сообщает странице их размер
     * через env(safe-area-inset-*). Передаём отступы сами — в CSS-переменных --sai-top/-bottom/-left/-right (в px).
     */
    private fun passInsetsToPage(webView: WebView) {
        var script = ""
        val push = { webView.evaluateJavascript(script, null) }
        ViewCompat.setOnApplyWindowInsetsListener(webView) { _, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
            )
            val density = webView.resources.displayMetrics.density
            fun px(value: Int) = "${value / density}px"
            script = "var r=document.documentElement.style;" +
                "r.setProperty('--sai-top','${px(bars.top)}');" +
                "r.setProperty('--sai-bottom','${px(bars.bottom)}');" +
                "r.setProperty('--sai-left','${px(bars.left)}');" +
                "r.setProperty('--sai-right','${px(bars.right)}');"
            webView.post(push)
            insets
        }
        // Страница может загрузиться позже первого события — повторяем несколько секунд.
        for (delay in listOf(500L, 1500L, 3000L, 6000L)) {
            webView.postDelayed({ if (script.isNotEmpty()) push() }, delay)
        }
        ViewCompat.requestApplyInsets(webView)
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
