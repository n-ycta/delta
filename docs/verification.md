# Verification report

## Automated checks

- Rust stable / Linux: `cargo build` and `cargo test` pass; 6 integration tests + 1 SDK doctest. Includes node pairing/routing, validation/scopes, external metadata/caller scopes, expiry, CLI node-kind persistence.
- Web: 20 contract/registry/layout/endpoint tests + 6 server tests; TypeScript strict build passes.
- DB: 4 schema/parameterized-query/owner tests.
- Type: 4 aggregate/privacy/outbox tests.
- UI localization: 1483+ own presentation strings in partitioned ru.json; hardcoded Russian literals / JSX text checker passes. Published plugin metadata is localized data owned by each plugin, not executable UI code.
- npm audit for web after Vite 8.3.4 / React Router 7.18.4 upgrade: 0 vulnerabilities at verification time.

## Real end-to-end checks

Tested native PostgreSQL **15.18** + TimescaleDB **2.17.2**, not a mock store. Extension and `delta_records` hypertable confirmed. Docker Compose ships PG16/TimescaleDB; that container image was not separately launched in this environment.

Live core + three independently supervised plugins: discovery, RU/EN sessions → durable outbox → PostgreSQL → filtered/grouped web metrics, UUID/time deduplication, previous-period alignment, dashboard save/load, CPU/memory/node status, ping, restricted-token denial of admin actions, token creation with expiry. Passed `delta-web/tests/e2e-live.mjs`.

Browser checks: desktop 1440×1000 and mobile 390×844; home, plugins, typing detail, nodes, node detail, contracts, dashboard, events, access, settings. No page errors, no mobile horizontal overflow. First login from an empty browser session accepts `/api`; applying the user's token does not write it to localStorage. Scripts saved under delta-web/scripts.

## Reproduction

Follow ecosystem.md, then:

```sh
cd delta-web
npm run test:e2e
npm exec playwright install chromium --only-shell
npm run test:first-login
npm run test:browser
```

The browser suite creates a fixture dashboard in the test user's account. Use a disposable test node and database. Secrets remain in private runtime files outside git and are excluded from the delivery archive.

## Explicit limitations

No package marketplace/automatic binary updater, global kernel event stream, historical node telemetry collector, existing-token role editor, or OS-wide keyboard interception. Those are not faked with mock data in HTTP mode. Supervisor process CPU/memory are unavailable and shown as missing; API log timestamps represent retrieval time because the original supervisor keeps untimestamped lines. Core is Unix-only and relies on TLS reverse proxy/VPN for external access. Type trainer does not support IME; accuracy means correction ratio, not reference-text correctness. See each repository README for details.
