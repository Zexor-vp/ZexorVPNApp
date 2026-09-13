//! Запуск и надзор за дочерним процессом xray-core.
//!
//! Главная гарантия: `xray.exe` не должен пережить приложение ни при каком
//! сценарии — ни при штатном выходе, ни при «Снять задачу» в диспетчере. Для
//! этого процесс помещается в Windows Job Object с флагом
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: когда handle джоба закрывается (в том
//! числе при аварийном завершении родителя), ядро само убивает всё, что внутри.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum XrayError {
    #[error("бинарник xray не найден по пути {0}")]
    BinaryNotFound(PathBuf),
    #[error("не удалось записать конфиг: {0}")]
    ConfigWrite(String),
    #[error("не удалось запустить xray: {0}")]
    Spawn(String),
    #[error("xray завершился сразу после старта (код {0:?}); проверьте конфиг")]
    ExitedImmediately(Option<i32>),
}

/// Дескриптор запущенного xray. Пока структура жива — процесс работает;
/// `Drop` гарантированно его гасит.
pub struct XrayProcess {
    child: Child,
    config_path: PathBuf,
    #[cfg(windows)]
    _job: job::JobObject,
}

impl XrayProcess {
    /// Пишет конфиг на диск и поднимает процесс.
    ///
    /// Путь конфига передаётся явно, а не берётся из окружения: так функция
    /// остаётся чистой и тестируемой (глобальный `LOCALAPPDATA` иначе течёт
    /// между параллельными тестами). Продакшен-путь даёт [`config_file_path`].
    pub fn start(
        binary: &Path,
        config: &serde_json::Value,
        config_path: &Path,
    ) -> Result<Self, XrayError> {
        if !binary.exists() {
            return Err(XrayError::BinaryNotFound(binary.to_path_buf()));
        }

        let config_path = config_path.to_path_buf();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| XrayError::ConfigWrite(e.to_string()))?;
        }
        let serialized = serde_json::to_string_pretty(config)
            .map_err(|e| XrayError::ConfigWrite(e.to_string()))?;
        std::fs::write(&config_path, serialized)
            .map_err(|e| XrayError::ConfigWrite(e.to_string()))?;

        let mut command = Command::new(binary);
        command
            .arg("run")
            .arg("-c")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command.spawn().map_err(|e| XrayError::Spawn(e.to_string()))?;

        #[cfg(windows)]
        let job = {
            let job = job::JobObject::new().map_err(XrayError::Spawn)?;
            job.assign(&child).map_err(XrayError::Spawn)?;
            job
        };

        let mut process = Self {
            child,
            config_path,
            #[cfg(windows)]
            _job: job,
        };

        // Конфиг с опечаткой роняет xray мгновенно — ловим это сразу, а не
        // показываем пользователю «подключено», когда туннеля нет.
        std::thread::sleep(std::time::Duration::from_millis(400));
        if let Some(status) = process.try_exit_status() {
            return Err(XrayError::ExitedImmediately(status));
        }

        Ok(process)
    }

    /// `None` — процесс ещё жив.
    pub fn try_exit_status(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            Ok(None) => None,
            Err(_) => Some(None),
        }
    }

    pub fn is_running(&mut self) -> bool {
        self.try_exit_status().is_none()
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Останавливает процесс. Идемпотентно.
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for XrayProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `%LOCALAPPDATA%\ZexorVPN\xray\config.json`
pub fn config_file_path() -> PathBuf {
    config_file_path_in(&app_data_dir())
}

/// Та же раскладка, но от произвольной базы — чтобы не зависеть от окружения в тестах.
pub fn config_file_path_in(base: &Path) -> PathBuf {
    base.join("ZexorVPN").join("xray").join("config.json")
}

/// Каталог данных приложения: `%LOCALAPPDATA%`, а если его нет — временный.
pub fn app_data_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Обёртка над Job Object: пока handle открыт — процессы внутри живут,
    /// закрылся (в том числе из-за смерти родителя) — ядро их убивает.
    pub struct JobObject(HANDLE);

    impl JobObject {
        pub fn new() -> Result<Self, String> {
            unsafe {
                let handle = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;

                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .map_err(|e| e.to_string())?;

                Ok(Self(handle))
            }
        }

        pub fn assign(&self, child: &Child) -> Result<(), String> {
            unsafe {
                AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle() as _))
                    .map_err(|e| e.to_string())
            }
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    // Handle можно безопасно передавать между потоками.
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `XrayProcess` намеренно не `Debug` (внутри raw handle джоба), поэтому
    /// ошибку достаём разбором результата, а не `unwrap_err()`.
    fn expect_err(result: Result<XrayProcess, XrayError>) -> XrayError {
        match result {
            Err(e) => e,
            Ok(_) => panic!("ожидалась ошибка, процесс неожиданно запустился"),
        }
    }

    /// Уникальный путь на каждый тест — тесты идут параллельно.
    fn temp_config_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("zexor-cfg-{}-{tag}", std::process::id()))
            .join("config.json")
    }

    #[test]
    fn reports_missing_binary_clearly() {
        let missing = PathBuf::from("/nonexistent/xray-binary");
        let err = expect_err(XrayProcess::start(
            &missing,
            &serde_json::json!({}),
            &temp_config_path("missing"),
        ));
        assert!(matches!(err, XrayError::BinaryNotFound(_)));
    }

    #[test]
    fn production_config_path_lives_under_app_data() {
        let path = config_file_path_in(Path::new("/base"));
        assert_eq!(path, Path::new("/base/ZexorVPN/xray/config.json"));
    }

    /// Скрипт-заглушка вместо xray: принимает любые аргументы, ведёт себя как
    /// указано в теле. Позволяет проверить весь путь `start()` целиком.
    #[cfg(unix)]
    fn fake_xray(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zexor-xray-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Процесс, который сразу завершается, должен диагностироваться как
    /// сломанный конфиг, а не выдаваться за успешное подключение.
    #[cfg(unix)]
    #[test]
    fn detects_process_that_exits_immediately() {
        let binary = fake_xray("dies.sh", "exit 23");

        let err = expect_err(XrayProcess::start(
            &binary,
            &serde_json::json!({"log": {}}),
            &temp_config_path("dies"),
        ));

        match err {
            XrayError::ExitedImmediately(code) => assert_eq!(code, Some(23)),
            other => panic!("ожидался ExitedImmediately, получено {other:?}"),
        }
    }

    /// Долгоживущий процесс считается запущенным, конфиг оказывается на диске,
    /// а `stop()` процесс гасит.
    #[cfg(unix)]
    #[test]
    fn writes_config_tracks_and_stops_running_process() {
        let binary = fake_xray("lives.sh", "sleep 30");
        let config = serde_json::json!({"log": {"loglevel": "warning"}});

        let mut process =
            XrayProcess::start(&binary, &config, &temp_config_path("lives")).unwrap();

        assert!(process.is_running());
        let written = std::fs::read_to_string(process.config_path()).unwrap();
        assert!(written.contains("\"loglevel\": \"warning\""), "{written}");

        process.stop();
        assert!(!process.is_running());
    }
}
