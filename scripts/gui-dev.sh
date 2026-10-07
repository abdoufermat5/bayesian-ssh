#!/usr/bin/env bash
# Run the desktop GUI in development mode with Vite hot reload.
#
# The Tauri CLI cannot drive this repo: the Tauri crate lives in crates/gui and
# desktop/src-tauri is a symlink to it, but the CLI finds the app's Cargo
# package by comparing the literal `src-tauri/Cargo.toml` path against cargo's
# canonical manifest path, which never matches through a symlink. So the dev
# server and the app are started here instead.
#
# Frontend edits reload through Vite. Rust edits need a restart.
set -euo pipefail

cd "$(dirname "$0")/.."
repo=$PWD

# npm is often only reachable through nvm.
if ! command -v npm >/dev/null 2>&1 && [ -d "$HOME/.nvm/versions/node" ]; then
  nvm_bin=$(ls -d "$HOME"/.nvm/versions/node/*/bin 2>/dev/null | sort -V | tail -n 1)
  [ -n "$nvm_bin" ] && PATH="$PATH:$nvm_bin"
fi
command -v npm >/dev/null 2>&1 || { echo "gui-dev: npm is required" >&2; exit 1; }

if [ ! -d desktop/node_modules ]; then
  (cd desktop && npm ci)
fi

# npm wraps the server in a shell, so stop both it and our own vite process
# (`.bin/vite` is what actually appears in the process list).
stop_vite() {
  [ -n "${vite_pid:-}" ] && kill "$vite_pid" 2>/dev/null
  pkill -f "$repo/desktop/node_modules/.bin/vite" 2>/dev/null
  return 0
}
on_signal() {
  stop_vite
  exit 130
}
trap stop_vite EXIT
trap on_signal INT TERM

(cd desktop && exec npm run dev) &
vite_pid=$!

for _ in $(seq 1 120); do
  curl -sf -o /dev/null http://localhost:1420/ && break
  if ! kill -0 "$vite_pid" 2>/dev/null; then
    echo "gui-dev: the Vite dev server exited — is port 1420 already in use?" >&2
    exit 1
  fi
  sleep 0.5
done
curl -sf -o /dev/null http://localhost:1420/ || {
  echo "gui-dev: Vite did not come up on http://localhost:1420" >&2
  exit 1
}

echo "gui-dev: Vite is serving http://localhost:1420 — starting the app (Ctrl-C stops both)"
# No default features: that drops `custom-protocol`, which is what makes the
# crate load `build.devUrl` instead of the prebuilt desktop/build assets.
cargo run --no-default-features -p bayesian-ssh-gui
