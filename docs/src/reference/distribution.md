# Distribution

How Bayesian SSH reaches each channel, which repository secret or variable turns
it on, and how to set that channel up for the first time.

Every channel is optional and independent. A tag push with none of the secrets
below set behaves exactly like the first release: it builds both architectures,
publishes the GitHub release, `SHA256SUMS` and its signature, and skips the
rest — each skipped job leaves a `::notice::` in the run summary saying which
secret is missing. Nothing here requires a secret to exist, and no secret is
ever echoed to the log.

## Channels at a glance

| Channel | Job | Turns on with | Publishes |
|---|---|---|---|
| GitHub release | `build`, `packages`, `release` | `GITHUB_TOKEN` (built in) | Binaries, bundles, unified `.deb`/`.rpm`, `SHA256SUMS`, provenance attestations, and `latest.json` when updater keys are set |
| In-app updater | `gate`, `build` | `TAURI_SIGNING_PRIVATE_KEY` secret **and** `BAYESIAN_SSH_UPDATER_PUBLIC_KEY` variable | Signed updater artifacts and `latest.json` |
| Unified packages | `packages` | none | `bayesian-ssh_<version>_amd64.deb` and `.rpm` combining the CLI and the desktop GUI |
| Snap Store | `snap` | `SNAPCRAFT_STORE_CREDENTIALS` | An amd64 + arm64 snap on the Snap Store (`stable`, or `candidate` for prereleases) |
| Flatpak / Flathub | — (manual) | none | A PR to Flathub from the manifest in the repo |

A tag with a pre-release suffix (`v2.6.0-rc.1`) is published as a GitHub
pre-release, and its snap goes to the `candidate` channel instead of `stable`.
Everything else is identical.

All Linux builds run on `ubuntu-24.04` / `ubuntu-24.04-arm` pinned runners: the
snap base (`core24`) must ship the glibc those binaries were linked against.

## Release signing

Every file in the release is covered twice:

- **Build provenance** — `actions/attest-build-provenance@v2` records a
  Sigstore attestation for each asset (`id-token: write`, `attestations: write`).
  It proves the file was built by this workflow at this ref.
- **Keyless blob signing** — `cosign sign-blob` signs `SHA256SUMS` and stores
  the bundle as `SHA256SUMS.sigstore.json`. The signing identity is the workflow
  at the tag, recorded in the Rekor transparency log, so there is no key to
  store or rotate.

Verify a download before installing it:

```bash
# 1. The checksum manifest itself is signed by this repository's release workflow.
cosign verify-blob --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github\.com/abdoufermat5/bayesian-ssh/\.github/workflows/release\.yml@refs/(tags/v.+|heads/main)$' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS

# 2. Every file you downloaded matches the signed manifest.
sha256sum --ignore-missing -c SHA256SUMS

# 3. Optional: check a single file's build provenance.
gh attestation verify bayesian-ssh-linux-x86_64.tar.gz --repo abdoufermat5/bayesian-ssh
```

`install.sh` runs the `cosign verify-blob` check automatically when `cosign` is
installed, and always verifies `SHA256SUMS` with `sha256sum -c`.

## In-app updater

The desktop app checks for updates under **Settings → Updates** and installs
them in place. Only the formats the updater can replace in place are updatable:
the `.AppImage` and the `bayesian-ssh-desktop` `.deb`/`.rpm` from the GitHub
release (the Tauri bundler stamps the bundle type into the binary). Everything
else reports who owns updates and never even registers the updater plugin:

- the **snap** — read-only squashfs, the Snap Store delivers updates; update
  with `sudo snap refresh bayesian-ssh`;
- **raw binaries** (`install.sh`, release tarballs), the **unified
  `bayesian-ssh` `.deb`/`.rpm`**, and **source builds** — updating would
  overwrite them with an AppImage or install a second, conflicting package, so
  the app says to update the way it was installed.

Installing an update closes open terminal sessions and restarts the app. The
downloaded artifact is verified against the minisign public key embedded at
compile time; there is no unsigned or default-key path.

### Keys

Generate the updater keypair once:

```bash
npx tauri signer generate -w ~/.tauri/bayesian-ssh-updater.key
```

