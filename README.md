<p align="center">
  <img src="assets/banner.svg" alt="Bayesian SSH: every host, one keystroke away" width="100%">
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

## Features

<table>
  <tr>
    <td width="50%" valign="top">
      <img src="assets/icon-search.svg" width="28" height="28" align="left" alt="">
      &nbsp;<strong>Finds the host you mean</strong><br>
      &nbsp;Type part of a name, a tag or an abbreviation. Hosts are ranked by how often, how recently and in what context you use them; <code>bssh why</code> explains a ranking.
    </td>
    <td width="50%" valign="top">
      <img src="assets/icon-terminal.svg" width="28" height="28" align="left" alt="">
      &nbsp;<strong>Command line and desktop</strong><br>
      &nbsp;The same hosts in <code>bssh</code> and in the desktop app: terminal tabs, file transfer, a command palette.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="assets/icon-bastion.svg" width="28" height="28" align="left" alt="">
      &nbsp;<strong>Bastions and Kerberos</strong><br>
      &nbsp;Jump hosts per connection or by default, <code>ProxyJump</code> picked up from <code>~/.ssh/config</code>, Kerberos tickets handled for you.
    </td>
    <td width="50%" valign="top">
      <img src="assets/icon-fleet.svg" width="28" height="28" align="left" alt="">
      &nbsp;<strong>Every host at once</strong><br>
      &nbsp;Run a command or upload files across every host with a tag, in parallel, with one summary.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="assets/icon-history.svg" width="28" height="28" align="left" alt="">
      &nbsp;<strong>Session history</strong><br>
      &nbsp;Who connected where, for how long, and how it ended. Sessions can be recorded and replayed.
    </td>
    <td width="50%" valign="top">
      <img src="assets/icon-vault.svg" width="28" height="28" align="left" alt="">
      &nbsp;<strong>bssh Teams</strong><br>
      &nbsp;Share connections with your team through a vault encrypted on your devices, with a signed audit log.
    </td>
  </tr>
</table>

## Install

```bash
curl -fsSL https://bssh.abdoufermat.dev/install.sh | bash
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
