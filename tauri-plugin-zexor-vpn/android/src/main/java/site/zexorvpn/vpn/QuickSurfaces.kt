package site.zexorvpn.vpn

import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.service.quicksettings.TileService

/**
 * «Быстрое включение» без захода в приложение: плитка в шторке и виджет на рабочем столе.
 *
 * Подключением занимается Rust-часть приложения, а она живёт внутри процесса приложения — поэтому нажатие
 * запускает приложение с пометкой [EXTRA]; плагин передаёт её Rust, тот включает или выключает VPN и просит
 * убрать окно обратно в фон.
 */
object QuickSurfaces {
    const val EXTRA = "zexor_quick"
    const val ACTION_TOGGLE = "toggle"

    /** Действие, пришедшее с плитки/виджета и ещё не забранное Rust-частью. */
    @Volatile
    var pending: String? = null

    fun launchIntent(context: Context): Intent? =
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
            putExtra(EXTRA, ACTION_TOGGLE)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

    /** Перерисовать плитку и виджеты (вызывается, когда VPN включился или выключился). */
    fun refresh(context: Context) {
        try {
            val appContext = context.applicationContext
            TileService.requestListeningState(appContext, ComponentName(appContext, ZexorTileService::class.java))
            val manager = AppWidgetManager.getInstance(appContext)
            val ids = manager.getAppWidgetIds(ComponentName(appContext, ZexorWidgetProvider::class.java))
            if (ids.isNotEmpty()) ZexorWidgetProvider.render(appContext, manager, ids)
        } catch (_: Exception) {
            // Плитка/виджет — удобство, а не основная функция: их сбой не должен мешать VPN.
        }
    }
}
