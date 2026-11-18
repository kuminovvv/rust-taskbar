# Этапы и критерии завершения

## M0 — Discovery / PoC (до разработки визуального продукта)

- [x] Инвентаризация функций стандартной Windows 11 taskbar.
- [x] P0/P1/P2 приоритизация.
- [ ] Исследовать интеграцию с системным треем и `ITaskbarList3`: какие данные реально доступны без private API.
- [ ] Определить baseline Windows сборки и target hardware; получить эталонные замеры штатной панели.
- [ ] DirectComposition vs Windows.UI.Composition: PoC кадров, нагрузки и recovery после device loss.
- [ ] Согласовать, нужна ли именно **полная замена**, или минимальный Dock с системным трей в штатном Explorer.

Выход: документ с фактами испытаний и решением о границах продукта.

## M1 — Native MVP, P0

- [ ] Нативное окно appbar с корректным рабочим пространством.
- [ ] Окна, активное состояние, запуск, сворачивание, закрытие.
- [ ] Закреплённые приложения и сохранение порядка.
- [ ] Время, дата, вызов системных Start/Search.
- [ ] Нет постоянного render loop при отсутствии взаимодействия.
- [ ] Нормальная работа DPI/fullscreen, простая настройка и безопасное отключение.
- [ ] Базовый набор автоматических и ручных тестов.

## M2 — UX/performance

- [ ] Composition анимации scale/opacity/translate с привязкой к refresh rate, включая 180 Гц.
- [ ] Замеры frametime, latency, RAM private bytes/working set, CPU/GPU idle, cold start.
- [ ] CPU/GPU profiling и устранение регрессий.
- [ ] Accessibility, keyboard navigation, light/dark, reduced motion.

## M3 — Everyday replacement, P1

- [ ] После PoC: решение по трею — совместимость / fallback / официально неполная замена.
- [ ] Превью окон, системные индикаторы, меню приложений, Quick Settings и Notification Center.
- [ ] Multi-monitor, auto-hide, overflow, виртуальные рабочие столы.
- [ ] Инсталлятор и откат, без отключения Explorer.

## M4 — Дополнительные возможности, P2

- [ ] Jump Lists в полном объёме, progress, overlays, thumbnail commands — только если подтверждена техническая возможность.
- [ ] Виджеты (опционально), пользовательские стили и гибкая анимация.

## Необходимые доказательства для заявления о 180 FPS

Сохранённые: версия Windows, мониторы/refresh rates, CPU/GPU, режим питания, build SHA, видеозапись теста (если доступна), traces/PresentMon или ETW, графики frametime, p50/p95/p99, загрузка CPU/GPU и RAM (private working set). Тест на 60-Гц дисплее не подтверждает 180 видимых FPS.
