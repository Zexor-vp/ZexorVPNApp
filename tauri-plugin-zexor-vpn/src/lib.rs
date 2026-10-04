//! Плагин «системный VPN Android» для Zexor VPN.
//!
//! Kotlin-часть (`android/`) показывает системный запрос разрешения на VPN, создаёт TUN-интерфейс через
//! `VpnService` и отдаёт Rust-части его дескриптор. xray запускается как дочерний процесс приложения с этим
//! дескриптором (`XRAY_TUN_FD`) и сам читает/пишет сетевые пакеты. На остальных платформах плагин пуст.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "android")]
mod mobile;
#[cfg(target_os = "android")]
pub use mobile::{Established, Info, TunParams, Vpn, VpnError};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("zexor-vpn")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin("site.zexorvpn.vpn", "VpnPlugin")?;
                _app.manage(mobile::Vpn(handle));
            }
            Ok(())
        })
        .build()
}
