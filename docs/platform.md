# Delta platform and operations

## Separation of responsibilities

The core owns identity, capability routing, authorization, peer transport, process supervision and verified official-package installation. Plugins run as separate processes and register declarative capabilities over the external-component protocol. The web console is not embedded in the core; storage is not embedded in the web console. Producers send typed records through capabilities, not database-driver imports.

The API is `POST /v1/call` with `{capability,args,node?}` and an `{ok,result}` or `{ok:false,error:{code,message}}` envelope. Discovery: `/v1/capabilities`, `core.components`, `core.catalog`. External components use registration, long-poll invocations and result delivery. See the small independent SDK copied into each plugin's `server/delta-sdk.mjs`.

## Files and lifecycle

Default private directory: Linux `~/.delta`; Windows `%LOCALAPPDATA%/Delta`. Override with `DELTA_HOME`.

- `bin/`: node executable and installer bootstrap.
- `runtime/`: private Node.js/npm runtime downloaded with official checksums.
- `config.json`: node settings, token hashes, paired-peer secrets, managed process environment.
- `admin.token`: local recovery/owner credential. Never share or commit it.
- `plugins/<component>-<version>-<suffix>/`: immutable unpacked code and production dependencies.
- `data/storage/`: file records/documents and private backend connection settings.
- `data/web/`: manifest cache, browser sessions, MCP profile credentials and one-use bootstrap state.
- `data/typing/`: outbox and opt-in collection settings.

Stop the node before copying configuration/data for a consistent backup. Protect the entire directory as sensitive, not just `admin.token`. Unix files use restrictive modes; Windows relies on the current user's profile ACLs. Configure appropriate NTFS ACLs for your environment. Old package directories remain available after upgrades; do not delete active paths. No automatic cleanup or global retention policy is implemented.

`delta service install` installs systemd user service on Linux (linger may require administrator approval). On Windows it writes a current-user Startup launcher, which starts after interactive sign-in, not at machine boot before login. Headless setup falls back to a detached process and records `node.log` when service installation is unavailable.

## Release trust

Node assets are built by GitHub Actions for Linux x64/ARM64 musl and Windows x64. Installers verify SHA256SUMS over HTTPS. Official plugin index entries pin repository, version, asset URL and SHA256. Node installation jobs verify the hash, reject archive links/traversal, enforce compressed/uncompressed size limits, validate manifest identity/version, and run `npm ci --omit=dev --ignore-scripts`. npm lockfile integrity protects dependencies. This is publisher/transport trust, **not cryptographic release signing or sandboxing**.

Plugins may access the OS as the current user. Web is deliberately an administrative service because it manages tokens and plugins; privileged delegation checks the original caller's scope before using that service credential. Package installation and token creation require global administrative authority even if someone grants an isolated `admin:packages.install` / `admin:tokens.create` scope.

Readiness failure after upgrade restores the previous supervisor configuration. Old data remains outside package folders. A process crash or power loss during installation is not a transactional OS rollback; inspect `packages.jobs` and supervisor logs. Installation jobs are in memory and do not survive a node restart.

## Networking and pairing

Core defaults to loopback port 7700; web loopback 7710; trainer loopback 7720. Core HTTP is not encrypted. Use VPN or a TLS reverse proxy, never a public plaintext administrative endpoint. Configuring a `tls_cert` field does not enable built-in TLS.

Pairing codes are one-use and expire after ten minutes. Both devices need reachable core URLs. Administrative trust is directional: the new device may explicitly trust the hub for management; merely knowing a read/call peer does not grant owner power. Local scope checks happen before forwarding; the peer applies its own trust ceiling. Peer calls are HMAC-authenticated and cannot be forwarded a second time. The protocol does not carry an end-user identity across peers: a trusted administrative hub is a security boundary.

Pairing provides capability routing, not a data-replication engine. A remote typing producer can forward records to a specified hub with `STORAGE_NODE`; local durable outbox retries transient delivery failures. Documents remain owner-isolated in whichever storage backend is active.

## Keys and recovery

A public key must be enrolled by an existing owner. BIP340 Schnorr signatures authenticate a canonical message containing protocol tag, node ID, public key, random nonce, expiry and audience origin. Challenges last 90 seconds and are consumed before verification. Key sessions last eight hours and are ephemeral in the core; restart requires a fresh key login. Removing a key revokes its issued tokens.

Raw 32-byte private-key hex and Nostr `nsec` use the same secp256k1 key. A BIP39 English phrase derives a Delta key using `SHA256(UTF8("delta-identity-v1:") || BIP39_seed_without_passphrase)`. This is a Delta-specific recovery format. It is not compatible with an arbitrary wallet/relay seed derivation. Neither private key nor phrase is sent to the server. The browser can still be compromised by malicious extensions or injected script: trust the site, use HTTPS remotely, save recovery material offline.

Default allowed identity origins are `http://127.0.0.1:7710` and `http://localhost:7710`. For a custom console origin, edit `identity_origins` in private config while stopped and set `PUBLIC_ORIGIN` for web. Do not add wildcard origins. Local installer owner links use one-use random bootstrap codes (hash stored on disk, five-minute expiry). The token exchange creates an eight-hour HttpOnly/SameSite=Strict cookie session; HTTPS deployments also set Secure. Manual API tokens remain sessionStorage-only.

## MCP policy

Each MCP profile has an independent random bearer secret (hash stored) and separate kernel token. The corresponding kernel token must be retained privately to perform calls; the profile file is sensitive even though client secrets are hashed. Owner APIs never list plaintext credentials. Revocation removes both profile and token.

Supported remote HTTP JSON-RPC methods: initialize (`2025-06-18`), ping, notifications/initialized, tools/list, tools/call. No SSE session, resource subscriptions or OAuth discovery is implemented. Configure a capable client with URL and Authorization bearer header. Use HTTPS for remote clients. Client configuration snippets are generic; a particular client may have additional setup requirements.

Only the published safe capability catalog is selectable. Administrative supervisor operations use exact `admin:<capability>` tokens; arbitrary shell, package installation, token creation, key enrollment and MCP-profile management are excluded. Source restrictions are enforced on known metric/storage query arguments; range and row caps are checked; list outputs are filtered; configured field names are removed recursively. Arbitrary text logs cannot be semantically redacted. Do not enable log access for an agent that must not see log text. Node restrictions require an explicit allowed `_targetNode` for direct tools. Empty node/plugin lists mean unrestricted within the selected tool set. The web list tools are filtered without requiring a target.

## Localization and extension

Use one manifest with stable IDs/types/scopes/versions and `metadata.ui.localizations.en/ru` patches. Display name/description/label/help/reason/placeholder/group/notes/alt may change; contract semantics and units may not. Arrays match by stable `id`, `key`, `scope`, `value` or `version`. Screenshots are HTTPS URLs with optional locale and descriptive alt text. The frontend never executes plugin-supplied JavaScript widgets; registered local widget renderers must validate their own schemas and version compatibility. Unknown capabilities/visualizations remain inspectable instead of crashing the panel.

All source projects use Apache 2.0. This does not relicense runtime dependencies or PostgreSQL/TimescaleDB.
