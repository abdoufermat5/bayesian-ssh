# Security Policy

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through GitHub:
[Report a vulnerability](https://github.com/abdoufermat5/bayesian-ssh/security/advisories/new).

You'll get an acknowledgement within 3 working days. Once a fix is released,
the advisory is published with credit to you, unless you'd rather stay anonymous.

## Supported versions

Only the latest minor release gets security fixes. Release assets are signed;
see "Verify a release" in the [README](README.md).

## What bssh stores, and where

Everything stays on your machine. bssh never proxies your SSH traffic.

| Data | Location | Protection |
|---|---|---|
| Connections, tags, history | Local SQLite database | File permissions |
| Exported or encrypted backups | Wherever you save them | Argon2id + AES-256-GCM (`services/crypto.rs`) |
| Session recordings | Local recordings directory | File permissions |
| Kerberos tickets, SSH keys | System locations (`~/.ssh`, credential cache) | Managed by your OS and SSH tooling, never copied |

## Teams (planned)

The upcoming paid Teams sync is designed end-to-end encrypted: hostnames,
credentials and recording contents are encrypted on your device, and the sync
server only ever stores ciphertext. It will be able to see this metadata, which
is what makes server-side audit possible:

- team id, member emails, roles and seat count
- opaque entry ids, revision numbers and ciphertext sizes
- audit events of the form *member M used entry E at time T from IP A*
- recording sizes and durations

All client-side cryptography lives in this public repository, so it can be reviewed.
