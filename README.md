![Delta — your devices, your data, one connected system](docs/images/delta-banner.png)

# Delta

**A personal, modular system for your devices, data and automation.**

Connect your desktop and VPS, collect only the information you choose, and manage everything from one web console. The node is small; the web interface, storage and data sources are independent plugins. Replace a plugin without rebuilding the core.

[Quick start](#quick-start) · [Add another device](#add-another-device) · [Plugins](#plugins) · [Security](#security-and-access) · [Developer guide](docs/platform.md)

> Early release. Back up your data before upgrades. This is a self-hosted system, not a hosted account or NAT relay.

## Quick start

No Rust compiler, Git clone or manual API-token paste is required. The installer downloads a prebuilt node, a private Node.js runtime and verified plugin releases. Default: **node + storage + web**.

### Linux · x64 / ARM64

```sh
curl -fsSL https://raw.githubusercontent.com/n-ycta/delta/main/install.sh -o delta-install.sh
sh delta-install.sh
```

Choose a setup interactively, include typing, or install a bare node:

```sh
sh delta-install.sh --choose
sh delta-install.sh --plugins storage,web,typing
sh delta-install.sh --plugins none
```

### Windows · x64

Run in PowerShell as your normal user:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/n-ycta/delta/main/install.ps1 -OutFile delta-install.ps1
.\delta-install.ps1
# Optional: .\delta-install.ps1 -Choose
# Optional: .\delta-install.ps1 -Plugins storage,web,typing
```

Read the script before running it. Respect your organization's PowerShell execution policy; no policy bypass is required by the installer. Linux requires curl, tar (with xz support) and sha256sum. Windows uses built-in tar and PowerShell. Internet access to GitHub, nodejs.org and npm is required during setup.

The console opens at **http://127.0.0.1:7710** with a one-use owner link. On a headless VPS the installer prints an SSH-tunnel command instead. The owner link expires after five minutes; run the installer again with `--open` (PowerShell: `-Open`) to issue a new one. It does not reinstall an already working node. To force a binary/runtime download, set `DELTA_FORCE_DOWNLOAD=1` before running the installer.

Linux uses a systemd **user** service when available, otherwise a detached process with `node.log`. Windows installs a **current-user sign-in launcher**, not a privileged Windows service. A bare node does not expose a web UI until the web plugin is installed.

## Your control room

![Live console overview with test-node telemetry](docs/images/overview.png)

- Build and save dashboards from plugin-published metrics and visualizations.
- Inspect device health, component versions and supervisor logs.
- Install official plugins from pinned GitHub release packages in the store.
- Switch English / Russian globally, including localized plugin presentation.
- Create independent, scoped MCP gateways for your AI tools.

![Typing dashboard with generated QA aggregates](docs/images/metrics.png)

These are real browser captures against a local node. The typing history uses generated aggregate-only QA sessions, not captured personal text. CPU and memory are live. System load history and a system-wide event stream are not connected yet; the UI says so instead of inventing data.

## Plugins

| Repository | Purpose |
| --- | --- |
| [delta-web](https://github.com/n-ycta/delta-web) | Responsive console, dashboards, store, key access and MCP gateways |
| [delta_db](https://github.com/n-ycta/delta_db) | Unified record/document contract; file quick start or PostgreSQL + TimescaleDB |
| [delta_type](https://github.com/n-ycta/delta_type) | Opt-in RU/EN typing aggregates; never records typed text |

The store reads [the official index](registry/plugins.json). Downloads are version-pinned and SHA256-checked, archives reject links/path traversal, npm lifecycle scripts are disabled, and failed readiness checks restore the previous supervisor configuration. Plugins execute native code with the current user's OS permissions — **not a sandbox**. Review requested scopes and trust the publisher. Only owners or explicitly trusted administrative peers can install code.

Storage is a plugin, not a core dependency. Local files let you start without a database server; configure PostgreSQL/TimescaleDB in the storage plugin's settings when needed. Switching backends is explicit and **does not migrate existing data**. File mode is limited to 100,000 records and intended for a small personal setup.

## Add another device

In the console open **Nodes → Add device**. Enter the mutually reachable hub and new-device URLs, choose Linux or Windows, and generate a short-lived installation command. Run it on the new machine. No compiler is needed.

Use a VPN (for example WireGuard) or authenticated HTTPS reverse proxy. The core serves HTTP; setting a TLS field is not a substitute for a TLS proxy. Do not expose an unprotected port to the public Internet. This version does not provide NAT traversal or hosted relay infrastructure.

The checkbox **Allow this hub to install and manage plugins** is an explicit, directional trust decision. Without it, pairing grants normal capability calls, not administrative installation. Once paired with administrative trust, select the new node in a plugin's install dialog. Peers do not forward calls to a third node. For centralized remote typing ingestion, configure `STORAGE_NODE` to the hub node ID in the typing process environment; this is not automatic database replication.

## Security and access

- Local setup exchanges a short-lived **one-use code** for an HttpOnly, SameSite=Strict browser session. The owner token never appears in the URL or JavaScript response.
- Tokens have explicit capability scopes and optional expiry. A normal `call:*` token cannot execute administrative commands.
- In **Keys & recovery**, enroll an x-only secp256k1 public key. Login signs a nonce with BIP340/Schnorr; challenges are origin-bound, expire and cannot be replayed.
- Accepts raw private-key hex, Nostr-compatible `nsec`, or a generated English BIP39 phrase. The phrase uses a documented **Delta-specific derivation**, not a general Nostr seed standard. No Nostr relay account is required.
- Private keys and phrases stay in the browser. Save recovery material offline. Revoking a key also revokes its active sessions. An owner session is required to enroll another key.
- Key login defaults to localhost origins. For a custom HTTPS origin, add it to `identity_origins` in the private node configuration and restart the node; set matching `PUBLIC_ORIGIN` on the web process.

See [the security and operations guide](docs/platform.md) for file locations, backups, MCP policy boundaries and threat-model details. Never publish `admin.token`, `.env`, `config.json`, session files or MCP client credentials.

## AI access through MCP

Open **MCP gateways** to create Analytics, Operations or custom profiles. Each gets a separate bearer credential and kernel token. Pick explicit tools, permitted nodes/plugins, maximum time range/rows and field-redaction rules. Copy the generated URL/header configuration into an MCP client that supports remote Streamable HTTP.

The endpoint implements stateless JSON-RPC initialize/ping/tools-list/tools-call over HTTP. It does not need an external MCP proxy. Credential management, arbitrary token creation, shell execution and package installation are deliberately excluded from the MCP tool catalog. Revoke a profile to invalidate its token. Client secrets are shown once; use HTTPS for remote clients.

## Develop and extend

```sh
cargo test --locked
cargo build --release --locked
```

[API and architecture](docs/platform.md) · [Apache 2.0 license](LICENSE)

CI tests native Linux and Windows. Release assets target Linux x64/ARM64 (musl) and Windows x64. macOS/Android do not have a one-command prebuilt installer in this release.
