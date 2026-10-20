# Inventory Sources

Inventory sources let bssh keep a set of connections in sync with an external
system instead of adding each host by hand. A source stores only how to find
hosts; nothing is imported until you run a sync.

Connections created by a source are marked as owned by it. A later sync updates
the same connection when a host's address changes, so sessions, aliases and
tags keep pointing at one entry. Connections you added manually are never
modified by a sync.

## Supported sources

| Kind | What it reads | Required configuration |
| --- | --- | --- |
| `ssh-config` | Concrete `Host` entries in an OpenSSH config | none (defaults to `~/.ssh/config`) |
| `ansible` | `ansible-inventory -i <path> --list` | `--inventory <path>` |
| `tailscale` | `tailscale status --json` peers | none (`--include-offline` to keep offline peers) |
| `aws` | Running EC2 instances via the AWS CLI | at least one `--region` |
| `gcp` | Running Compute Engine instances via gcloud | `--project <id>` |

The provider CLIs (`aws`, `gcloud`, `tailscale`, `ansible-inventory`) must be
installed and authenticated. bssh runs them with a fixed argument list, never
through a shell, and gives up after 60 seconds.

## Add a source

```bash
# A source never syncs automatically.
bssh source add local ssh-config
bssh source add ansible-lab ansible --inventory ./inventory.yml
bssh source add tailnet tailscale --include-offline
bssh source add aws-prod aws --profile work --region eu-west-1 --address private
bssh source add gcp-dev gcp --project my-project --address external
```

AWS sources accept `--address private` (default), `public` or `dns`; GCP
sources accept `internal` (default) or `external`. Both accept `--user`,
`--bastion` and `--bastion-user`, which are applied to every discovered host.
Flags that do not belong to the chosen kind are rejected.

## List sources

```bash
bssh source list
```

The table shows each source's kind, number of owned hosts, how many are stale,
the last sync time (relative) and the last error, truncated.

## Sync

```bash
bssh source sync aws-prod            # one source
bssh source sync aws-prod --dry-run  # print the plan, write nothing
bssh source sync --all               # continue past failures, exit 1 if any failed
bssh source sync --all --prune       # also delete long-stale hosts
```

A sync prints each change:

```
+ added web@aws-prod (10.0.1.12)
~ updated db (host 10.0.1.7 → 10.0.1.9)
- stale old-worker
↺ back cache
renamed: web → web@aws-prod
skipped (invalid): 2
```

Rules:

- Hosts are matched by the provider's stable id (EC2 instance id, Tailscale
  node id, ssh alias, ...), never by address.
- New connections get the provider's name. If that name is already taken, the
  name is scoped as `<name>@<source>` and then suffixed `-2`, `-3`, ...
- Existing connections are never renamed.
- A changed address, port, user or key path updates the existing connection.
  Tags are merged, never removed.
- Hosts missing from a sync are marked stale. The stale timestamp is set once
  and kept across later syncs, so a host that reappears is restored (`↺ back`).
- Invalid hosts (for example an AWS `Name` tag with spaces) are skipped and
  counted; the rest of the sync continues.

### Pruning

Without `--prune`, stale connections are kept indefinitely and can still be
used. With `--prune`, connections that have been stale for longer than
`--prune-after` days (default 30) are deleted together with their sessions,
aliases and tags.

## Stale hosts elsewhere

- `bssh list` appends `(stale)` to the name and shows the owning source.
- `bssh connect` prints a one-line warning when the target is stale, then
  connects normally.

## Remove a source

```bash
bssh source remove aws-prod                  # keep hosts, detach them
bssh source remove aws-prod --delete-hosts   # delete its connections too
```

Detaching clears `source_id`, `external_id` and `stale_since`, leaving a normal
manual connection. Removal asks for confirmation; `--force` skips it.

## Desktop app

Settings → Inventory sources provides the same workflow: add a source, list its
host counts and last sync, preview a sync, run it, and remove the source with a
choice to keep or delete its connections. Hosts owned by a source show a
`Synced` badge in the Hosts view, and stale hosts show a `Stale` badge.

## Limitations

- Names are assigned on first sync; changing the display name in the provider
  does not rename an existing connection.
- When two providers report the same external id inside one source, only the
  first is used and the duplicate is reported.
- `bssh import` is unchanged and does not create a source.
