# Delta ecosystem integration

Четыре независимых приватных репозитория: `delta` (Rust kernel + node/CLI), `delta-web` (React + внешний web bridge), `delta_db` (PostgreSQL/TimescaleDB), `delta_type` (агрегаты RU/EN). Плагины не включены в Cargo workspace и общаются только по API.

## Локальный запуск

Разместите клоны рядом. Нужны Rust stable, Node.js **22.12+** (Node 24 рекомендован), npm, Docker Compose.

```sh
cd delta && cargo build --release
./target/release/delta init --name Delta
./target/release/delta run
```

В другом терминале:

```sh
cd delta_db
POSTGRES_PASSWORD='choose-a-strong-password' docker compose up -d
npm ci
cd ../delta_type && npm ci
cd ../delta-web && npm ci && npm run build
cd ../delta
DATABASE_URL='postgres://delta:YOUR_URL_ENCODED_PASSWORD@127.0.0.1:5432/delta' node scripts/setup-local.mjs
```

Скрипт записывает `.env` 0600 и добавляет отдельные процессы в supervisor с явными scopes/идентичностью; supervisor выдаёт каждому эфемерный token и отзывает его при остановке. Пользовательские токены сохраняются отдельно. Одноразовый bootstrap: при повторном запуске создаст новые токены; старые нужно отозвать. После перезапуска supervisor сам запускает плагины. Web service token имеет admin: исходный supervisor и tokens API admin-only, поэтому bridge проверяет права исходного caller и admin-флаги capabilities. Этот token только в окружении дочернего процесса; не отправляется браузеру. Для запуска без supervisor задайте отдельный server token в `.env`.

Web: `http://127.0.0.1:7710`. Настройки → Подключение: режим HTTP, адрес `/api`, содержимое `console-user.token`. Секрет UI хранится только в sessionStorage, не localStorage. Тренажёр: `http://127.0.0.1:7720`, токен из `trainer-user.token`. Файлы токенов находятся рядом с репозиториями и не должны попадать в git/архивы. Секрет БД не включайте в URL в истории публичного терминала.

## Передача данных

`typing.ingest` → строгая сессия без текста → дисковый outbox → `storage.write` → Timescale hypertable → `web.metrics_query` → динамические виджеты. Стабильные `(time,id)` обеспечивают идемпотентный повтор. Документы (дашборды/аудит) идут через capabilities `storage.document_*`; SQL в потребителях отсутствует. Для другой БД реализуйте контракт storage v1 и смените `STORAGE_COMPONENT` у всех плагинов.

## Права и ограничения

Права проверяет ядро, не «Просмотр от имени роли». Bridge передаёт пользовательский token на HTTP-proxy, а при вызовах service token проверяет caller scopes. UI принимает `metadata.ui` как данные и валидирует через Zod; удалённый JS не исполняется. Admin-only операции не становятся доступными от `call:web.*`.

Нет встроенного TLS: снаружи используйте reverse proxy/Tailscale/WireGuard. `PUBLIC_ORIGIN` веб-плагина должен совпадать с origin внешнего HTTPS. По умолчанию HTTP-плагины и PostgreSQL слушают localhost.

История нагрузки/uptime, поток всех kernel events, package marketplace, обновление бинарников, миграция плагина между нодами и редактирование scopes существующего токена пока не реализованы. Неподдерживаемые команды возвращают явную ошибку; нет фиктивного успеха. Процессные CPU/memory не измеряются исходным supervisor. Type собирает только в тренажёре/через агрегатный API, а не глобально в ОС. Удаление установленного процесса выполняется через supervisor; purge данных не поддерживается.

## Проверки

`cargo test`; в web: `npm test`, `npm run test:server`, `npm run test:i18n`, `npm run build`; в DB/Type: `npm test`. E2E-тесты отдельно описаны в отчётах репозиториев.
