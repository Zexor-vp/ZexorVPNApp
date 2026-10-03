package site.zexorvpn.vpn

import android.app.PendingIntent
import android.graphics.drawable.Icon
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService

/** Плитка «Zexor VPN» в шторке быстрых настроек. */
class ZexorTileService : TileService() {

    override fun onStartListening() {
        update()
    }

    override fun onClick() {
        val intent = QuickSurfaces.trampolineIntent(this)
        val open = {
            if (Build.VERSION.SDK_INT >= 34) {
                startActivityAndCollapse(
                    PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT),
                )
            } else {
                @Suppress("DEPRECATION")
                startActivityAndCollapse(intent)
            }
        }
        if (isLocked) unlockAndRun { open() } else open()
    }

    private fun update() {
        val tile = qsTile ?: return
        val running = ZexorVpnService.running
        tile.state = if (running) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
        tile.label = getString(R.string.tile_label)
        tile.icon = Icon.createWithResource(this, R.drawable.ic_quick_power)
        if (Build.VERSION.SDK_INT >= 29) {
            tile.subtitle = getString(if (running) R.string.state_on else R.string.state_off)
        }
        tile.updateTile()
    }
}
