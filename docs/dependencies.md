# Dependency and invocation contracts

Core 0.4.3 adds declarative dependencies and invocation-bound forwarding. Both APIs are independent of a graphical host.

## Dependency descriptors

A plugin's `metadata.ui.dependencies` is an array of up to 32 descriptors:

```json
{"id":"dev.example.search","kind":"plugin","version":">=1.0.0, <2.0.0","required":false,"features":["search"]}
```

`kind` distinguishes `core`, `plugin`, `provider`, `ui-package`, and `prerequisite`. Unsupported kinds fail closed when required; they are never installed as executable plugins. Version requirements use Rust semver syntax, including comma-separated comparators. Optional dependencies do not block unrelated capabilities.

A capability can declare `requires: ["search"]`. Core evaluates all dependencies contributing to that feature, checks installed version and current registration, and returns `missing_dependency` (HTTP 424) when unavailable. Required dependencies gate all capabilities of the consumer. Registry discovery and dependency diagnostics remain accessible through Core.

- `dependencies.status {id?}` returns dependency states, feature gates and required reverse references.
- `dependencies.check {capability}` reports availability without executing the operation.
- `dependencies.bind {id, dependency, component}` explicitly selects a local provider whose `metadata.contracts` advertises a compatible contract version. No provider is silently chosen.
- `packages.plan {id, optionalDependencies?}` returns a bounded topological installation plan, rejects cycles and incompatible releases, and explains omitted optional integrations.
- `packages.install` accepts the same optional selection plus `approveDependencies`. Installing additional executable dependencies requires explicit owner approval. Selected dependencies install before the consumer. This is ordered installation, not an atomic multi-package transaction: successfully installed dependencies remain if a later package fails.

Stopping/removing a required provider is blocked while an enabled consumer depends on it. A global owner can explicitly use `force: true`; package replacement uses this maintenance path. Removing an optional provider leaves its consumers installed and gates only the declared features.

Resolution is local-node. Cross-node provider bindings, UI package activation, and system prerequisite installation are not implemented by this resolver. A disabled provider is never silently enabled. Metadata does not grant operating-system permissions or sandbox executable code.

## Invocation-bound forwarding

An external component can POST to `/v1/components/:id/call` with its registration session, the outstanding `invocation` ID, destination `capability`, `args`, and optional `node`.

Core verifies registration authority and session, retrieves the original principal from its own pending invocation table, and re-runs normal destination authorization. Caller JSON, service owner credentials, and UI-provided scopes cannot replace that principal. Unknown, completed, timed-out and wrong-component invocations cannot be borrowed. The request borrows an already authorized in-flight operation; token revocation does not retroactively cancel an operation already accepted by Core.

The caller needs permission for both the facade and the destination. Existing global-owner keys continue to work; narrowly scoped compatibility keys may need new explicit destination scopes. Browser credentials are never forwarded as an administrative service credential.
