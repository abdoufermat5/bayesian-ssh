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

This creates your account and this machine's device keys. A server may only
accept accounts for people it knows: the hosted one takes invited members
(`--invite <TOKEN>`, from the invite) and the team creators its operator
registered. The private keys
never leave the machine; they're encrypted with a passphrase you choose. Every
`bssh teams` command asks for it, or reads `BSSH_TEAMS_PASSPHRASE` in scripts.

`bssh teams whoami` shows your account and this device's **fingerprint**.

## Create a team and share connections

```bash
bssh teams create platform
bssh teams share db-prod --team platform
```

You're the team's admin. In the desktop app, **Share with a team…** in a host's
menu (or `S` on the selected host) does the same. A shared connection carries its host, user, port,
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
   for Ana, including a one-time token and your fingerprint. On a new install of
   the desktop app, Ana can instead choose **I was invited to a team** in
   onboarding and paste the whole invite message.
2. **Ana:** runs `bssh teams init ... --invite <TOKEN>`, then
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
your device key, chains it to your previous events and uploads it.

Admins see the log with `bssh teams audit <team>` (`--host`, `--member`,
`--since` to narrow it), or in the team's **Audit log** tab in the desktop
app. Every event is verified on your machine, not taken from the server: an
edited, reordered or forged event shows as invalid, and a deleted one as a gap.
The command exits non-zero when verification fails, so it can run in a
compliance check.

One limit: if the server drops a member's *newest* events before that member
uploads anything more recent, what's left is still a valid log. The member's
next sync then fails with an audit-chain error. The desktop app keeps that
warning on screen (and on the sidebar's Teams badge) until a later sync
succeeds; on the command line, watch for that message.

## In the desktop app

The **Teams** page in the sidebar does everything the commands below do.

**Set up and unlock.** Setup asks for the sync server, your email and a
passphrase. The passphrase encrypts this device's Teams keys; it never leaves
the machine and nobody can reset it. If you forget it, set the device up again
and ask an admin to grant the key to the new device. Unlock once per session;
the keys stay unlocked in memory until you click **Lock** or quit. Team hosts
still connect while Teams is locked; their session events are signed at the
next unlock.

**Teams home** lists your teams with your role, host count, when each last
synced (hover for the exact UTC time) and anything that needs attention:
devices waiting for a key, a failed audit check, entries that failed integrity
checks, a stopped key rotation, or this device still waiting for its key.
These stay on screen, with a badge on the sidebar's **Teams** entry, until the
condition clears; they don't disappear like notifications. The page checks
again every 30 seconds while it's open.

**Create team** and **Join with invite** are in the page header. To join,
paste the whole invite message your admin sent you: the app reads the token
and the admin's fingerprint from it (you can also fill both fields by hand). If
you haven't set Teams up yet, paste the invite on the setup screen to fill in
the server and your email. If the admin fingerprint doesn't match, nothing is
trusted; check it with your admin, correct it and choose **Try again** (the
token is already used, so this finishes the join by team id). After joining,
the page shows "Waiting for an admin to grant this device the key" with your
fingerprint and a **Copy** button until the key arrives, and syncs the team as
soon as it does while the page is open.

Open a team to see three tabs:

- **Hosts**: the team's connections, **Share** one of yours, or **Unshare** one
  (a confirmation says it's removed for everyone in the team).
- **Members & devices**: members, their role and each device's fingerprint.
  Admins also see **Invite**, the devices waiting for the key and **Remove**.
  **Invite** produces a message to send as is, with both the desktop and the
  command-line steps, the one-time token, your fingerprint and the expiry, and
  a **Copy message** button.
- **Audit log** (admins): verified as soon as you open it.

**Granting a key.** Choose **Verify and grant…** next to the waiting device.
The app never shows the fingerprint the server reports for that device; ask the
member for theirs over a channel you trust and paste it. Spaces and a missing
`SHA256:` prefix are fine. Once it matches, the fingerprint appears in groups
of four so you can read it back to them. Tick "I got this fingerprint from …
directly, not from this screen" and **Grant key** becomes available. A
mismatch means: don't grant; ask them to read it again, since someone may be
impersonating that device.

**Removing a member.** **Remove…** first shows what will happen: the next key
version, which of their devices lose access, how many hosts they already
synced (they keep those copies, so rotate credentials that matter) and which
devices receive the new key. Type their email to confirm. If the server lists
a device this machine never approved, removal isn't offered until you grant or
remove that device's owner; if a rotation stops anyway, the reason stays in the
dialog.

**Audit log.** The banner says one of three things: **Verified** (every event
checked from each device's first one), **Verified, partial** (only the newest
events were fetched, or the log doesn't start at the first event: older events
weren't checked) or **Verification failed** (treat the log as tampered; the
problems are listed). Filter by member, host and date (`--member`, `--host`,
`--since`), and **Export CSV** to keep a copy. Times are in UTC; the Check
column says "Valid", "Bad signature", "Unknown device" or "Other team" for each
event.

## Commands

| Command | What it does |
| --- | --- |
| `bssh teams init --server URL --email EMAIL [--invite TOKEN]` | Set up this machine |
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
| `bssh teams audit TEAM [--host H] [--member E] [--since D]` | Show and verify the audit log (admin) |
