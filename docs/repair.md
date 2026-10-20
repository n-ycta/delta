# Repair/update an existing Linux node

Run as the same OS user that owns the existing Delta profile (not with sudo):

```sh
curl -fsSL https://raw.githubusercontent.com/n-ycta/delta/main/repair.sh -o /tmp/delta-repair.sh && sh /tmp/delta-repair.sh
```

For a non-default profile, export `DELTA_HOME` first. The command uses the official Core installer and checksum-verified package installer directly; it does not depend on a working Updater plugin. It updates all installed official components and ensures Storage, Web, Type, LifeOS, Updater and Observability are installed. Custom components are left untouched; disabled optional components remain disabled. The normal installer configures user-service autostart. `--no-service` is only for deliberately temporary/test nodes.

Before changing components, a private snapshot of configuration and local data is written under `$DELTA_HOME/backups/repair-*`. External PostgreSQL volumes are not copied by this snapshot. Existing PostgreSQL configuration is preserved: an unavailable database is not silently replaced with an empty file store. If readiness checks fail, repair stops with an error rather than claiming success.

For an existing Vector installation, the old writer is stopped, and the owner authorizes a narrowly scoped, short-lived read/copy migration across the old and new Storage identities. Credentials are never sent to an app iframe. Both the source data and a LifeOS migration backup are preserved; different existing LifeOS data is never overwritten. The old Vector runtime is removed only after LifeOS and its portable app are installed and healthy.

Open Delta and select **LifeOS** in the app switcher/dock, like Media. Its navigation remains inside its own app interface; dashboard widgets can be added separately. Refresh an already-open Console once after Web updates.

Type still requires the initial explicit OS approval for hardware keyboard access. A valid, effective per-user persistent rule is reused after subsequent starts; repair does not bypass OS security or grant access to unrelated devices.

## Native Desktop and Android are separate updates

Updating Web does **not** update an installed native client. Desktop 0.1.13 and Android 0.1.13 pin shared UI 0.3.6 (`f5c27826f6a4529e9bea1bc10c75eabf5604c1ae`), including the stable iframe order fix. Their reviewed portable-app catalog includes LifeOS 0.3.2. Do not uninstall the native application or clear its profile to update it.

Before restarting any old client showing an unsaved-state warning, export its current LifeOS data. The new pending-draft journal cannot recover edits already lost by an old version.

### Linux Desktop

Close Delta Desktop first. Update the existing node using the command above (and the same `DELTA_HOME`, if customized), then update the native binary:

```sh
curl -fsSL https://raw.githubusercontent.com/n-ycta/delta-desktop/main/install.sh -o /tmp/delta-desktop-install.sh && sh /tmp/delta-desktop-install.sh
```

### Windows Desktop

Close Delta Desktop and run the official native installer in PowerShell:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/n-ycta/delta-desktop/main/install.ps1 -OutFile "$env:TEMP\delta-desktop-install.ps1"; & "$env:TEMP\delta-desktop-install.ps1"
```

This updates the client, not a separate remote service node. Update that node separately. Windows builds are checksum-verified but not Authenticode-signed; the installer does not bypass OS protection.

### Android

Install the signed APK from [the latest Android release](https://github.com/n-ycta/delta-android/releases/latest) **over the existing application**, without uninstalling it or clearing storage. Android does not run the desktop Node plugins locally; LifeOS uses the service on the explicitly selected paired trusted node. Update that service node as well.

### Update the existing portable LifeOS app

After updating a native host, open **All apps** (or the app catalog) and update **LifeOS** to 0.3.2. Updating the host's bundled catalog alone does not replace an already-installed older portable app. The Linux repair command already installs/replaces LifeOS in the Web host; native app stores are separate.

## Reliability fixes in these versions

- LifeOS 0.3.2: independent calendar-cell geometry, bounded selector icons, collapsed planner help, pending-draft journal, write acknowledgement reconciliation and stale-read protection.
- Web 0.6.22 / UI 0.3.6: retain iframe DOM ordering across app switches; remove retired Vector catalog metadata with a one-time metadata backup, without deleting its user data.
- The shared FrameHost browser regression passed 20 alternating app switches with the actual LifeOS UI/backend and an inert Media SDK fixture. It is not a physical Android or full Media playback test.
- Published Linux repair was run twice in an isolated profile; the same test task ID/title survived the second update and Storage/LifeOS reported ready/connected. No user's live profile was modified by that test.
