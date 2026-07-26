#!/usr/bin/env bash

# Bayesian SSH - Unified Packaging Script (.deb & .rpm)
# Creates a single 'bayesian-ssh' package containing both CLI and Desktop GUI.

set -e

# Load NVM or add Node binaries to PATH if available
export NVM_DIR="${HOME}/.nvm"
if [ -s "${NVM_DIR}/nvm.sh" ]; then
    . "${NVM_DIR}/nvm.sh"
elif [ -d "${HOME}/.nvm/versions/node" ]; then
    LATEST_NODE="$(ls -d "${HOME}/.nvm/versions/node"/v* 2>/dev/null | sort -V | tail -n 1)/bin"
    if [ -d "${LATEST_NODE}" ]; then
        export PATH="${LATEST_NODE}:${PATH}"
    fi
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

PKG_NAME="bayesian-ssh"
VERSION="$(grep '^version = ' "${ROOT_DIR}/crates/cli/Cargo.toml" | head -n 1 | cut -d'"' -f2)"
ARCH="$(uname -m)"
if [ "$ARCH" = "x86_64" ]; then
    DEB_ARCH="amd64"
    RPM_ARCH="x86_64"
elif [ "$ARCH" = "aarch64" ]; then
    DEB_ARCH="arm64"
    RPM_ARCH="aarch64"
else
    DEB_ARCH="$ARCH"
    RPM_ARCH="$ARCH"
fi

BUILD_DIR="${ROOT_DIR}/target/package-stage"
OUT_DIR="${ROOT_DIR}/target/packages"

echo "=========================================="
echo "🚀 Packaging ${PKG_NAME} v${VERSION} (${DEB_ARCH})"
echo "=========================================="

# 1. Build release binaries
echo "🔨 Step 1/4: Building Rust CLI release binaries..."
cd "${ROOT_DIR}"
cargo build --release --workspace

echo "🔨 Step 2/4: Building Desktop GUI release binary..."
if [ -d "${ROOT_DIR}/desktop" ] && command -v npm >/dev/null 2>&1; then
    cd "${ROOT_DIR}/desktop"
    if [ ! -d "node_modules" ]; then
        echo "📦 Installing npm dependencies for desktop..."
        npm ci || npm install
    fi
    npm run tauri build -- --config ../crates/gui/tauri.conf.json --no-bundle
else
    echo "⚠️  Desktop directory or npm not found, skipping GUI build step (or using existing desktop binary)."
fi

# 2. Stage filesystem layout
echo "📁 Step 3/4: Staging package filesystem layout..."
rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}/usr/bin"
mkdir -p "${BUILD_DIR}/usr/share/applications"
mkdir -p "${BUILD_DIR}/usr/share/icons/hicolor/128x128/apps"
mkdir -p "${BUILD_DIR}/usr/share/doc/${PKG_NAME}"

# Copy CLI binaries
cp "${ROOT_DIR}/target/release/bayesian-ssh" "${BUILD_DIR}/usr/bin/bayesian-ssh"
chmod +x "${BUILD_DIR}/usr/bin/bayesian-ssh"

ln -sf bayesian-ssh "${BUILD_DIR}/usr/bin/bssh"

# Copy Desktop binary if built (Cargo workspace puts all binaries in root target/release)
DESKTOP_BIN="${ROOT_DIR}/target/release/bayesian-ssh-desktop"
if [ ! -f "${DESKTOP_BIN}" ]; then
    DESKTOP_BIN="${ROOT_DIR}/target/release/desktop"
fi

if [ -f "${DESKTOP_BIN}" ]; then
    cp "${DESKTOP_BIN}" "${BUILD_DIR}/usr/bin/bayesian-ssh-desktop"
    chmod +x "${BUILD_DIR}/usr/bin/bayesian-ssh-desktop"
fi

# Copy Icon and Desktop entry
if [ -f "${ROOT_DIR}/crates/gui/icons/128x128.png" ]; then
    cp "${ROOT_DIR}/crates/gui/icons/128x128.png" "${BUILD_DIR}/usr/share/icons/hicolor/128x128/apps/bayesian-ssh-desktop.png"
fi

# Copy License and README
if [ -f "${ROOT_DIR}/LICENSE" ]; then
    cp "${ROOT_DIR}/LICENSE" "${BUILD_DIR}/usr/share/doc/${PKG_NAME}/LICENSE"
fi
if [ -f "${ROOT_DIR}/README.md" ]; then
    cp "${ROOT_DIR}/README.md" "${BUILD_DIR}/usr/share/doc/${PKG_NAME}/README.md"
fi

# Create Desktop launcher file
cat <<EOF > "${BUILD_DIR}/usr/share/applications/bayesian-ssh-desktop.desktop"
[Desktop Entry]
Name=Bayesian SSH
Comment=Fast and lightweight SSH session manager with Bayesian search, Kerberos, and bastion support
Exec=/usr/bin/bayesian-ssh-desktop
Icon=bayesian-ssh-desktop
Terminal=false
Type=Application
Categories=Development;Network;System;
StartupNotify=true
EOF
chmod 644 "${BUILD_DIR}/usr/share/applications/bayesian-ssh-desktop.desktop"

