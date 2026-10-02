//! Права администратора: режиму TUN нужно создать сетевой адаптер, обычному пользователю это запрещено.

/// Запущено ли приложение с правами администратора. Вне Windows — всегда `true` (там режим TUN не используется).
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        windows_impl::is_elevated()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Запускает через UAC новый экземпляр приложения (с небольшой задержкой — чтобы текущий успел выйти и
/// `single-instance` не принял новый за дубликат). Текущий процесс после успеха нужно завершить самому.
pub fn relaunch_elevated(exe: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows_impl::relaunch_elevated(exe)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err("повышение прав поддерживается только в Windows".to_string())
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn is_elevated() -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned = 0u32;
            let result = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            );
            let _ = CloseHandle(token);
            result.is_ok() && elevation.TokenIsElevated != 0
        }
    }

    pub fn relaunch_elevated(exe: &Path) -> Result<(), String> {
        // `cmd` с правами администратора ждёт пару секунд и запускает приложение; запущенное им наследует права.
        let params = format!(
            "/c timeout /t 2 /nobreak >nul & start \"\" \"{}\"",
            exe.display()
        );
        let verb = wide("runas".as_ref());
        let file = wide("cmd.exe".as_ref());
        let params = wide(params.as_ref());
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(file.as_ptr()),
                PCWSTR(params.as_ptr()),
                PCWSTR::null(),
                SW_HIDE,
            )
        };
        // По документации успех — значение больше 32.
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err("запрос прав администратора отклонён".to_string())
        }
    }
}
