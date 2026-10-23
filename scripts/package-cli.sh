#!/usr/bin/env bash

# Bayesian SSH - CLI-only packages (.deb & .rpm) for servers and headless machines.
# Packages a prebuilt binary, so the release reuses the binaries it already built
# for each architecture instead of compiling again.
#
# Usage: scripts/package-cli.sh <binary> <x86_64|aarch64> <version> <output dir>

set -euo pipefail

if [ $# -ne 4 ]; then
    echo "usage: $0 <binary> <x86_64|aarch64> <version> <output dir>" >&2
    exit 1
fi
BIN="$1"
ARCH="$2"
VERSION="$3"
OUT_DIR="$4"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

PKG_NAME="bayesian-ssh-cli"
# Same rule as package.sh: '~' in the package version, '-' kept in file names.
PKG_VERSION="${VERSION//-/\~}"
case "$ARCH" in
    x86_64) DEB_ARCH="amd64" ;;
    aarch64) DEB_ARCH="arm64" ;;
    *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac
SUMMARY="Bayesian SSH command line (bssh), without the desktop app"
DESCRIPTION="Bayesian SSH (bssh) is a keyboard-first SSH session manager: fuzzy,
usage-ranked search, bastions, Kerberos, file transfer and session history,
with end-to-end encrypted team sharing. This package has the command line only,
for servers and headless machines; 'bayesian-ssh' adds the desktop app."

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "${OUT_DIR}"
OUT_DIR="$(cd "${OUT_DIR}" && pwd)"

# Filesystem layout
ROOT="${STAGE}/root"
install -Dm755 "$BIN" "${ROOT}/usr/bin/bayesian-ssh"
ln -s bayesian-ssh "${ROOT}/usr/bin/bssh"
install -Dm644 "${ROOT_DIR}/LICENSE" "${ROOT}/usr/share/doc/${PKG_NAME}/LICENSE"
install -Dm644 "${ROOT_DIR}/README.md" "${ROOT}/usr/share/doc/${PKG_NAME}/README.md"

# .deb
DEB_ROOT="${STAGE}/deb"
cp -a "${ROOT}" "${DEB_ROOT}"
mkdir -p "${DEB_ROOT}/DEBIAN"
cat <<EOF > "${DEB_ROOT}/DEBIAN/control"
Package: ${PKG_NAME}
Version: ${PKG_VERSION}
Architecture: ${DEB_ARCH}
Maintainer: Abdoufermat5 <abdoufermat5@users.noreply.github.com>
Section: net
Priority: optional
Depends: openssh-client
Conflicts: bayesian-ssh
Replaces: bayesian-ssh
Homepage: https://bssh.abdoufermat.dev
Description: ${SUMMARY}
$(printf '%s\n' "${DESCRIPTION}" | sed 's/^/ /')
EOF
DEB_FILE="${OUT_DIR}/${PKG_NAME}_${VERSION}_${DEB_ARCH}.deb"
dpkg-deb --build --root-owner-group "${DEB_ROOT}" "${DEB_FILE}" >/dev/null
echo "✅ ${DEB_FILE}"

# .rpm (rpmbuild only: the binary is already built, possibly for another architecture,
# so stripping, debuginfo and automatic dependency scanning are turned off, and the
# architecture comes from --target: a BuildArch the host can't build is refused)
if ! command -v rpmbuild >/dev/null 2>&1; then
    echo "ℹ️  rpmbuild not found: skipping the .rpm (install 'rpm')."
    exit 0
fi
TOPDIR="${STAGE}/rpmbuild"
mkdir -p "${TOPDIR}/SPECS"
RPM_FILES="$(cd "${ROOT}" && find . \( -type f -o -type l \) | sed 's|^\.||' | sort)"
cat <<EOF > "${TOPDIR}/SPECS/${PKG_NAME}.spec"
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none
Name:           ${PKG_NAME}
Version:        ${PKG_VERSION}
Release:        1
Summary:        ${SUMMARY}
License:        MIT
URL:            https://bssh.abdoufermat.dev
AutoReqProv:    no
Requires:       openssh-clients
Conflicts:      bayesian-ssh

%description
${DESCRIPTION}

%install
mkdir -p %{buildroot}
cp -a "${ROOT}"/. %{buildroot}/

%files
${RPM_FILES}
EOF
rpmbuild -bb --quiet --target "${ARCH}" --define "_topdir ${TOPDIR}" "${TOPDIR}/SPECS/${PKG_NAME}.spec"
RPM_FILE="${OUT_DIR}/${PKG_NAME}-${VERSION}-1.${ARCH}.rpm"
cp "$(find "${TOPDIR}/RPMS" -name '*.rpm' | head -n 1)" "${RPM_FILE}"
echo "✅ ${RPM_FILE}"
