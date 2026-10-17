# MultiClaude — тестовая сборка с Claude Science

Запуск: `Start MultiClaude Science Test.cmd` (двойной клик).

Старая MultiClaude при этом продолжает работать: у тестовой сборки другой
identifier и отдельные каталоги.

## Почему CC_SWITCH_TEST_HOME

`DeviceStore` (live-state — кто каким клиентом владеет) намеренно не следует
override каталога конфигурации. Поэтому лончер дополнительно задаёт
`CC_SWITCH_TEST_HOME`, а в его каталог кладёт `live-state.direct.json`: все
клиенты помечены direct, и при старте тестовая сборка ничего не переписывает
у Claude/Codex/Gemini/Grok. Прокси на старом порту не поднимается.

## Что изолировано

| Что | Где |
| --- | --- |
| База, настройки, live-state, логи приложения | `%USERPROFILE%\.multiclaude-science-test-home\.multiclaude` |
| Данные демона Claude Science | `%USERPROFILE%\.claude-science-test` |

Реальный `%USERPROFILE%\.claude-science` не трогается (бэкап от 2026-10-17:
`C:\Users\alexivanov\claude-science-backup\20261017-213840`). В тестовом
каталоге Science попросит войти в claude.ai заново.

Чтобы тестовая сборка работала с реальными данными Science, убери строку
`CLAUDE_SCIENCE_DATA_DIR` из `.cmd` — но тогда переключение провайдера
перезапустит реальный демон.

## Порт прокси

Свежая база по умолчанию берёт `15723`, а этот порт уже занят рабочей
MultiClaude. Прямой режим Science прокси не нужен. Перед включением
**Route/прокси** зайди в Settings → Routing и поставь, например, `15724`,
сохрани, и только потом включай маршрут.

## Логи

`%USERPROFILE%\.multiclaude-science-test-home\.multiclaude\logs\cc-switch.log`

Строки Claude Science помечены `[claude-science]`: какой режим применён,
какой base URL, есть ли креды (значения не пишутся), какой data-dir, что
сказал `serve` при ошибке. Открыть каталог логов можно из Settings.

## Проверка

1. Открой вкладку Claude Science.
2. Добавь провайдера (например, DeepSeek с Anthropic-совместимым endpoint).
3. Direct — Science пойдёт прямо в endpoint провайдера.
4. Model mapping (Route) — через локальный шлюз MultiClaude, id `claude-*`
   маппятся на модель провайдера; предварительно смени порт прокси.
5. На странице видна строка статуса демона (running/stopped, порт, версия).
6. Переключение провайдера перезапускает демон Science — его вкладка в
   браузере потребует новую ссылку входа (`claude-science url`).

Прибрать после теста: удалить `%USERPROFILE%\.multiclaude-science-test-home`,
`%USERPROFILE%\.claude-science-test` и старый `%USERPROFILE%\.multiclaude-science-test`
(от первой попытки, когда изоляция ещё не покрывала live-state).
