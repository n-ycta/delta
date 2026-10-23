# Installation and operations notes

Details that used to sit at the bottom of the README. For the first install, follow the [quick start](../README.md#quick-start).

## Installer failure diagnostics

An existing installation is reused only when its `bin/release-version` matches the requested release; legacy installs without that marker are refreshed with checksum verification. `DELTA_FORCE_DOWNLOAD=1` still forces a download. Bootstrap reads the configured core port on repeat installation. The Linux user unit explicitly selects the private Node runtime, without depending on an interactive shell's PATH.

If a plugin does not register within 60 seconds, package installation saves its process state and recent logs to a random private file under `data/package-diagnostics/` **before** rollback. The API returns the file location, not the logs. Treat diagnostic contents as sensitive; redact before sharing. A diagnostic write failure does not block rollback. Package jobs include `startedAt` so clients can choose the latest attempt, rather than sorting random job IDs.

Run `node --test tests/installer.test.mjs` alongside `cargo test --locked` to check bootstrap port selection and stale installer reuse. Release binaries and the package index must be rebuilt/versioned together; editing source alone does not update the installed prebuilt runtime.

## Recover access after a browser-owner session expires

If no key was ever enrolled, do not reset or reinstall the node. Use the local-owner [access recovery guide](docs/recover-access.md) to issue a one-use link on the original node and register a saved key. Recovery preserves the existing node ID, plugins, settings and data.

## Autostart repair and browser sessions (v0.4.1)

The default installer now enables or repairs the user service even if a temporary node is already online. On Linux, an owned temporary process is safely stopped before handing control to systemd. Service setup failures are no longer silently presented as a reboot-ready installation; `--no-service` is an explicit temporary/development mode only.

Signed-login token hashes persist across core restarts. The optional remembered-device challenge binds a 30-day lifetime to the signature; ordinary signed sessions remain limited to eight hours. Revoking the key also revokes its persisted sessions. Existing expired or in-memory sessions require one fresh login. Web and core changes must be deployed together for remembered-device login.

## Declarative plugin dependencies (Core 0.4.3+, installer fixes in 0.4.4)

The installer resolves catalog component names or stable plugin IDs, not a fixed plugin pool. Required dependencies are planned before execution; optional integrations require selection. Use `--with-optional` to explicitly select the chosen plugins' optional packages, or `--approve-dependencies` to approve additional required executable dependencies. Review requested scopes before approval. Missing optional providers disable only their declared features. See [dependency contracts](docs/dependencies.md) for compatibility, explicit provider bindings and removal protection. Group installation is ordered, not atomic.
