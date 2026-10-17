# Reliability release: final delivery review

This checklist tracks the complete user report and supersedes the pre-publication delivery snapshot in delta-ui/docs/2026-incident-review.md. Implementation, tests, publication, updater visibility and installation are different states. No production user profile was reset or inspected.

| # | Acceptance requirement | Initial evidence / owning boundary | Status |
|---|---|---|---|
| 1 | Failed updates terminate every waiting item | Updater expects a flat version; Core returns installed[]; pending entries never terminalize | Updater 0.1.1 released; 17 deterministic tests cover Core installed[] result and terminal skipped states |
| 2 | Web restart preserves authenticated shell | Auth gate rejects cached principal on any info error | Implemented; actual WebAuthGate browser regression preserves mounted input/frame on transient failure; 401 still signs out |
| 3 | Reconnect notices are localized; no false core-down/login error | web.info not_found is not authentication failure | Implemented; RU reconnect regression and 2,186-key EN/RU parity pass; inventory errors no longer mark the connection down |
| 4 | Update click acknowledges immediately; active progress refreshes promptly | Mutation pending not used by install buttons | Implemented; click-pending browser assertion and one-second active-job polling |
| 5 | Collapsible compact node groups with update counts | Flat component list repeats node names | Implemented; actual Updates browser test verifies per-node groups, counts and collapsed current node |
| 6 | Progress is prominent and distinct | Ordinary panel follows long list | Implemented; mobile light/dark screenshots inspected; active accent panel placed before inventory |
| 7 | Optional bounded parallel updates | Serialize installer per node; local Web last; no unsafe same-node concurrency | Updater 0.1.1 released; 1–3 cross-node workers tested; same-node serialization and local Web-last preserved |
| 8 | Context-menu selection and batch library deletion | Per-item confirmation only | Media 0.3.6 released; real service + browser at 390/1440 verify right-click selection, batch deletion and reload |
| 9 | Skip delete confirmation for one hour | Must expire; never skip silently forever | Media 0.3.6 released; browser confirms one-hour opt-out and immediate subsequent delete |
| 10 | Deleted radio never reappears after sync/reload | Client/server deletion and identity protocol | Media 0.3.6 released; real service restart and source-identity deletion tests; published Core capability path passes |
| 11 | Deleted YouTube item never reappears after sync/reload | Same boundary, test independently | Media 0.3.6 released; YouTube source tombstone test and browser delete/sync/reload pass |
| 12 | Favorite and queue states remain independent and durable | Whole-state sync and cross-frame mutation races | Media 0.3.6 released; field patch/reconciliation and concurrent search/library action regressions pass |
| 13 | Explicit single-click Play on search and library rows | Search lacks play button/title action | Media 0.3.6 released; explicit Play button/title/cover; real local decoder regression passes |
| 14 | Play now does not mutate queue | play handler inserts current track | Media 0.3.6 released; currentId/playId independent of queue; saved-state regression passes |
| 15 | Stable progress without synthetic time or zero jumps | Player current reference detached by sync | Media 0.3.6 released; stable decoder references and provider-confirmed positions; real WAV seek/reload regression passes |
| 16 | Player displays available artwork | Footer always renders generic icon | Media 0.3.6 released; footer uses available artwork; unavailable artwork retains fallback |
| 17 | Discoverable video show/hide/restore, compact/resize | Preserve decoder when hidden; test actual provider separately | Media 0.3.6 released; real WebM decoder survives compact/resize/hide/restore. Cross-surface video reparenting and external YouTube playback not established |
| 18 | No false already-open-elsewhere warning | Slot placeholder unconditionally rendered | Implemented; repeated actual frame switches preserve one iframe and no false placeholder; browser regression passes |
| 19 | YouTube artist/channel page with paginated tracks | Provider support and real external verification needed | Sources 0.2.5 and Media 0.3.6 released; real channel metadata plus distinct first/second pages verified (30 then 60 items); deterministic continuation test |
| 20 | YouTube playlist import | Provider support and real external verification needed | Implemented; real public playlist returns 13 items; deterministic pagination. Very large imports remain bounded by bridge payload/storage limits |
| 21 | Canvas move/resize snapping with guides and Alt bypass | Missing implementation; screen-pixel threshold at every zoom | Implemented; move/resize edge/center snapping, scale-aware threshold and Alt bypass; unit and actual drag/guide/persist tests pass |
| 22 | Studio browser address/search/navigation | Embedded websites can refuse framing; never claim bypass | Implemented; address/search/back/forward/reload and external-browser fallback tested. Embedded websites may refuse framing; not an unrestricted native browser |
| 23 | Remove redundant whole-Media desktop | Versioned, backed-up migration; preserve user customization | Implemented; backed-up versioned migration removes only exact redundant root whole-Media desktop, not custom compositions |
| 24 | Remove redundant whole-Console desktop | Same migration; remove template from creation UI | Implemented; same migration covers whole-Console; customized layout preservation unit tests pass |
| 25 | Media dashboard surfaces operate | Test multiple surfaces and one playback owner | Real Media SDK embedding at 390/1440 and independent surfaces/single-owner browser tests pass; physical native devices not tested |
| 26 | App widgets save regardless of valid size | UI emits system/app; server rejects this feed | Web server accepts system/app feed; round-trip schema regression and plugin widget save/reload browser tests pass |
| 27 | Itemized review and truthful delivery states | Record implementation, tests, push, release, registry, installed separately | This review tracks every reported requirement; delivery and unverified conditions remain separate |

