#!/bin/sh
# Delta official installer. Download, inspect, then execute if you prefer not to pipe a script.
set -eu
DELTA_VERSION=${DELTA_VERSION:-v0.4.4}
NODE_VERSION=v24.14.1
HOME_DIR=${DELTA_HOME:-"$HOME/.delta"}
export DELTA_HOME="$HOME_DIR"
if [ "${1:-}" = "--choose" ]; then
 shift
 printf 'Install [1] node + web (default), [2] node + web + typing, [3] bare node: ' >/dev/tty
 read -r CHOICE </dev/tty
 case "$CHOICE" in 2) set -- --plugins storage,web,typing "$@";; 3) set -- --plugins '' "$@";; *) set -- --plugins storage,web "$@";; esac
fi
# Reuse a verified release only at the requested version; an old bootstrap must
# never silently bypass an installer upgrade. DELTA_FORCE_DOWNLOAD repairs it.
if [ "$(cat "$HOME_DIR/bin/release-version" 2>/dev/null || true)" = "$DELTA_VERSION" ] && [ "${DELTA_FORCE_DOWNLOAD:-0}" != 1 ] && [ -x "$HOME_DIR/bin/delta" ] && [ -x "$HOME_DIR/runtime/bin/node" ] && [ -f "$HOME_DIR/bin/bootstrap.mjs" ]; then
 exec "$HOME_DIR/runtime/bin/node" "$HOME_DIR/bin/bootstrap.mjs" "$@"
fi
case "$(uname -s)" in Linux) ;; *) echo 'This installer supports Linux. Use install.ps1 on Windows.' >&2; exit 1;; esac
case "$(uname -m)" in x86_64|amd64) ARCH=x64;; aarch64|arm64) ARCH=arm64;; *) echo 'Unsupported CPU architecture' >&2; exit 1;; esac
for cmd in curl tar sha256sum; do command -v "$cmd" >/dev/null || { echo "Required utility missing: $cmd" >&2; exit 1; }; done
umask 077
TMP=$(mktemp -d);trap 'rm -rf "$TMP"' EXIT HUP INT TERM
mkdir -p "$HOME_DIR/bin" "$HOME_DIR/runtime"
BASE="https://github.com/n-ycta/delta/releases/download/$DELTA_VERSION"
ASSET="delta-linux-$ARCH.tar.gz"
curl -fLsS "$BASE/$ASSET" -o "$TMP/$ASSET"
curl -fLsS "$BASE/SHA256SUMS" -o "$TMP/sums"
(cd "$TMP"; grep "  $ASSET\$" sums | sha256sum -c -)
tar -xzf "$TMP/$ASSET" -C "$TMP"
install -m 700 "$TMP/delta" "$HOME_DIR/bin/delta"
cp "$TMP/bootstrap.mjs" "$TMP/upgrade.mjs" "$HOME_DIR/bin/"
# A private Node runtime makes installation independent of system Node/npm versions.
NODE_ASSET="node-$NODE_VERSION-linux-$ARCH.tar.xz"
curl -fLsS "https://nodejs.org/dist/$NODE_VERSION/$NODE_ASSET" -o "$TMP/$NODE_ASSET"
curl -fLsS "https://nodejs.org/dist/$NODE_VERSION/SHASUMS256.txt" -o "$TMP/node-sums"
(cd "$TMP"; grep "  $NODE_ASSET\$" node-sums | sha256sum -c -)
# Do not overwrite an executable used by running plugins (ETXTBSY on Linux).
mkdir "$TMP/runtime"
tar -xJf "$TMP/$NODE_ASSET" --strip-components=1 -C "$TMP/runtime"
OLD_RUNTIME="$HOME_DIR/.runtime-old.$$"
mv "$HOME_DIR/runtime" "$OLD_RUNTIME"
if mv "$TMP/runtime" "$HOME_DIR/runtime"; then
 rm -rf "$OLD_RUNTIME"
else
 mv "$OLD_RUNTIME" "$HOME_DIR/runtime"
 exit 1
fi
printf '%s\n' "$DELTA_VERSION" > "$HOME_DIR/bin/release-version"
export PATH="$HOME_DIR/runtime/bin:$HOME_DIR/bin:$PATH"
export DELTA_NODE_BINARY="$HOME_DIR/runtime/bin/node" DELTA_NPM_BINARY="$HOME_DIR/runtime/bin/npm"

rm -rf "$TMP"
trap - EXIT HUP INT TERM
exec "$DELTA_NODE_BINARY" "$HOME_DIR/bin/bootstrap.mjs" "$@"
