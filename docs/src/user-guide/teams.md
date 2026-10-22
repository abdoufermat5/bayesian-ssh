# Teams

bssh Teams shares connections with your team through a vault that is
encrypted on your devices. The sync server stores only ciphertext and the
metadata listed in `SECURITY.md`: it never sees hostnames, users, tags or
notes, and it is never in the SSH path.

Team connections appear in `bssh list` and `bssh connect` like any other host
(they're an [inventory source](./inventory-sources.md) of kind `team`), and
connecting to one is recorded in the team's signed audit log.

## Set up a machine

```bash
bssh teams init --server https://teams.example.com --email you@example.com
```

This creates your account and this machine's device keys. The private keys
never leave the machine; they're encrypted with a passphrase you choose. Every
`bssh teams` command asks for it, or reads `BSSH_TEAMS_PASSPHRASE` in scripts.

`bssh teams whoami` shows your account and this device's **fingerprint**.

## Create a team and share connections

```bash
bssh teams create platform
bssh teams share db-prod --team platform
```

You're the team's admin. A shared connection carries its host, user, port,
bastion, Kerberos setting and tags; never key paths or passwords, which stay
per person. On your machine the team's copy is listed as `db-prod@platform`
next to your own `db-prod`.

To change a team connection, edit the team copy (`bssh edit db-prod@platform`)
and run `bssh teams share db-prod@platform --team platform` before the next
sync, which would otherwise restore the vault's version.
`bssh teams unshare <name>` deletes it for everyone (the server keeps its
history).

## Invite someone

Fingerprints are how bssh keeps the server from slipping in a device of its
own, so they're exchanged out of band, e.g. in your chat.

1. **Admin:** `bssh teams invite platform ana@example.com` prints two commands
   for Ana, including a one-time token and your fingerprint.
2. **Ana:** runs `bssh teams init ...`, then
   `bssh teams join <TOKEN> --fingerprint <ADMIN FINGERPRINT>`. bssh refuses to
   trust the team if no admin of it has that fingerprint. It prints Ana's
   device fingerprint for her to send back.
3. **Admin:** `bssh teams grant platform` lists devices waiting for the key;
   `bssh teams grant platform --fingerprint <ANA'S FINGERPRINT>` gives the key
   only to the device with that fingerprint.
4. **Ana:** `bssh teams sync`. The team's connections appear in `bssh list`.

If Ana mistyped the fingerprint in step 2, the invite token is already used:
she finishes with `bssh teams join --team <TEAM ID> --fingerprint <FINGERPRINT>`
(the error message prints the exact command).

## Sync

```bash
bssh teams sync            # every team
bssh teams sync platform
```

Sync pulls and verifies new entries, pushes your edits, uploads audit events
and updates the team's connections in `bssh list`. You can keep working
offline: edits wait locally until the next sync. Entries that fail integrity
checks (a server replaying an old version, an unknown author) are reported and
ignored.

## Remove a member

```bash
bssh teams members platform
bssh teams remove platform ana@example.com
```

Removing a member rotates the team key: anything written afterwards is
unreadable to them, and they can no longer sync. They may still have copies of
what they already synced, so rotate any credentials that matter.

A rotation wraps the new key only to devices **this admin machine approved**
with `bssh teams grant`. If the server lists a device you never approved, the
rotation stops and names it; grant it after checking its fingerprint, or remove
the member.

## Audit log

`bssh connect` to a team connection records a "session started" event
locally, even offline and without your passphrase. The next sync signs it with
your device key, chains it to your previous events and uploads it. The server
verifies the signature and the chain, so events can't be edited, reordered or
dropped without detection.

## In the desktop app

The **Teams** page in the sidebar does everything the commands below do. Unlock
it once with your passphrase; the keys stay unlocked in memory until you click
**Lock** or quit. When granting a key, paste the fingerprint the member sent
you: **Grant key** only becomes available when it matches the waiting device.

## Commands

| Command | What it does |
| --- | --- |
| `bssh teams init --server URL --email EMAIL` | Set up this machine |
| `bssh teams whoami` | Account, device and fingerprint |
| `bssh teams create NAME` | Create a team (you're its admin) |
| `bssh teams list` | Teams on this machine |
| `bssh teams share CONNECTION --team TEAM` | Share or update a connection |
| `bssh teams unshare CONNECTION` | Delete a team connection for everyone |
| `bssh teams invite TEAM EMAIL [--admin]` | Invite someone (admin) |
| `bssh teams join TOKEN --fingerprint FP` | Join a team |
| `bssh teams grant TEAM [--fingerprint FP]` | List waiting devices, or grant one (admin) |
| `bssh teams sync [TEAM]` | Sync one team or all |
| `bssh teams members TEAM` | Members and device fingerprints |
| `bssh teams remove TEAM EMAIL` | Remove a member and rotate the key (admin) |
