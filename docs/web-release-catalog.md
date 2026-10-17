# Web releases and the updater catalog

Publishing a GitHub release alone does not make it visible in Delta Updater.
Core's `packages.catalog` reads `registry/plugins.json` from the `delta` main
branch; both the version and SHA-256 pin must point to the new package.

The `Sync verified Web release catalog` workflow runs at minutes 12 and 42
(GitHub scheduled runs may be delayed), on changes to its implementation, and
on manual dispatch. It downloads the latest stable Web release, verifies its
SHA256SUMS and embedded manifest identity/version, and commits only the Web
entry. It refuses downgrades, prereleases and same-version replacements. It
uses the repository's GITHUB_TOKEN, not a personal token.

Run `python tests/web-registry-sync.test.py` for safety tests and
`python scripts/sync-web-registry.py` to verify/sync locally. Missing assets or
failed verification leave the catalog unchanged and fail the workflow.

For immediate release visibility, manually dispatch this workflow after the
Web release assets are published. Scheduled checks provide eventual pickup;
GitHub raw-content caching may add a few minutes before clients see a commit.
