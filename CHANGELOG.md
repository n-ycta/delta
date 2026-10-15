# Changelog

## Release policy

- Collect compatible fixes in **Unreleased**; do not publish a separate release for every small commit.
- Publish tested, meaningful batches only after explicit maintainer approval.
- Before tagging, move the reviewed changes into a version section below. Release workflows require that section.
- Historical patch releases have been consolidated into the next retained release. Git tags remain available for source history and comparison links.

## [Unreleased]

- Improve release documentation and changelog-backed release publishing; no new runtime release is created for this maintenance change.

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

