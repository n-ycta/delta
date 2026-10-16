# Changelog

## [v0.4.3]

- Add invocation-bound external-component forwarding without service-credential elevation.
- Add local declarative dependency diagnostics, feature gates and explicit provider bindings.
- Add cycle-checked package plans, optional dependency selection and owner approval.
- Protect required providers from accidental removal; retain explicit owner maintenance override.
- Add delegation and optional-feature integration regressions. Ordered dependency installation is not an atomic group rollback.

## Release policy

- Collect compatible fixes in **Unreleased**; do not publish a separate release for every small commit.
- Publish tested, meaningful batches only after explicit maintainer approval.
- Before tagging, move the reviewed changes into a version section below. Release workflows require that section.
- Historical patch releases have been consolidated into the next retained release. Git tags remain available for source history and comparison links.

## [Unreleased]

- No pending changes.

## [v0.4.2]

### Features
- Portable library assembly without desktop process/npm components or host telemetry; explicit mobile policy also applies when desktop features are compiled into a workspace.
- Credential-private native host runtime with bounded typed requests, persistent identity, exclusive profile lease and verified attachment to an existing local node.

### Security
- The native owner credential remains in Rust and is never returned as Host Bridge metadata. Mobile assembly does not register process/package capabilities.

### Upgrade notes
- Default CLI behaviour is preserved. Android/Desktop native hosts remain experimental; this release does not certify physical reboot/suspend, mobile background operation or secure-store integrations.

## [v0.4.1]

### Fixes
- Preserve signed-login token hashes across core restarts, so valid browser sessions are not lost after a reboot.
- Enable or repair the user service when rerunning the installer, even if a temporary node is already online.
- Report service-installation failures instead of silently falling back to a process without autostart.

### Improvements
- Support an optional 30-day remembered-device session, with the requested lifetime bound to the signed challenge.
- Preserve scoped permissions and revoke persisted sessions when their enrolled key is removed.

### Upgrade notes
- Update Delta Web to v0.5.1 for the remembered-device option. Old expired or memory-only sessions require one fresh sign-in.

## [v0.4.0]

### Features
- Explicit directional administrative trust for paired nodes, including equal-peer deployments.
- Bind incoming administrative grants to single-use pairing invitations; only global owners can grant or change trust.
- Per-relationship callback URLs for protected reverse tunnels without changing global node addresses.
- Report incoming administrative trust in peer discovery and support immediate downgrade/revocation.

### Improvements
- Identify bare Linux server installs with the VPS node kind.
- Document core-supervised SSH connections, private forwarding keys, restart recovery and operational limits.

### Security
- Reject administrative trust escalation by narrowly delegated pairing tokens.
- Bound pending invitations and validate callback URL schemes and credentials.
- Preserve non-administrative defaults for existing pairing clients.

### Validation
- Integration tests cover real two-node routing, mutual and one-way grants, downgrade, single-use invitations and delegated-token rejection.

## [v0.3.4]

This release focuses on safe checked plugin updates.

### Features
- Accept expectedVersion and expectedSha256 guards when installing a checked plugin release.

### Improvements
- Add Linux/Windows regression coverage for checked package updates and disabled components.
- Provide the core safety features required by the Web 0.4.0 update center.

### Bug Fixes
- Preserve the disabled state of a managed plugin during an update.
- Do not launch disabled plugins merely to wait for readiness.
- Honor enabled=false when adding a supervised component.

### Security
- Reject installation when the official catalog version or checksum changed after the update was selected.

### Installation
- Download the matching Linux/Windows asset and verify `SHA256SUMS` before installation.

**Full Changelog:** https://github.com/n-ycta/delta/compare/v0.3.3...v0.3.4

## [v0.3.3]

Includes changes from v0.3.0 through v0.3.3.

### Features
- Modular Rust node with a capability API, peer connections and supervised external components.
- Prebuilt Linux/Windows binaries and guided installation with selectable official plugins.
- Checksum-verified plugin installation with asynchronous jobs, configuration rollback and upgrade support.
- Node identity/device kinds, plugin metadata and portable CPU, memory and disk telemetry.

### Improvements
- Refresh installed plugins against the official catalog and reuse the configured node port.
- Add Linux/Windows installer and startup/readiness regression checks.

### Bug Fixes
- Preserve component service identity during plugin replacement.
- Handle Windows Node/npm paths containing spaces.
- Do not reuse an old bootstrap when a newer core release was requested.
- Stage/swap the private Node runtime rather than overwriting a running Linux executable.
- Preserve startup diagnostics before rollback and refresh managed plugin registration.
- Retain readiness published before supervisor subscription to avoid a startup race.

### Security
- One-use Schnorr login, expiring tokens and explicit plugin identities/scopes.
- Restrict administrative delegation and verify official package SHA-256 digests.
- Keep process diagnostics private instead of returning raw logs through package API errors.

### Notes
- Retained as the compatibility baseline for upgrade/rollback testing. Use v0.3.4 for new installations.

### Installation
- Download the matching Linux/Windows asset and verify `SHA256SUMS` before installation.

**Source:** https://github.com/n-ycta/delta/tree/v0.3.3