## Publication and updater visibility

| Component | Published version | Publication / delivery evidence |
|---|---|---|
| Shared UI | 0.3.2 | Release asset checksum verified; product tag d2a4ed787f75eb75dfd7bf77536c9f7c544d4bda; hosts pin 0935e3f6a70e6e74f44902dde44c74f93682825e, which adds only an explicit MCP-tab test selection. Full shared-UI CI passed on that commit. |
| Web | 0.6.18 | Release and 65 server tests passed; release archive checksum verified; official registry version and archive SHA match. |
| Media | 0.3.6 | Main/tag Verify and Release passed; both release archives verified; official registry version and archive SHA match. |
| YouTube / SoundCloud / Spotify / Downloads | 0.2.5 | Verify and Release passed; all downloaded provider archives match official registry hashes. |
| Updater | 0.1.1 | 17 tests; release checksum and official registry match; independent installer exercised against published Core and official registry. |
| Desktop | 0.1.11 | Native build CI passed; Linux AppImage and Windows executable match published SHA256SUMS. Physical installation not performed. |
| Android | 0.1.11 | Build, emulator acceptance and publish jobs passed in run 39027394983; signed APK matches published SHA256SUMS. Physical device not tested. |

Registry publication commits: 0e5bf72 (Media, providers, updater, shared UI) and c12a858 (verified Web 0.6.18). The unchanged official raw registry URL was re-read after CDN expiry; it advertises Media 0.3.6, all four source packages 0.2.5, updater 0.1.1 and Web 0.6.18. Their hashes were compared with actual downloaded release assets, not guessed from a local build.

### SHA-256 of key published assets

- Media delta-app: `ce4a491b06de21c82bc119f6cac8fcd7eaf7ada9a35b95f82a18299129231794`
- Media tar.gz: `fc30f642888bf7b3aac9f42f6abb19d9ec9e9c24c5a43ad2f4854409dc1c46a6`
- Shared UI tar.gz: `3213bef5884218f35ad7269e0305041efa7e4d2cfd0f0ca1eda9b6ab71f1af7d`
- Android arm64/x86_64 APK: `9954cc5337d5f8574702303381da71f238c4865fd61c9a6819df314462ea4057`
- Desktop AppImage: `4e4d1343c6394441b712694b546f4b94b47bcb6a052d95aefb3688314c3f494c`
- Desktop Windows executable: `eb68cb096b32db6d8c3c9c85de4144f54377cea90e850342ed0c922b9e86d4f3`

## End-to-end evidence

