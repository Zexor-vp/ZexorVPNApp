package site.zexorvpn.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import java.util.concurrent.CompletableFuture

/** Параметры TUN-интерфейса для движков со своей адресацией (AmneziaWG): адреса и маршруты как `(адрес, длина маски)`. */
class TunParams(
    val addresses: List<Pair<String, Int>>,
    val routes: List<Pair<String, Int>>,
    val dns: List<String>,
    val mtu: Int,
)

/** `10.0.0.2/32` → (`10.0.0.2`, 32); `null`, если строка не похожа на адрес с маской. */
fun parseCidr(value: String): Pair<String, Int>? {
    val parts = value.trim().split("/")
    if (parts.size != 2) return null
    val prefix = parts[1].trim().toIntOrNull() ?: return null
    return parts[0].trim() to prefix
}

/**
 * Системный VPN: создаёт TUN-интерфейс и держит его, пока работает туннель.
 *
 * Сам трафик обрабатывает xray — отдельный процесс приложения, которому через Rust передаётся копия
 * дескриптора TUN. Само приложение (а значит и xray, и запросы к кабинету) исключено из туннеля
 * (`addDisallowedApplication`), иначе исходящие соединения xray шли бы обратно в него же.
 */
class ZexorVpnService : VpnService() {

    companion object {
        const val ACTION_START = "site.zexorvpn.vpn.START"
        const val ACTION_STOP = "site.zexorvpn.vpn.STOP"
        private const val CHANNEL_ID = "zexor_vpn"
        private const val NOTIFICATION_ID = 7

        @Volatile
        var tun: ParcelFileDescriptor? = null

        /** Завершается, когда сервис создал (или не смог создать) TUN. */
        @Volatile
        var pending: CompletableFuture<Boolean>? = null

        @Volatile
        var running: Boolean = false

        /** Параметры интерфейса для следующего запуска; `null` — интерфейс для xray по умолчанию. */
        @Volatile
        var tunParams: TunParams? = null

        fun start(context: Context): CompletableFuture<Boolean> {
            val future = CompletableFuture<Boolean>()
            pending = future
            val intent = Intent(context, ZexorVpnService::class.java).setAction(ACTION_START)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
            return future
        }

        fun stop(context: Context) {
            context.startService(Intent(context, ZexorVpnService::class.java).setAction(ACTION_STOP))
        }
    }

    /** TUN, созданный именно этим экземпляром сервиса: при пересоздании чужой интерфейс закрывать нельзя. */
    private var ownTun: ParcelFileDescriptor? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            teardown()
            stopSelf()
            return START_NOT_STICKY
        }
        startInForeground()
        establish()
        return START_NOT_STICKY
    }

    private fun establish() {
        val future = pending
        try {
            teardown(keepForeground = true)
            val params = tunParams
            val builder = Builder().setSession("Zexor VPN").setMtu(params?.mtu ?: 1500)
            if (params == null) {
                builder
                    .addAddress("172.19.0.1", 30)
                    .addRoute("0.0.0.0", 0)
                    .addAddress("fd00:19::1", 126)
                    .addRoute("::", 0)
                    .addDnsServer("1.1.1.1")
                    .addDnsServer("8.8.8.8")
            } else {
                params.addresses.forEach { builder.addAddress(it.first, it.second) }
                params.routes.forEach { builder.addRoute(it.first, it.second) }
                val servers = params.dns.ifEmpty { listOf("1.1.1.1", "8.8.8.8") }
                servers.forEach { builder.addDnsServer(it) }
            }
            // Сам xray и запросы приложения идут мимо туннеля.
            builder.addDisallowedApplication(packageName)
            val descriptor = builder.establish()
            ownTun = descriptor
            tun = descriptor
            running = descriptor != null
            QuickSurfaces.refresh(this)
            future?.complete(descriptor != null)
        } catch (error: Exception) {
            running = false
            future?.complete(false)
        }
    }

    private fun startInForeground() {
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Zexor VPN", NotificationManager.IMPORTANCE_LOW),
            )
        }
        val open = packageManager.getLaunchIntentForPackage(packageName)
        val pending = open?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE)
        }
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        val notification = builder
            .setContentTitle("Zexor VPN")
            .setContentText("VPN подключён")
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .setOngoing(true)
            .apply { if (pending != null) setContentIntent(pending) }
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun teardown(keepForeground: Boolean = false) {
        // Быстрое «выключить — включить» пересоздаёт сервис: старый экземпляр не должен закрыть новый TUN.
        val mine = ownTun
        try {
            mine?.close()
        } catch (_: Exception) {
        }
        ownTun = null
        if (mine != null && tun === mine) {
            tun = null
            running = false
        }
        QuickSurfaces.refresh(this)
        if (!keepForeground) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                stopForeground(STOP_FOREGROUND_REMOVE)
            } else {
                @Suppress("DEPRECATION")
                stopForeground(true)
            }
        }
    }

    /** Пользователь отозвал VPN в системных настройках или его занял другой VPN. */
    override fun onRevoke() {
        teardown()
        stopSelf()
    }

    override fun onDestroy() {
        teardown()
        super.onDestroy()
    }
}
