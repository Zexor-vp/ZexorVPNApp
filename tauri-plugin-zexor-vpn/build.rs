// Команды плагина вызываются только из Rust-кода приложения (не из JS), поэтому для них не нужны
// разрешения в capabilities; список нужен сборке, чтобы подключить Android-модуль.
const COMMANDS: &[&str] = &["prepare", "establish", "stop", "info"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
