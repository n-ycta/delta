#!/bin/sh
# Update the existing local profile using verified official packages, without Updater.
set -eu
umask 077
TMP=$(mktemp -d);trap 'rm -rf "$TMP"' EXIT HUP INT TERM
BASE=https://raw.githubusercontent.com/n-ycta/delta/main
curl -fLsS "$BASE/install.sh" -o "$TMP/install.sh"
# Boot/update the Core runtime, without reinstalling or resetting the profile.
sh "$TMP/install.sh" --plugins '' "$@"
curl -fLsS "$BASE/scripts/migrate-lifeos.mjs" -o "$TMP/migrate-lifeos.mjs"
curl -fLsS "$BASE/scripts/repair-install.mjs" -o "$TMP/repair-install.mjs"
"${DELTA_HOME:-$HOME/.delta}/runtime/bin/node" "$TMP/repair-install.mjs"
