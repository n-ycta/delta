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
