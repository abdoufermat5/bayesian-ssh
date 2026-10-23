# bssh Teams protocol, v1

Shared, end-to-end encrypted connection vaults for teams. The server stores
ciphertext and the metadata listed in `SECURITY.md`; it never holds a key that
can decrypt the vault. This document plus `src/` is the contract: the types are
in `api.rs`, the exact signed and authenticated bytes in `layout.rs`.

## Keys

| Key | Held by | Purpose |
|---|---|---|
| Device X25519 keypair | each device | receives wrapped team keys |
| Device Ed25519 keypair | each device | signs entries, audit events, key wraps, auth challenges |
| Team key `TK_v` (32 random bytes) | every active member device | root of the vault, one per `key_version` |
| Vault key `VK_v` | derived on demand | AES-256-GCM key for entries |

Device private keys are generated locally and stored sealed on disk with the
existing `services/crypto.rs` scheme (Argon2id + AES-256-GCM). They're never sent.

## Recipes

All AEAD is **AES-256-GCM** with a random 96-bit nonce from the OS RNG, tag appended
to the ciphertext. All KDF is **HKDF-SHA256**. All signatures are **Ed25519** (RFC 8032,
strict verification: reject non-canonical `S` and small-order keys).

**Vault key**

```
VK_v = HKDF(salt = team_id (16 bytes), ikm = TK_v,
            info = "bssh-teams/v1/vault-key" ‖ key_version (u32 BE), len = 32)
```

**Seal an entry** (payload = JSON `VaultPayload`)

```
aad        = layout::entry_aad(team_id, entry_id, base_revision, key_version, author_device_id)
ciphertext = AES-256-GCM(VK_v, nonce, payload, aad)
signature  = Ed25519(author_device_sk, layout::entry_signing_bytes(team_id, entry))
```

Open: verify the signature against the author device's Ed25519 key first, then decrypt.
Reject an entry whose author device isn't a current or former member device you know.

**Wrap a team key to a device**

```
eph           = fresh X25519 keypair (one per wrap)
shared        = X25519(eph.secret, recipient.x25519_public)    reject all-zero output
wrap_key      = HKDF(salt = eph.public ‖ recipient.x25519_public, ikm = shared,
                     info = "bssh-teams/v1/key-wrap", len = 32)
aad           = layout::key_wrap_aad(team_id, key_version, recipient_device_id, wrapping_device_id)
ciphertext    = AES-256-GCM(wrap_key, nonce, TK_v, aad)
signature     = Ed25519(wrapping_device_sk, layout::key_wrap_signing_bytes(wrapped))
```

Unwrap: verify the signature against a **pinned admin** Ed25519 key (see Trust), then
decrypt and check the plaintext is exactly 32 bytes. Zeroize `shared`, `wrap_key` and
`TK_v` copies when done.

**Auth**: `POST /auth/challenge`, then sign `layout::auth_signing_bytes` and
`POST /auth/token` for a one-hour bearer token.

## Trust

The server must not be able to insert a key or member. v1 uses one trust root per
team: the creating admin device's Ed25519 public key.

- The creator pins its own key when creating the team.
- An invitee gets the invite token and the admin's Ed25519 fingerprint out of band
  (`SHA256:<base64>` like OpenSSH), and pins that key when accepting.
- A client accepts a `WrappedKey` only if `wrapping_device_id`'s Ed25519 key is pinned.
- Promoting more admins and changing the trust root are out of scope for v1.

## Flows

**Create a team.** Generate `TK_1`, wrap it to your own device, sign, then
`POST /teams { initial_key }`. Write a `TeamProfile` entry with the team name.

**Invite and join.** An admin calls `POST /teams/{id}/invites` and sends the token and
fingerprint to the invitee. A new invitee creates their account with `POST /accounts`,
passing the token as `invite_token`: a server may accept accounts only for an email with a
pending, unexpired invite (the token is checked, not consumed). The invitee then calls
`POST /invites/accept`, which consumes it. The admin's client
polls `GET /teams/{id}/devices/pending` and posts a `WrappedKey` for each new device
(`POST /teams/{id}/keys`, same `key_version`).

**Revoke an invite.** A pending invite takes a seat until it is accepted or expires.
`GET /teams/{id}/invites` (admin) lists the pending ones, without their tokens;
`DELETE /teams/{id}/invites/{invite}` (admin) revokes one, and its token stops working.

**Remove a member.** `DELETE /teams/{id}/members/{member}` sets `rotation_required`.
Pushes with the old `key_version` then fail with `RotationRequired`. The admin generates
`TK_{v+1}`, wraps it to every remaining active device, and posts them all in one request.
Entries are re-encrypted lazily: a client that edits an entry writes it under the
newest key. Old ciphertext stays readable to whoever already had the old key.

## Sync

- Local-first: the client keeps a full local copy and an outbox. SSH never waits on sync.
- Pull: `GET /vault?since=<last_seen_revision>` until `has_more = false`. Apply entries in
  revision order. The highest revision per `entry_id` wins.
- Push: send outbox entries with `base_revision` = the revision they were edited from.
  On `conflicts`: pull, then re-seal the pending local edit with `base_revision` =
  the new current revision and push again (last push wins). The overwritten version
  stays in server history, so nothing is lost. A local edit to an entry that was
  tombstoned remotely is dropped, and the user is told.
