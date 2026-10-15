# delta (Rust)

Rust rewrite of `n-ycta/delta_3`: one API for nodes, components and capabilities.
The wire protocol (`/v1/...`), error codes, scopes, HMAC peer signing and the
`config.json` format are kept identical to the Python version, so Python and Rust
nodes (and plugins) can talk to each other.

```
cargo build --release
./target/release/delta init
./target/release/delta run            # or: delta service install
delta caps
delta call system.info
cargo test
```

Layout: `kernel.rs` (registry/router/access), `server.rs` (axum transport), `client.rs`,
`plugin.rs` (SDK for plugins written in Rust), `components/*` (core, tokens, system, exec,
peers, supervisor), `node.rs` (assembly), `main.rs` (CLI).

## Differences from the Python version
* TLS is not built in (`tls_cert`/`tls_key` are ignored with a warning): use a reverse proxy,
  WireGuard or Tailscale. Easy to add later with `axum-server` + rustls.
* Unix only (systemd/launchd, setsid, statvfs).
* `system.info` returns `runtime: "rust"` instead of `python`.
* Tokens are `dlt_` + 64 hex chars (still stored only as SHA-256).
* A failing `spawn` in the supervisor retries every 5 s instead of every 0.5 s.
* Python's unit tests are not ported 1:1; `tests/integration.rs` covers auth, scopes, validation,
  an external plugin and two-node pairing/routing.

## Verified build and additions (0.2.0)
Built and integration-tested on Linux with Rust stable. External components can publish optional
`metadata.ui` manifests, returned by `core.components`; caller identity includes scopes for
safe plugin-side delegation. Tokens support `ttl_seconds`; expiration is checked on every request.
`system.info.cpu_usage` is a sampled Linux CPU percentage (null on unsupported hosts).

Core and node remain one Rust executable. Web, storage and typing are independent repositories,
connected via the existing protocol; no SQL/UI libraries are linked into the core.
