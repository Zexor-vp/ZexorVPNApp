# Zexor VPN Desktop (Windows)

Десктоп-клиент Zexor VPN: подключение через xray-core (VLESS+REALITY), локальный
блокировщик рекламы и автообновление.

## Структура

```
core/        Платформонезависимое ядро — тестируется на любой ОС
  xray/      Разбор подписки, сборка конфига xray, запуск и надзор за процессом
  proxy/     Системный прокси Windows: снапшот → применение → восстановление
  adblock/   Разбор блок-листов (hosts / AdBlock Plus / plain)
  auth/      Логика ротации токенов кабинета
src-tauri/   GUI-слой: команды Tauri, состояние, трей
src/         Фронтенд (React + Vite)
```

Системная логика намеренно вынесена в `core/` без Tauri-зависимостей — так её
можно прогонять тестами на Linux, не имея под рукой Windows.

## Разработка

```bash
cargo test -p zexor-vpn-core                                    # тесты ядра
cargo check -p zexor-vpn-core --target x86_64-pc-windows-gnu    # проверка Windows-кода
npm install && npm run tauri dev                                # запуск приложения (только на Windows)
```

Собрать `.exe` можно **только на Windows** — Tauri требует Windows SDK и WebView2.
На Linux доступны разработка и тесты ядра; релизные сборки делает CI.

## Релиз

Сборку выполняет `.github/workflows/build.yml` на windows-раннере: тянет
xray-core нужной версии, собирает фронтенд, пакует NSIS-инсталлятор, подписывает
его ключом обновлений и прикладывает к GitHub Release.

```bash
git tag v0.1.0 && git push origin v0.1.0   # тег запускает релизную сборку
```

### Что нужно настроить один раз

1. **Ключи подписи обновлений.** Без них `tauri-plugin-updater` не примет релиз:

   ```bash
   npm run tauri signer generate -- -w tauri-signing-key
   ```

   - приватный ключ (`tauri-signing-key`) → в GitHub Secrets как `TAURI_SIGNING_PRIVATE_KEY`;
   - пароль от него → `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`;
   - публичный ключ (`tauri-signing-key.pub`) → в `src-tauri/tauri.conf.json`,
     поле `plugins.updater.pubkey` (сейчас там плейсхолдер).

   Приватный ключ в git не попадает — он в `.gitignore`. Потеря ключа означает, что
   существующие установки перестанут обновляться, поэтому держите копию в
   надёжном месте.

2. **Адрес манифеста обновлений** уже настроен на релизы этого репозитория:
   `https://github.com/Zexor-vp/ZexorVPNApp/releases/latest/download/latest.json`.
   Репозиторий публичный, поэтому отдельный хостинг для обновлений не нужен —
   клиенты тянут манифест и инсталлятор прямо с GitHub.

3. **Подпись кода.** Сборка пока не подписана сертификатом, поэтому при установке
   Windows SmartScreen покажет предупреждение «Windows protected your PC». Это
   ожидаемо и лечится только покупкой сертификата (OV ~$300–500/год либо Azure
   Trusted Signing).

## Версия xray-core

Закреплена в `env.XRAY_VERSION` внутри workflow. Обновляется осознанно, чтобы
сборки оставались воспроизводимыми.
