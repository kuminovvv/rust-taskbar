# Сборка rust-taskbar на Windows 11

Актуально на 18 ноября 2026 года. Основная ветка — `main`.

## Необходимые инструменты

1. Rust stable с целевой платформой `x86_64-pc-windows-msvc`.
2. Visual Studio 2022 Build Tools **с набором компонентов** Desktop development with C++; отдельная установка оболочки Build Tools не включает автоматически `link.exe`.
3. MSVC v143 C++ x64/x86 tools и Windows 10/11 SDK.

Не требуются полная Visual Studio, Electron, Tauri и установка системной панели. Проверять наличие компилятора удобно через `rustup show active-toolchain`, а наличие компонентов Visual Studio — через `vswhere.exe`.

## Диагностика

Открой новый PowerShell:

```powershell
cd C:\project\rust-taskbar
git pull --ff-only origin main
pwsh -NoProfile -File .\scripts\check-toolchain.ps1
```

Скрипт **ничего не устанавливает**, проверяет наличие Rust, MSVC x64 linker и SDK.

Если `link.exe not found`:

1. Запусти **Visual Studio Installer** из меню «Пуск».
2. Выбери **Build Tools 2022 → Modify (Изменить)**.
3. Включи **Desktop development with C++** и проверь компоненты **MSVC v143 x64/x86** и **Windows 10/11 SDK**.
4. Заверши изменение установки и открой **Developer PowerShell for VS 2022** (либо x64 Native Tools Command Prompt).
5. Повтори диагностику, затем сборку.

Просто `winget install --id Microsoft.VisualStudio.2022.BuildTools -e` может установить только установщик и базовые инструменты: наличие C++ workload нужно проверить отдельно.

Документация Microsoft: [MSVC command line environment](https://learn.microsoft.com/en-us/cpp/build/building-on-the-command-line?view=msvc-170), [Build Tools workloads and component IDs](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022).

## Сборка и запуск

```powershell
git pull --ff-only origin main
cargo test --all-targets
cargo run --release
```

Приложение — только прототип плавающей панели рядом со штатным Explorer. Выход через кнопку `Exit`; автозагрузка, замена shell и изменение системных файлов не используются. 

## Фактический статус проверок

- Пользовательская сборка 18.11.2026: Rust скачал и скомпилировал зависимости, исходные ошибки E0425/E0308/E0282 исправлены коммитом `987a921`.
- Следующая попытка сборки остановилась на **`linker link.exe not found`**. Это ошибка конфигурации MSVC, а не подтверждение успешной сборки exe.
- `cargo test`, `cargo clippy`, `cargo build --release` на Windows, реальный запуск GUI и измерения 180 Гц **ещё не подтверждены**.
- GitHub Actions предусмотрен в `.github/workflows/ci.yml`; без результатов завершённого запуска нельзя объявлять CI успешным.

## Полезные команды

```powershell
rustc --version
cargo --version
rustup show active-toolchain
Get-Command link.exe -ErrorAction SilentlyContinue
```

В обычном PowerShell `Get-Command link.exe` может не находить линкер даже при установленном MSVC: набор переменных среды подготавливается командным окружением Developer PowerShell, включая PATH/LIB/INCLUDE.
