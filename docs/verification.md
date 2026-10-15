# Verification report

## Automated source checks

- Core: native Linux build; 7 integration tests and 1 SDK doctest. Includes node pairing/routing, scopes, external registration, caller preservation, CLI node kind, signed-login replay and key revocation, and administrative-delegation limits.
- Native GitHub Actions: Linux and Windows `cargo test --locked` pass. Release builds produce Linux x64/ARM64 musl and Windows x64 binaries.
- Web: strict TypeScript production build; 22 contract/registry/layout/endpoint/chart-query tests and 12 server/security tests.
- Storage: 5 file/schema/parameterized-query/owner-isolation tests.
- Typing: 4 aggregate/privacy/outbox tests.
- UI locale checks: EN/RU key and interpolation parity; no hardcoded Russian literals or bare JSX text in owned UI source. Plugin presentation has separate EN/RU manifest patches.

## Real integration checks

Both file storage and native PostgreSQL 15.18 + TimescaleDB 2.17.2 were exercised, not replaced with mock stores. The PostgreSQL extension and hypertable are real. Switching file → PostgreSQL → file was tested with explicit acknowledgement; each backend retains its own data, without pretending to migrate it. The bundled Docker Compose image was not separately launched here.

A live node with all three independent plugins passed discovery, RU/EN aggregate ingestion, duplicate-session rejection, outbox flush, storage deduplication, filtered/grouped metrics, comparison queries, dashboard save/load, node telemetry/ping, token expiry creation and restricted-delegation denial (`delta-web/tests/e2e-live.mjs`).

Real HTTP tests passed signed browser login, one-use challenge rejection, key/session revocation, installer bootstrap exchange, MCP initialize/list/call, disallowed tool/source denial and profile revocation (`tests/onboarding-live.mjs`). JSON schemas now describe nested metric arguments so MCP clients can construct calls rather than guessing undocumented payloads.

Two independent local nodes were paired. Directional administrative trust, remote process restart, denial of broad read-only administrative calls, peer discovery and forwarding a typing record into hub storage with the authenticated remote node ID were tested. This verifies reachable-node routing, not Internet NAT traversal or replication.

## Browser checks

Desktop 1440×1000 and mobile 390×844: overview, dashboards, plugins/detail/settings, nodes/detail, contracts, events, access, settings, keys and MCP. No page errors or horizontal page overflow. Sparse samples can be shown as dots or connected lines, areas, steps and time bars. Lines connect observed values by default; the gap switch preserves breaks without changing missing values. Browser regressions verify all five modes, bucket queries, saved reloads and mobile rendering. Palette buttons are checked against their own panel bounds at widths 320, 360, 390, 768 and 1440 pixels. MCP and storage forms were visually inspected after layout fixes.

First manual login accepts `/api` before authentication and keeps API secrets out of localStorage. Automatic installer login uses an HttpOnly cookie instead. Product screenshots use live node telemetry and explicitly generated aggregate-only typing sessions; they are not personal keystroke captures.

## Release installation checks

`.github/workflows/install-test.yml` installs published binaries and all plugin release packages on Linux/Windows, using paths with spaces and no Rust compilation. The public release smoke test [38716266968](https://github.com/n-ycta/delta/actions/runs/38716266968) passed on both platforms with core 0.3.1, web 0.2.5, storage 0.2.1 and typing 0.2.1. A separate fresh Linux installation from the same public downloads also passed. `scripts/verify-install.mjs` verifies file backend, typing ingestion, all plugin registrations and web HTTP. Consult the latest **Prebuilt installation smoke test** run for the release-specific result; compiling a Windows binary alone is not a claim of a completed Windows install test.

## Boundaries

- No hosted relay/NAT traversal, automatic data replication, automatic cross-backend migration or cryptographically signed release chain.
- No system-wide event stream or historical CPU/memory collector. Supervisor per-process CPU/memory is unavailable, not invented. Untimestamped supervisor log lines use retrieval timestamps in the UI.
- Plugins execute with current-user OS rights, not a sandbox. The official store is not an unrestricted third-party repository downloader.
- Windows autostart is a current-user sign-in launcher, not a privileged pre-login Windows service. No macOS/Android prebuilt one-command installer is supplied.
- No OS-wide keyboard interception; trainer is opt-in, aggregate-only and rejects IME/composition counting.
- MCP is tool-only stateless Streamable HTTP with explicit bearer configuration; not OAuth/SSE/resource subscription support. Redaction is structural, not semantic removal of secrets from arbitrary log text.

Private test credentials, database connection URLs, node state and session files remain outside Git and delivery archives. Commit messages and READMEs are English, authored by the repository owner's configured account.
