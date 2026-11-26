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

mapfile -t manifests < <(git ls-files 'Cargo.toml' '*/Cargo.toml' 'Cargo.lock' \
    'desktop/package.json' 'desktop/package-lock.json')
if [ "${#manifests[@]}" -gt 0 ] && hits=$(grep -nEi "$private" "${manifests[@]}"); then
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

if [ -f desktop/package-lock.json ] && hits=$(grep -nE '"resolved"[[:space:]]*:' desktop/package-lock.json \
    | grep -Ev '"resolved"[[:space:]]*:[[:space:]]*"https://registry\.npmjs\.org/'); then
    echo "perimeter: desktop/package-lock.json has non-npmjs resolved URLs:" >&2
    echo "$hits" >&2
    fail=1
fi

repo_root="$(pwd -P)"
mapfile -t cargo_tomls < <(git ls-files 'Cargo.toml' '*/Cargo.toml')
for toml in "${cargo_tomls[@]}"; do
    toml_dir="$(dirname "$toml")"
    while IFS= read -r rel_path; do
        [ -n "$rel_path" ] || continue
        resolved="$(realpath -m "${toml_dir}/${rel_path}")"
        case "$resolved" in
            "$repo_root"|"$repo_root"/*) ;;
            *)
                echo "perimeter: ${toml} has out-of-tree path dependency: ${rel_path}" >&2
                fail=1
                ;;
        esac
    done < <(sed -nE 's/.*\bpath[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' "$toml")
done

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
