#!/usr/bin/env bash

# Bayesian SSH - signed APT and RPM repositories, served by the docs site at
# https://bssh.abdoufermat.dev/apt and /rpm (the release packs this tree as
# bssh-repo.tar.gz; the bssh-docs deploy unpacks it). Holds the latest release only.
#
# Usage: scripts/build-repo.sh <packages dir> <output dir> <signing key id>
#   The key must be in the gpg keyring, without a passphrase (CI imports REPO_GPG_KEY).
#
# Layout:
#   apt/pool/main/*.deb, apt/dists/stable/{Release,InRelease,Release.gpg}
#   apt/dists/stable/main/binary-{amd64,arm64}/Packages[.gz]
#   rpm/*.rpm (signed), rpm/repodata/ (repomd.xml.asc), rpm/bssh.repo
#   bssh.asc (armored public key, for dnf), bssh.gpg (binary, for apt signed-by)

set -euo pipefail

if [ $# -ne 3 ]; then
    echo "usage: $0 <packages dir> <output dir> <signing key id>" >&2
    exit 1
fi
PKGS="$(cd "$1" && pwd)"
OUT="$2"
KEY="$3"
SITE="https://bssh.abdoufermat.dev"

for tool in apt-ftparchive gpg rpmsign createrepo_c; do
    command -v "$tool" >/dev/null 2>&1 || { echo "missing: $tool" >&2; exit 1; }
done
compgen -G "${PKGS}/*.deb" >/dev/null || { echo "no .deb in ${PKGS}" >&2; exit 1; }
compgen -G "${PKGS}/*.rpm" >/dev/null || { echo "no .rpm in ${PKGS}" >&2; exit 1; }

mkdir -p "${OUT}"
OUT="$(cd "${OUT}" && pwd)"
[ -z "$(ls -A "${OUT}")" ] || { echo "${OUT} is not empty" >&2; exit 1; }
SIGN=(gpg --batch --yes --local-user "${KEY}")

# APT: one suite (stable), one component (main), amd64 and arm64.
mkdir -p "${OUT}/apt/pool/main"
cp "${PKGS}"/*.deb "${OUT}/apt/pool/main/"
(
    cd "${OUT}/apt"
    for arch in amd64 arm64; do
        dir="dists/stable/main/binary-${arch}"
        mkdir -p "${dir}"
        apt-ftparchive --arch "${arch}" packages pool > "${dir}/Packages"
        gzip -9kn "${dir}/Packages"
    done
    apt-ftparchive \
        -o APT::FTPArchive::Release::Origin="Bayesian SSH" \
        -o APT::FTPArchive::Release::Label="Bayesian SSH" \
        -o APT::FTPArchive::Release::Suite=stable \
        -o APT::FTPArchive::Release::Codename=stable \
        -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
        -o APT::FTPArchive::Release::Components=main \
        -o APT::FTPArchive::Release::Description="Bayesian SSH packages (${SITE})" \
        release dists/stable > Release.tmp
    mv Release.tmp dists/stable/Release
    "${SIGN[@]}" --armor --detach-sign -o dists/stable/Release.gpg dists/stable/Release
    "${SIGN[@]}" --clearsign -o dists/stable/InRelease dists/stable/Release
)

# RPM: every package signed, then signed metadata. One repository for both
# architectures: dnf picks the packages matching the machine.
mkdir -p "${OUT}/rpm"
cp "${PKGS}"/*.rpm "${OUT}/rpm/"
# rpm's own idea of the gpg path varies by distribution (it isn't gpg on Ubuntu).
rpmsign --define "__gpg $(command -v gpg)" --define "_gpg_name ${KEY}" --addsign "${OUT}"/rpm/*.rpm >/dev/null
createrepo_c --quiet "${OUT}/rpm"
"${SIGN[@]}" --armor --detach-sign -o "${OUT}/rpm/repodata/repomd.xml.asc" "${OUT}/rpm/repodata/repomd.xml"
cat <<EOF > "${OUT}/rpm/bssh.repo"
[bssh]
name=Bayesian SSH
baseurl=${SITE}/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=${SITE}/bssh.asc
EOF

# Public key, both forms.
gpg --batch --armor --export "${KEY}" > "${OUT}/bssh.asc"
gpg --batch --export "${KEY}" > "${OUT}/bssh.gpg"

echo "✅ repository in ${OUT}:"
(cd "${OUT}" && find . -type f | sort)
