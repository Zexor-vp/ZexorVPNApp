package site.zexorvpn.vpn

import android.app.Activity
import android.os.Bundle
import android.os.Handler
import android.os.Looper

/**
 * Невидимая прослойка для плитки и виджета.
 *
 * Если приложение уже работает в фоне (например, VPN включён), переключение передаётся Rust-части без открытия окна.
 * Прозрачное окно нужно, чтобы система считала приложение «на экране» и разрешила запустить VPN-сервис. Если Rust не
 * ответил за пару секунд или приложение не запущено совсем — открываем его как раньше.
 */
class QuickToggleActivity : Activity() {
    private val handler = Handler(Looper.getMainLooper())
    private var finished = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        overridePendingTransition(0, 0)

        if (!QuickSurfaces.rustReady) {
            openApp()
            return
        }
        QuickSurfaces.done = false
        QuickSurfaces.pending = QuickSurfaces.ACTION_TOGGLE

        // Rust опрашивает действие примерно раз в секунду: не забрал за 2,5 с — значит, он не отвечает.
        handler.postDelayed({
            if (QuickSurfaces.pending != null) {
                QuickSurfaces.pending = null
                openApp()
            }
        }, 2500)
        // Дальше ждём, пока Rust закончит (подключение может занять несколько секунд).
        handler.postDelayed(object : Runnable {
            private var waited = 0L
            override fun run() {
                if (finished) return
                waited += 200
                if (QuickSurfaces.done || waited > 25000) finishQuietly() else handler.postDelayed(this, 200)
            }
        }, 200)
    }

    private fun openApp() {
        QuickSurfaces.launchIntent(this)?.let { startActivity(it) }
        finishQuietly()
    }

    private fun finishQuietly() {
        if (finished) return
        finished = true
        finish()
        overridePendingTransition(0, 0)
    }

    override fun onDestroy() {
        finished = true
        handler.removeCallbacksAndMessages(null)
        super.onDestroy()
    }
}