# 3. Create .deb package
echo "📦 Step 4/4: Building Debian package (.deb)..."
mkdir -p "${OUT_DIR}"
DEB_STAGE="${BUILD_DIR}_deb"
rm -rf "${DEB_STAGE}"
cp -r "${BUILD_DIR}" "${DEB_STAGE}"

mkdir -p "${DEB_STAGE}/DEBIAN"
cat <<EOF > "${DEB_STAGE}/DEBIAN/control"
Package: ${PKG_NAME}
Version: ${VERSION}
Architecture: ${DEB_ARCH}
Maintainer: Abdoufermat5 <abdoufermat5@users.noreply.github.com>
Section: utils
Priority: optional
Homepage: https://github.com/abdoufermat5/bayesian-ssh
Description: Fast and lightweight SSH session manager with Kerberos support
 Bayesian SSH (bssh) is a smart, high-performance SSH session manager featuring
 Bayesian-ranked search, bastion jump-host chaining, Kerberos integration,
 TUI, and a desktop graphical user interface.
EOF

# postinst script for icon cache
cat <<'EOF' > "${DEB_STAGE}/DEBIAN/postinst"
#!/bin/sh
set -e
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
fi
EOF
chmod 755 "${DEB_STAGE}/DEBIAN/postinst"

DEB_FILE="${OUT_DIR}/${PKG_NAME}_${VERSION}_${DEB_ARCH}.deb"
dpkg-deb --build --root-owner-group "${DEB_STAGE}" "${DEB_FILE}"
echo "✅ Debian package created: ${DEB_FILE}"

# 4. Create .rpm package (if rpmbuild or fpm is available)
if command -v rpmbuild >/dev/null 2>&1; then
    echo "📦 Building RPM package (.rpm)..."
    RPM_TOPDIR="${ROOT_DIR}/target/rpmbuild"
    rm -rf "${RPM_TOPDIR}"
    mkdir -p "${RPM_TOPDIR}/"{BUILD,RPMS,SOURCES,SPECS,SRPMS}

    # Generate RPM spec file
    SPEC_FILE="${RPM_TOPDIR}/SPECS/${PKG_NAME}.spec"
    cat <<EOF > "${SPEC_FILE}"
Name:           ${PKG_NAME}
Version:        ${VERSION}
Release:        1%{?dist}
Summary:        Fast and lightweight SSH session manager with Kerberos support
License:        MIT
URL:            https://github.com/abdoufermat5/bayesian-ssh
BuildArch:      ${RPM_ARCH}

%description
Bayesian SSH (bssh) is a smart, high-performance SSH session manager featuring
Bayesian-ranked search, bastion jump-host chaining, Kerberos integration,
TUI, and a desktop graphical user interface.

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a ${BUILD_DIR}/* %{buildroot}/

%post
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
fi

%files
/usr/bin/bayesian-ssh
/usr/bin/bssh
/usr/bin/bayesian-ssh-desktop
/usr/share/applications/bayesian-ssh-desktop.desktop
/usr/share/icons/hicolor/128x128/apps/bayesian-ssh-desktop.png
/usr/share/doc/${PKG_NAME}/*

%changelog
* Sun Jul 26 2026 Abdoufermat5 <abdoufermat5@users.noreply.github.com> - ${VERSION}-1
- Release ${VERSION}
EOF

    rpmbuild -bb --define "_topdir ${RPM_TOPDIR}" "${SPEC_FILE}"
    RPM_RESULT="$(find "${RPM_TOPDIR}/RPMS" -name "*.rpm" | head -n 1)"
    if [ -n "${RPM_RESULT}" ]; then
        cp "${RPM_RESULT}" "${OUT_DIR}/"
        echo "✅ RPM package created: ${OUT_DIR}/$(basename "${RPM_RESULT}")"
    fi
elif command -v fpm >/dev/null 2>&1; then
    echo "📦 Building RPM package using FPM..."
    fpm -s dir -t rpm -n "${PKG_NAME}" -v "${VERSION}" \
        -a "${RPM_ARCH}" \
        --description "Fast and lightweight SSH session manager with Kerberos support" \
        --url "https://github.com/abdoufermat5/bayesian-ssh" \
        -C "${BUILD_DIR}" \
        -p "${OUT_DIR}/${PKG_NAME}-${VERSION}-1.${RPM_ARCH}.rpm"
    echo "✅ RPM package created: ${OUT_DIR}/${PKG_NAME}-${VERSION}-1.${RPM_ARCH}.rpm"
else
    echo "ℹ️  rpmbuild or fpm not found. Skipping RPM creation (only .deb created)."
    echo "    To build RPMs locally, install 'rpm' (e.g. sudo apt install rpm)."
fi

echo "=========================================="
echo "🎉 Packaging Complete!"
echo "Outputs in: ${OUT_DIR}"
ls -lh "${OUT_DIR}"
echo "=========================================="
