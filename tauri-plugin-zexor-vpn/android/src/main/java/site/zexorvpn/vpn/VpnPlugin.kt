package site.zexorvpn.vpn

import android.app.Activity
import android.content.Intent
import android.app.ActivityManager
import android.app.ApplicationExitInfo
import android.os.Build
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
        recordPreviousExit()
        installCrashLogger()
        takeQuickAction(activity.intent)
        QuickSurfaces.rustReady = true
        val dir = File(activity.filesDir, "geo").apply { mkdirs() }
        // Базы в APK сжаты, поэтому `openFd` для них падает; читаем обычным `open`. Перекопируем, если файла нет
        // или он старше установленной версии приложения (после обновления базы могли смениться).
        val installedAt = try {
            activity.packageManager.getPackageInfo(activity.packageName, 0).lastUpdateTime
        } catch (_: Exception) {
            0L
        }
        for (name in listOf("geoip.dat", "geosite.dat")) {
            try {
                val target = File(dir, name)
                if (target.length() > 0 && target.lastModified() >= installedAt) continue
                val temp = File(dir, "$name.tmp")
                activity.assets.open("geo/$name").use { input ->
                    temp.outputStream().use { input.copyTo(it) }
                }
                if (!temp.renameTo(target)) throw java.io.IOException("не удалось сохранить $name")
            } catch (e: Exception) {
                // Без баз geosite/geoip xray не запустится — оставляем след в logcat, а приложение расскажет об ошибке.
                android.util.Log.e("ZexorVpn", "не удалось подготовить $name", e)
            }
        }
    }

    /** Приложение уже запущено, а плитка/виджет нажаты снова: окно приходит как новый Intent. */
    override fun onNewIntent(intent: Intent) {
        takeQuickAction(intent)
    }

    private fun takeQuickAction(intent: Intent?) {
        val action = intent?.getStringExtra(QuickSurfaces.EXTRA) ?: return
        intent.removeExtra(QuickSurfaces.EXTRA)
        QuickSurfaces.pending = action
    }

    /** Действие с плитки/виджета, которое ещё не выполнено (пустая строка — нет). Забирается один раз. */
    @Command
    fun quickaction(invoke: Invoke) {
        val action = QuickSurfaces.pending
        QuickSurfaces.pending = null
        invoke.resolve(JSObject().put("action", action ?: ""))
    }

    /** Убирает окно приложения обратно в фон (после «тихого» включения с плитки/виджета). */
    @Command
    fun background(invoke: Invoke) {
        QuickSurfaces.done = true
        try {
            activity.runOnUiThread { activity.moveTaskToBack(true) }
        } catch (_: Exception) {
            // Окна приложения уже нет (процесс жил в фоне) — убирать нечего.
        }
        invoke.resolve()
    }

    /** Короткое системное сообщение: ошибка быстрого включения, когда окна приложения нет на экране. */
    @Command
    fun toast(invoke: Invoke) {
        val text = invoke.getString("text") ?: ""
        QuickSurfaces.done = true
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            android.widget.Toast.makeText(activity.applicationContext, text, android.widget.Toast.LENGTH_LONG).show()
        }
        invoke.resolve()
    }

    private val reportFile get() = File(activity.filesDir, "last_crash.txt")

    /**
     * Android помнит, почему завершился прошлый процесс приложения (краш, нехватка памяти, убит системой...).
     * Сохраняем это в файл — по нему видно, почему приложение «вылетело», когда логов под рукой нет.
     */
    private fun recordPreviousExit() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        try {
            val manager = activity.getSystemService(Activity.ACTIVITY_SERVICE) as ActivityManager
            val last = manager.getHistoricalProcessExitReasons(activity.packageName, 0, 1).firstOrNull() ?: return
            val abnormal = when (last.reason) {
                ApplicationExitInfo.REASON_EXIT_SELF,
                ApplicationExitInfo.REASON_USER_REQUESTED,
                ApplicationExitInfo.REASON_USER_STOPPED,
                ApplicationExitInfo.REASON_PERMISSION_CHANGE,
                -> false
                else -> true
            }
            if (!abnormal) return
            val name = when (last.reason) {
                ApplicationExitInfo.REASON_CRASH -> "ошибка в Java/Kotlin"
                ApplicationExitInfo.REASON_CRASH_NATIVE -> "падение нативного кода"
                ApplicationExitInfo.REASON_LOW_MEMORY -> "системе не хватило памяти"
                ApplicationExitInfo.REASON_SIGNALED -> "процесс убит сигналом ${last.status}"
                ApplicationExitInfo.REASON_ANR -> "приложение не отвечало"
                ApplicationExitInfo.REASON_EXCESSIVE_RESOURCE_USAGE -> "чрезмерное потребление ресурсов"
                ApplicationExitInfo.REASON_INITIALIZATION_FAILURE -> "ошибка запуска"
                else -> "причина ${last.reason}"
            }
            val ageMinutes = (System.currentTimeMillis() - last.timestamp) / 60000
            if (ageMinutes > 24 * 60) return
            val description = last.description?.takeIf { it.isNotBlank() }?.let { ": $it" } ?: ""
            reportFile.appendText("Прошлый запуск завершился ($ageMinutes мин назад): $name$description\n")
        } catch (_: Exception) {
        }
    }

    /** Необработанная ошибка в потоках приложения: пишем причину в тот же файл и отдаём системе как обычно. */
    private fun installCrashLogger() {
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            try {
                reportFile.appendText("Ошибка в потоке ${thread.name}: ${error.stackTraceToString().take(1500)}\n")
            } catch (_: Exception) {
            }
            previous?.uncaughtException(thread, error)
        }
    }

    /** Текст отчёта о прошлом сбое (и очищает его, чтобы не показывать повторно). */
    @Command
    fun crashes(invoke: Invoke) {
        val result = JSObject()
        try {
            val text = if (reportFile.exists()) reportFile.readText().takeLast(2500) else ""
            if (text.isNotEmpty()) reportFile.delete()
            result.put("text", text)
        } catch (_: Exception) {
            result.put("text", "")
        }
        invoke.resolve(result)
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

    /**
     * Постоянный идентификатор устройства для панели (лимит устройств): производный от ANDROID_ID, который не меняется
     * при переустановке, пока приложение подписано тем же ключом. Формат UUID — как у остальных платформ.
     */
    private fun stableDeviceId(): String = try {
        val androidId = android.provider.Settings.Secure.getString(activity.contentResolver, android.provider.Settings.Secure.ANDROID_ID)
        if (androidId.isNullOrBlank()) "" else java.util.UUID.nameUUIDFromBytes("zexor:$androidId".toByteArray()).toString()
    } catch (_: Exception) {
        ""
    }

    @Command
    fun info(invoke: Invoke) {
        val result = JSObject()
        result.put("libDir", activity.applicationInfo.nativeLibraryDir)
        result.put("filesDir", activity.filesDir.absolutePath)
        result.put("running", ZexorVpnService.running)
        result.put("deviceId", stableDeviceId())
        result.put("model", "${Build.MANUFACTURER} ${Build.MODEL}")
        result.put("osVersion", "Android ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})")
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
                val ok = ZexorVpnService.start(activity.applicationContext).get(15, TimeUnit.SECONDS)
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
        try {
            ZexorVpnService.stop(activity)
        } catch (_: Exception) {
            // Сервис уже остановлен или приложение в фоне — останавливать нечего.
        }
        invoke.resolve()
    }
}
