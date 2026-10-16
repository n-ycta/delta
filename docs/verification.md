# Verification report

## Current split-service source checks

- Core: Linux build, 17 integration tests, dependency unit tests, host-runtime tests and SDK doctest. Portable builds also run the dependency unit tests. The integration suite exercises scopes, pairing, identity, external invocation forwarding, optional feature gates and explicit contract provider selection.
- Bootstrap: five installer regressions, including a catalog component outside the old default preset, dependency approval and service installation failure handling.
- Shared UI: TypeScript production build, 32 contract/registry/layout tests, six Host Bridge tests, EN/RU key and interpolation parity.
- Web: production build, shared contract tests and 56 server/security tests. Independent service test fixtures are development-only immutable archives, not production imports.
- MCP: 12 tests, including headless HTTP, legacy profile migration, canonical metric restrictions and coexistence with Type's trainer port.
- Updater: 13 tests. Provision: eight tests. Observability: seven tests.

Consult each release workflow for platform-specific results. A cross-compiled binary is not evidence of a completed physical-device installation test.

## Live isolated checks

A fresh disposable profile installed the public Core 0.4.4 Linux binary and seven production plugin release archives from the official catalog. Type trainer on 7720 and MCP on 7730 ran together; direct and Web-proxied MCP initialize succeeded, and an under-scoped facade caller was denied.

A disposable local Core ran all four extracted services with no Web process. MCP initialize/tool discovery, updater status, provisioning preflight and observed event persistence worked through their own APIs. Web compatibility aliases were then tested against the same services, with original-caller scope denial and private event visibility. Stopping Web did not stop the independent service processes.

Desktop 1440px and mobile 390px browser checks covered RU/EN dependency panels and optional feature availability. Stopping MCP returned `missing_dependency` (HTTP 424), hid the unavailable constructor and left unrelated plugin discovery working. No page errors or horizontal page overflow were observed in these checks. These tests ran in a sandbox, not on the user's PC or VPS.

Historical checks also exercised two local paired nodes, file storage and native PostgreSQL 15.18 with TimescaleDB 2.17.2. Those older runs do not establish that the current managed-container or physical Windows/Android installation has been tested. Storage 0.4.x now implements explicit verified backend copying with backups; review its dedicated tests and migration status.

## Release installation checks

The prebuilt installation workflow installs published Core and plugin packages on Linux/Windows without Rust compilation. Release-specific status is authoritative; an older successful workflow is not proof of the current release. Catalog entries must be populated from downloaded release assets after verifying SHA256SUMS and manifest/package versions.

The final [prebuilt installation run 38884994786](https://github.com/n-ycta/delta/actions/runs/38884994786) passed on both Linux and Windows using Core 0.4.4, Web 0.6.7 (shared UI 0.1.2), Storage 0.4.0, Type 0.4.4, MCP 0.1.2, Updater 0.1.0, Provision 0.1.0 and Observability 0.1.1. It verifies seven supervised plugin registrations, explicit file storage, generated typing ingestion, dependency readiness and direct/proxied MCP initialization. This is a fresh no-service CI installation, not a physical desktop or Android test.

A sandbox release upgrade from Web 0.6.6 to 0.6.7 preserved the independent services. RU/EN browser checks at 390px and 1440px passed with MCP present and disabled, including minimum readable dependency-name widths and no page overflow.

## Boundaries and incomplete work

- Native Desktop and Android hosts remain previews with limited screens, not full Web parity. Android physical-device/background lifecycle and credential-vault work remain incomplete. Windows packages are unsigned previews.
- Shared UI delivery is currently bundled with each host. Independent activation/rollback and a cryptographically signed release chain are not implemented.
- No hosted relay/NAT traversal, automatic data replication, atomic multi-package installation or durable restart-resumable package jobs.
- Observability samples node telemetry and observed lifecycle events; it is not an OS-wide event stream. Missing periods stay missing. Per-process telemetry is not invented.
- Plugins execute with current-user OS rights, not in an OS sandbox. Global-owner-only operations remain restricted even if a narrow administrative scope is granted.
- Dependency resolution and provider binding are local-node. UI-package activation and system prerequisite installation are not implemented by the plugin resolver.
- Type includes opt-in native collectors on supported platforms; this report does not claim physical keyboard permission/IME testing across every desktop environment.
- MCP is tool-only HTTP with bearer configuration, not OAuth/SSE/resource subscriptions. Redaction is structural, not semantic scrubbing of arbitrary text.

Private tokens, connection credentials, node profiles and test state stay outside repositories and delivery archives. Technical documentation is English; TODO and roadmap files are not published.