- Every request carries `Bssh-Protocol: 1` and `Bssh-Client-Version: <semver>`.

**Integrity checks the client must run** (the server is untrusted for confidentiality
and integrity, see PERIMETER.md):

- *Rollback.* `revision` is assigned by the server and not signed, but `base_revision`
  is. Because pull returns every revision in order, the revisions of one `entry_id` form
  a chain: each pulled entry's `base_revision` must equal the revision the client already
  holds for that entry (0 if new to it), and its `revision` must be greater. An entry that
  breaks the chain, for example an old ciphertext relabelled as newer, is rejected and
  reported, never applied.
- *Unknown authors.* An entry whose `author_device_id` isn't a known device of the team
  is rejected.
- *Key replay.* Before unwrapping, check `team_id`, `recipient_device_id` (this device)
  and `key_version` against what's expected. A stored key version is never replaced by
  a different wrapped key for the same version.

The server can still withhold updates (a client can't detect an entry it never sees);
that's an availability limit, not a confidentiality or integrity one.

## Audit

Each device keeps one hash chain per team: `seq` from 0, `prev_hash` =
`layout::audit_event_hash(previous)`, genesis `AUDIT_GENESIS`. Events are written
locally first and uploaded in `seq` order. The server verifies each signature against
the device's Ed25519 key and that the batch continues the stored chain, then stores
the events with `received_at` and `client_ip`. `bssh teams audit` downloads the log
and re-checks every signature and chain on the admin's machine.

What the chain guarantees, against a server that edits its database:

- *Edited* events fail their signature.
- *Reordered*, *forked* or *duplicated* events break the `prev_hash` links.
- A *deleted* event is visible as a gap once a later event from the same device is
  stored; and while it's missing, that device's uploads are refused
  (`AuditChainBroken`), which its owner sees at every sync.
- Not detectable from the log alone: *truncation*, i.e. dropping a device's newest
  events before it uploads anything after them. The device notices on its next upload.
  Closing this needs devices to publish signed chain heads elsewhere (future work).

## Organizations

An organization owns teams and, with billing, their subscription and seats. It has no
name on the server (team names live in the encrypted vault), so clients label it by its
owners' emails.

- `GET /orgs`: the organizations the caller owns or has a team in, as `OrgInfo`
  (`org_id`, `my_role`, `owners`, `team_ids`, `created_at`). Owners see every team of
  the organization; members only their own. `GET /orgs/{id}` returns one.
- `POST /orgs` (no body): create an empty organization owned by the caller. Refused with
  `PaymentRequired` while the caller owns another without a live subscription: one free
  organization per account.
- `POST /orgs/{id}/owners { email }` (owner): the account must already be an active member
  of one of the organization's teams. Returns the updated `OrgInfo`.
  `DELETE /orgs/{id}/owners/{account_id}` (owner): 204; the last owner can't be removed.
- `POST /teams` accepts `org_id`: the caller must own that organization. Without it the
  team goes to the oldest organization the caller owns, or to a new one. `TeamInfo.org_id`
  and `BillingInfo.org_id` say which organization a team belongs to.

Seats are counted per organization: distinct active members across its teams, plus one
per pending, unexpired invite to an email that isn't a member yet. Someone in three teams
of the same organization takes one seat. `FREE_SEATS` applies per organization; on a
server without billing the limit is the organization's `seats` (`teams.seats` is no longer
read).

- `GET /orgs/{id}/billing` (any member of the organization): `BillingInfo`, with `org_id`.
- `POST /orgs/{id}/billing/checkout { seats, interval }` (owner): a checkout page URL.
- `POST /orgs/{id}/billing/portal` (owner): the provider's customer portal.

The team-scoped billing routes (`/teams/{id}/billing…`) still work and act on the team's
organization. A server older than organizations has no `/orgs` routes and answers 404.

## Billing

Seats are enforced by the server (perimeter rule P3: no license in the client). Seats
belong to the organization (see above): its `free_seats` members are free, and above that
it needs a subscription for all its seats. The client never handles payment data: an owner,
or a team admin through the team route, asks for a provider page and opens it.

- `GET /teams/{id}/billing` (any member): `BillingInfo` of the team's organization,
  including `read_only`.
- `POST /teams/{id}/billing/checkout { seats, interval }` (admin): a checkout page URL.
  `seats` must cover the seats in use.
- `POST /teams/{id}/billing/portal` (admin): the provider's customer portal (card,
  invoices, seats, cancellation).

When a subscription lapses and the organization has more members than the free seats, its
teams become **read-only**: pulls, key grants, member removal and audit uploads keep
working (nobody is locked out of their hosts, and the organization can shrink back under
the free seats), while pushes fail with `PaymentRequired` and invites with `SeatLimit`.

## Server-side checks (non-exhaustive)

- Entry push: the author device belongs to the caller, is active and is a member; the
  signature verifies; `key_version` is current, or `RotationRequired`.
- Key post: the caller is admin; signatures verify against the caller's device key;
  recipients are active member devices; a rotation covers all of them.
- Audit ingest: `device_id` is the caller's device; signatures and chain verify.
- Revoked devices and removed members get `Forbidden` on everything team-scoped.

## Versioning

Changing any layout in `layout.rs` changes the wire format: bump `PROTOCOL_VERSION`,
and update the golden-vector tests on purpose.
