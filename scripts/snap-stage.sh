#!/usr/bin/env bash
# Stage a snap project from the assets of a published GitHub release.
#
# The snap repackages what the release already ships instead of rebuilding:
# the CLI binary (bayesian-ssh-linux-<arch>.tar.gz) and the desktop .deb
# (bayesian-ssh-desktop-linux-<arch>.deb). Both are checked against the
# release's SHA256SUMS before use.
#
#   scripts/snap-stage.sh <tag> <amd64|arm64> [out-dir]
#
# out-dir (default target/snap) receives snapcraft.yaml with __VERSION__
# rendered, cli/bayesian-ssh, bayesian-ssh-desktop.deb and the store icon;
# run `snapcraft pack` there. Needs an authenticated `gh` (GH_TOKEN in CI).
set -euo pipefail

TAG="${1:?usage: $0 <tag> <amd64|arm64> [out-dir]}"
SNAP_ARCH="${2:?usage: $0 <tag> <amd64|arm64> [out-dir]}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${3:-${ROOT_DIR}/target/snap}"
REPO="${GITHUB_REPOSITORY:-abdoufermat5/bayesian-ssh}"

# The tag reaches sed and download patterns (it is user input on dispatch).
VERSION="${TAG#v}"
case "$VERSION" in
    '' | *[!0-9A-Za-z.+-]*)
        echo "invalid release tag: $TAG" >&2
        exit 1
        ;;
esac

case "$SNAP_ARCH" in
    amd64) ASSET_ARCH=x86_64 ;;
    arm64) ASSET_ARCH=aarch64 ;;
    *)
        echo "unsupported snap architecture: $SNAP_ARCH (expected amd64 or arm64)" >&2
        exit 1
        ;;
esac

CLI_ASSET="bayesian-ssh-linux-${ASSET_ARCH}.tar.gz"
DEB_ASSET="bayesian-ssh-desktop-linux-${ASSET_ARCH}.deb"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/download" "$OUT_DIR/cli"

gh release download "$TAG" -R "$REPO" --dir "$OUT_DIR/download" \
    --pattern "$CLI_ASSET" --pattern "$DEB_ASSET" --pattern SHA256SUMS

# Every staged asset must have a matching SHA256SUMS line; --ignore-missing
# alone would pass if an asset were absent from the manifest.
(
    cd "$OUT_DIR/download"
    for asset in "$CLI_ASSET" "$DEB_ASSET"; do
        if ! awk -v f="$asset" '$2 == f || $2 == "*" f { found = 1 } END { exit !found }' SHA256SUMS; then
            echo "no checksum for $asset in SHA256SUMS of $TAG" >&2
            exit 1
        fi
    done
    sha256sum --check --ignore-missing --strict SHA256SUMS
)

tar -xzf "$OUT_DIR/download/$CLI_ASSET" -C "$OUT_DIR/download"
install -m 755 "$OUT_DIR/download/bayesian-ssh-linux-${ASSET_ARCH}" "$OUT_DIR/cli/bayesian-ssh"
mv "$OUT_DIR/download/$DEB_ASSET" "$OUT_DIR/bayesian-ssh-desktop.deb"
rm -rf "$OUT_DIR/download"

# Store listing icon (512x512); snapcraft.yaml `icon:` must be inside the project.
cp "$ROOT_DIR/crates/gui/icons/icon.png" "$OUT_DIR/bayesian-ssh.png"

sed "s|__VERSION__|${VERSION}|g" "$ROOT_DIR/packaging/snap/snapcraft.yaml" > "$OUT_DIR/snapcraft.yaml"
if grep -q '__VERSION__' "$OUT_DIR/snapcraft.yaml"; then
    echo "the __VERSION__ token was not replaced" >&2
    exit 1
fi

echo "Staged bayesian-ssh ${VERSION} (${SNAP_ARCH}) in ${OUT_DIR}"
