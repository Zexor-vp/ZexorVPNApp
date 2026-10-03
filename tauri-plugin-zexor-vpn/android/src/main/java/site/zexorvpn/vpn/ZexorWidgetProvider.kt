package site.zexorvpn.vpn

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.widget.RemoteViews

/** Небольшой виджет рабочего стола: статус VPN и кнопка включения/выключения. */
class ZexorWidgetProvider : AppWidgetProvider() {

    override fun onUpdate(context: Context, manager: AppWidgetManager, appWidgetIds: IntArray) {
        render(context, manager, appWidgetIds)
    }

    companion object {
        fun render(context: Context, manager: AppWidgetManager, ids: IntArray) {
            val running = ZexorVpnService.running
            val views = RemoteViews(context.packageName, R.layout.zexor_widget)
            views.setTextViewText(R.id.widget_state, context.getString(if (running) R.string.state_on else R.string.state_off))
            views.setInt(
                R.id.widget_root,
                "setBackgroundResource",
                if (running) R.drawable.widget_bg_on else R.drawable.widget_bg_off,
            )
            QuickSurfaces.launchIntent(context)?.let { intent ->
                val pending = PendingIntent.getActivity(
                    context,
                    1,
                    intent,
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
                views.setOnClickPendingIntent(R.id.widget_root, pending)
            }
            manager.updateAppWidget(ids, views)
        }
    }
}
