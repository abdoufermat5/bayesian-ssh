#!/usr/bin/env bash
# Enforce the open/closed perimeter: this public repo must never depend on or
# contain code from the private bssh-cloud server.
#
# Fails when:
#   - a manifest or lockfile names bssh-cloud (or its crates, bssh_cloud_*),
#   - Cargo.lock pulls a crate from anywhere other than crates.io,
#   - a source file imports bssh-cloud.
#
# Run by CI and by the pre-push hook in .githooks/.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0
private='bssh[-_]cloud'

manifests=$(git ls-files 'Cargo.toml' '*/Cargo.toml' 'Cargo.lock' \
    'desktop/package.json' 'desktop/package-lock.json')
if hits=$(grep -nEi "$private" $manifests); then
    echo "perimeter: manifests reference the private server:" >&2
    echo "$hits" >&2
    fail=1
fi

if hits=$(grep -n '^source = ' Cargo.lock \
    | grep -v '"registry+https://github.com/rust-lang/crates.io-index"'); then
    echo "perimeter: Cargo.lock has crates from outside crates.io:" >&2
    echo "$hits" >&2
    fail=1
fi

# The Teams client may name the server in docs and user-facing strings, but no
# code may import it. Rust `use`/`extern crate` and JS imports are what matter.
if hits=$(git grep -nEi \
    -e "(use|extern crate)[[:space:]]+$private" \
    -e "(from|require\()[[:space:]]*['\"][^'\"]*$private" \
    -- '*.rs' '*.ts' '*.js' '*.svelte'); then
    echo "perimeter: source imports the private server:" >&2
    echo "$hits" >&2
    fail=1
fi

if [ "$fail" -ne 0 ]; then
    echo "perimeter: see PERIMETER.md rule P1 (dependencies go one way only)" >&2
    exit 1
fi
echo "perimeter: ok"
