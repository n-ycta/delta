# Local source development

For end-user installation, use [the prebuilt installer](../README.md#quick-start). This guide is for development only.

Clone `delta`, `delta-web`, `delta_db` and `delta_type` into sibling directories. The node/core and plugins remain independent repositories and communicate through the capability API, not Cargo workspace links.

Requirements: Rust stable, Node.js 22.12+ (24 recommended), npm. PostgreSQL/TimescaleDB is optional; local file storage is the default.

```sh
cd delta
cargo build --locked
./target/debug/delta init --name Delta-dev
./target/debug/delta run
```

In another terminal:

```sh
cd delta_db && npm ci
cd ../delta_type && npm ci
cd ../delta-web && npm ci && npm run build
cd ../delta
node scripts/setup-local.mjs
```

The source setup script creates ignored, private `.env` files and absolute launchers, registers the three supervised processes, and saves test-only console/trainer credentials outside the repositories. Use a disposable node; do not run this script over an existing production deployment. To test PostgreSQL, set `DATABASE_URL` privately before running setup. The role must be able to run the included TimescaleDB/schema migration.

Web: `http://127.0.0.1:7710`; trainer: `http://127.0.0.1:7720`. For source-mode manual login, enter the test console token in Settings → Connection. End-user prebuilt installation instead opens a one-use browser session automatically.

```sh
cd delta-web
npm test
npm run test:server
npm run test:i18n
npm run test:e2e
npm run test:onboarding
npm run test:browser
npm run test:first-login
```

Live tests require the node and plugins; use only disposable data. Browser tests require Playwright Chromium. `test:onboarding` creates and revokes a temporary key/MCP profile; set `TEST_BOOTSTRAP_FILE` to the disposable web data directory's `bootstrap.json` to additionally test installer bootstrap exchange.

See [platform.md](platform.md) for contracts, scopes, networking, storage and security boundaries.