1. **Actual update, not a simulated install:** released updater 0.1.1 independent worker fetched the official registry and called published Core 0.4.4 packages.install to upgrade Media 0.3.5 to 0.3.6. The job completed, rather than timing out, and a persisted favorite/library sentinel survived.
2. **Published Core + published Media:** scoped process registration, capability routing, independent field merge, tombstone deletion and unrelated-capability denial passed in an isolated loopback profile.
3. **Actual MCP transport:** released MCP 0.1.2 initialized over HTTP, exposed the selected tools, read the persisted Media library through a scoped Core token, denied an unrelated tool and rejected a revoked profile. This supersedes the earlier pre-publication review's MCP-unverified note.
4. **Published Web archive:** remembered scoped login survived real Core, Web and browser process restarts. Revocation still produced a genuine sign-in requirement rather than an endless reconnect. Clean, forged and revoked sessions, anonymous owner calls, signature replay and production demo bypass were denied. Setup token rotation, local-only recovery phrase and HttpOnly remembered login passed.
5. **Media regressions:** 23 TypeScript and 6 API tests; real WAV/WebM decoder checks, seek/reload, shared surfaces, offline service restart, source tombstones, independent favorite/queue mutations and 390/1440 batch-delete/search/library workflows.
6. **Shared UI regressions:** host/workbench/app/i18n/build checks; 2,186-key EN/RU parity; actual auth-gate retention/rejection, immediate update pending, node groups, frame ownership, plugin widget save/reload, Canvas interaction and populated-Canvas performance. Canvas test: 98 frames, median 16.7 ms, p95 16.8 ms at 2400x1600 with a live dashboard at 50% zoom.
7. **Embedding/mobile:** released Media 0.3.6 SDK embedding at 390/1440, isolated storage and root-capability denial. Catalog geometry covers 320/360/390/1440, RU/EN and light/dark, centered 44-pixel mobile controls and no horizontal document overflow. Representative 390-pixel light/dark catalog and active-update screenshots were opened individually. This does not mean every generated frame was visually inspected or that all intentionally scrollable tab strips were removed.
8. **Android signed release on API 35 emulator:** explicit key onboarding, internal navigation remaining in APK, offline portable Media installation/embedded navigation, Keystore ciphertext, explicit background service, background/resume and user restart with persistent profile passed. Representative Media library screenshot inspected individually. No physical-device or battery-behavior guarantee.
9. **Real external YouTube metadata:** search returned 20 items; public playlist returned 13; a channel returned distinct pages of 30 then 60 items with continuation. These checks are not proof of successful external audio/video playback or yt-dlp downloads.

## Root causes and upgrade semantics

- The updater read a flat install version, while Core returns an installed[] result. Success was not recognized and pending entries survived failure. The new parser checks exact package/version identity and terminalizes failed/interrupted jobs, including waiting entries.
- Transient web.info or inventory failures were treated as authentication loss or total Core failure. The UI now retains the authenticated shell and retries; actual unauthorized/revoked credentials still require login.
- Whole-library sync and detached item references let unrelated queue/favorite writes overwrite each other and resurrect deleted identities. Field patches, durable source-identity tombstones and stable decoder references address those tested races.
- A dashboard system/app feed emitted by the UI was missing from Web's accepted schema. The schema now accepts it, with round-trip and save/reload regressions.
- Installing only Media is not the complete host fix. Shared UI ships inside Web/native host releases; updater failure handling requires updater 0.1.1. An already-running old updater process is not retroactively patched by a new registry entry.

## Explicit remaining boundaries

- Requested work was implemented and tested as recorded above, pushed and released, and the headless/Web/shared-UI catalog entries are visible to the updater. Installation on the user's production nodes and physical devices is **not confirmed**.
- External YouTube embed/account/network restrictions and successful yt-dlp downloads remain unverified. No synthetic timeline or audio is used as evidence.
- A real WebM decoder survives hide/restore/resize within its owner surface. Cross-surface video iframe reparenting is not established; do not advertise seamless video migration between all surfaces.
- Studio provides an embedded address/search/navigation surface and an external-browser fallback. Sites can refuse framing; this is not an unrestricted native browser.
- Artist/channel uploads and playlist imports are supported, not a guarantee of a complete YouTube Music discography, every regional catalog or arbitrarily large collections. Item synchronization is bounded into batches; huge queue/playlist payloads can still exceed the bridge limit.
- Field patches prevent tested unrelated-field clobbering; they are not a general distributed CRDT.
- UI Play now and URL-intent playback are independent of the queue. The legacy MCP command path that selects a track ID still resolves it from the queue; API-wide Play-ID parity is not claimed by this release.

## Public references

- [Shared UI CI](https://github.com/n-ycta/delta-ui/actions/runs/39027173783)
- [Desktop native CI](https://github.com/n-ycta/delta-desktop/actions/runs/39027392157)
- [Android build, emulator and publication](https://github.com/n-ycta/delta-android/actions/runs/39027394983)
- [Web release](https://github.com/n-ycta/delta-web/releases/tag/v0.6.18)
- [Media release](https://github.com/n-ycta/delta-media/releases/tag/v0.3.6)
- [Sources release](https://github.com/n-ycta/delta-media-sources/releases/tag/v0.2.5)
- [Updater release](https://github.com/n-ycta/delta-updater/releases/tag/v0.1.1)
- [Desktop release](https://github.com/n-ycta/delta-desktop/releases/tag/v0.1.11)
- [Android release](https://github.com/n-ycta/delta-android/releases/tag/v0.1.11)