Keep the private key offline (the maintainer's copy is never in the repo) and
configure the two sides:

- secret `TAURI_SIGNING_PRIVATE_KEY` — the private key content;
- secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — its passphrase;
- variable `BAYESIAN_SSH_UPDATER_PUBLIC_KEY` — the public key content (a single
  line).

The `gate` job enforces that the signing key and the public key are set
together or left empty together: a signing key without a matching public key
would publish artifacts no installed app could ever verify, so the release fails
early instead. The public key is only written into the build when both sides are
present; otherwise the release is published without updater artifacts or
`latest.json` and a `::notice::` says so.

### Rotating the key

1. Generate a new keypair with `npx tauri signer generate`.
2. Replace the `TAURI_SIGNING_PRIVATE_KEY` /
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secrets and the
   `BAYESIAN_SSH_UPDATER_PUBLIC_KEY` variable with the new values.

Old installs only trust the key compiled into them, so rotation strands them:
they keep checking for updates with the old public key, the new signatures do
not verify, and they can never install a release signed with the new key. They
must reinstall a build that embeds the new key before in-app updates work again.

## Snap Store

Users install and wire up the snap with:

```bash
sudo snap install bayesian-ssh
sudo snap connect bayesian-ssh:ssh-keys   # read-only ~/.ssh and /etc/ssh
sudo snap alias bayesian-ssh bssh         # optional short alias
```

The snap is built for amd64 and arm64 on a `core24` base under strict
confinement. It ships two apps: `bayesian-ssh` (the CLI) and
`bayesian-ssh.gui` (the desktop app, also in the app menu).

Confinement limits:

- `~/.ssh` is mounted read-only, so `known_hosts` is not updated and
  `key generate` cannot write keys into `~/.ssh`.
- The app's config and database live under
  `~/snap/bayesian-ssh/current/.config/bayesian-ssh` and are **not** shared with
  a native install or with `install.sh`.
- `ssh`, `kinit` and `klist` are the snap's own copies. Kerberos tickets created
  on the host in `/tmp` are not visible; run `kinit` inside the snap instead.
- The in-app updater is disabled in the snap — the Snap Store owns updates.

Publishing needs a store credential. One-time setup:

```bash
sudo snap install snapcraft --classic   # if snapcraft is not installed yet
snapcraft login
snapcraft register bayesian-ssh         # also reserves the name (global)

snapcraft export-login --snaps=bayesian-ssh \
  --acls package_access,package_push,package_update,package_release - \
  | gh secret set SNAPCRAFT_STORE_CREDENTIALS
```

`release.yml` calls `.github/workflows/snap.yml` after publishing every release.
To package an already-released version (or rebuild a tag), dispatch it directly:

```bash
gh workflow run snap.yml -f tag=v2.6.0
```

The snap repackages the published release assets rather than rebuilding:
`scripts/snap-stage.sh` downloads the CLI tarball and the desktop `.deb`, checks
them against the release's `SHA256SUMS`, and renders the `__VERSION__` token in
`packaging/snap/snapcraft.yaml`. Build it locally the same way:

```bash
scripts/snap-stage.sh v2.6.0 amd64        # or: make snap-build TAG=v2.6.0
cd target/snap && snapcraft pack
```

Without `SNAPCRAFT_STORE_CREDENTIALS` the snap is still built and kept as a
workflow artifact; the publish step leaves a `::notice::` instead.

## Flatpak (Flathub)

Flatpak is the one channel with no automation secret: Flathub builds from a
manifest in its own repository, so each release is a pull request. The manifest
lives at `packaging/flatpak/com.bayesianssh.App.yml`, with the desktop entry in
`com.bayesianssh.App.desktop` and the AppStream metadata in
`com.bayesianssh.App.metainfo.xml`.

Per release, in the manifest: point the `.deb` and desktop-entry URLs at the new
tag and update their `sha256` values, then open a PR against the Flathub app
repository. Test locally with
`flatpak-builder --force-clean --ccache --install-deps-from=flathub target/flatpak-build packaging/flatpak/com.bayesianssh.App.yml`
(or `make flatpak-build`).

## Cutting a release

The `gate` job refuses a tag whose version does not match the app, so bump all
four version files first:

- `crates/cli/Cargo.toml`
- `crates/gui/Cargo.toml`
- `crates/gui/tauri.conf.json`
- `desktop/package.json`

Then refresh the lockfile and move the changelog:

```bash
cargo update -w                 # refresh crates/* version entries in Cargo.lock
# Roll the CHANGELOG's [Unreleased] section into "## [X.Y.Z] - <date>".

git add -A
git commit -m "Release vX.Y.Z"
git tag vX.Y.Z
git push --follow-tags
```

The tag runs the gate checks, builds both architectures, folds the unified
`.deb`/`.rpm` into the release, signs and publishes everything, and then runs
whichever optional channels are configured. Keep the version numbers in the
CHANGELOG, README and docs examples in step with the tag.
