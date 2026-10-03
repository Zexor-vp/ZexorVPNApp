package site.zexorvpn.vpn

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.os.Bundle
import android.widget.RemoteViews

/**
 * Виджет рабочего стола: статус VPN и кнопка включения/выключения. Размер меняется и вширь, и вверх — от одной клетки
 * (только кнопка) до большой карточки; содержимое подбирается под текущий размер.
 */
class ZexorWidgetProvider : AppWidgetProvider() {

    override fun onUpdate(context: Context, manager: AppWidgetManager, appWidgetIds: IntArray) {
        render(context, manager, appWidgetIds)
    }

    /** Пользователь растянул или сжал виджет — перерисовываем под новый размер. */
    override fun onAppWidgetOptionsChanged(context: Context, manager: AppWidgetManager, appWidgetId: Int, newOptions: Bundle) {
        render(context, manager, intArrayOf(appWidgetId))
    }

    companion object {
        private const val COMPACT_MAX_WIDTH_DP = 100
        private const val LARGE_MIN_WIDTH_DP = 110
        private const val LARGE_MIN_HEIGHT_DP = 110

        fun render(context: Context, manager: AppWidgetManager, ids: IntArray) {
            val running = ZexorVpnService.running
            val click = PendingIntent.getActivity(
                context,
                1,
                QuickSurfaces.trampolineIntent(context),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            for (id in ids) {
                val options = manager.getAppWidgetOptions(id)
                val width = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 110)
                val height = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, 40)
                val compact = width < COMPACT_MAX_WIDTH_DP
                val large = !compact && width >= LARGE_MIN_WIDTH_DP && height >= LARGE_MIN_HEIGHT_DP
                val layout = when {
                    compact -> R.layout.zexor_widget_compact
                    large -> R.layout.zexor_widget_large
                    else -> R.layout.zexor_widget
                }
                val views = RemoteViews(context.packageName, layout)
                if (!compact) {
                    views.setTextViewText(R.id.widget_state, context.getString(if (running) R.string.state_on else R.string.state_off))
                }
                views.setInt(
                    R.id.widget_root,
                    "setBackgroundResource",
                    if (running) R.drawable.widget_bg_on else R.drawable.widget_bg_off,
                )
                views.setOnClickPendingIntent(R.id.widget_root, click)
                manager.updateAppWidget(id, views)
            }
        }
    }
}
