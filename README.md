<p align="center">
  <img src="assets/banner.svg" alt="Bayesian SSH Banner" width="100%">
</p>

# Bayesian SSH

[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![CI](https://github.com/abdoufermat5/bayesian-ssh/workflows/CI/badge.svg)](https://github.com/abdoufermat5/bayesian-ssh/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-bssh.abdoufermat.dev-blue.svg)](https://bssh.abdoufermat.dev)

An SSH session manager for the terminal and the desktop. Type part of a name and it connects:
search ranks your connections by how often and how recently you use them, and it handles
bastions, Kerberos, file transfers and session history. **bssh Teams** shares connections
with your team, end-to-end encrypted.

**Documentation: [bssh.abdoufermat.dev](https://bssh.abdoufermat.dev)**

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/abdoufermat5/bayesian-ssh/main/install.sh | bash
```

Add `-s -- --no-gui` for the CLI only. `.deb`, `.rpm`, AppImage and checksums are on the
[releases page](https://github.com/abdoufermat5/bayesian-ssh/releases); there is also a
[snap](https://snapcraft.io/bayesian-ssh) (`sudo snap install bayesian-ssh`). Other methods,
release signature checks and updates: [Installation](https://bssh.abdoufermat.dev/start/installation/).

## Quick start

```bash
bssh import                          # bring in hosts from ~/.ssh/config
bssh add web-prod web.example.com -u deploy
bssh connect webprod                 # fuzzy: finds web-prod
bssh upload -t prod ./app.tar.gz /opt/app/
bssh history --days 7
bayesian-ssh-desktop                 # the desktop app
```

`bssh` is the short name for `bayesian-ssh`. Next: the [quick start](https://bssh.abdoufermat.dev/start/quick-start/),
the [guides](https://bssh.abdoufermat.dev/guides/connections/), [bssh Teams](https://bssh.abdoufermat.dev/teams/overview/)
and the [CLI reference](https://bssh.abdoufermat.dev/reference/cli/).

## Build from source

```bash
git clone https://github.com/abdoufermat5/bayesian-ssh.git && cd bayesian-ssh
make release && make install         # `make package` builds .deb and .rpm
```

## Contributing

Pull requests are welcome; open an issue first for anything large. Release notes are in
[CHANGELOG.md](CHANGELOG.md), and the documentation source is in
[bssh-docs](https://github.com/abdoufermat5/bssh-docs).

## License

MIT, see [LICENSE](LICENSE).
