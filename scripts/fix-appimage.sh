#!/usr/bin/env bash
# Repack a Tauri-built AppImage so every file in it is readable (and
# executable where it should be) by any user.
#
# linuxdeploy, which Tauri runs, packs AppRun.wrapped as 0770 and the usr/ and
# apprun-hooks/ directories as 0700. The AppImage then starts only for the
# user who built it: the AppImage catalog test (and any sandbox that runs it
# under another uid) fails with "AppRun.wrapped: Permission denied". Tauri
# gives no hook between building the AppDir and squashing it, so extract,
# fix the modes and squash again with appimagetool.
#
#   scripts/fix-appimage.sh <file.AppImage> <x86_64|aarch64>
#
# The file is replaced in place. Any updater signature (<file>.sig) is stale
# afterwards: sign the result again.
set -euo pipefail

APPIMAGE="${1:?usage: $0 <file.AppImage> <x86_64|aarch64>}"
ARCH="${2:?usage: $0 <file.AppImage> <x86_64|aarch64>}"

APPIMAGETOOL_VERSION=1.9.1
case "$ARCH" in
    x86_64) APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 ;;
    aarch64) APPIMAGETOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158 ;;
    *)
        echo "unsupported arch: $ARCH" >&2
        exit 1
        ;;
esac

APPIMAGE="$(realpath "$APPIMAGE")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

TOOL="${WORK}/appimagetool"
curl -fsSL -o "$TOOL" \
    "https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-${ARCH}.AppImage"
echo "${APPIMAGETOOL_SHA256}  ${TOOL}" | sha256sum -c --quiet
chmod +x "$TOOL"

# --appimage-extract is handled by the AppImage runtime and needs no FUSE.
chmod +x "$APPIMAGE"
(cd "$WORK" && "$APPIMAGE" --appimage-extract >/dev/null)

chmod -R a+rX "${WORK}/squashfs-root"
chmod a+x "${WORK}/squashfs-root/AppRun" "${WORK}/squashfs-root/AppRun.wrapped"

# No FUSE on CI runners: let appimagetool run from its extracted self.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$ARCH" "$TOOL" --no-appstream \
    "${WORK}/squashfs-root" "${WORK}/out.AppImage" >/dev/null
mv "${WORK}/out.AppImage" "$APPIMAGE"
chmod +x "$APPIMAGE"
