# Session Recording & Search

`bssh` can optionally record interactive SSH sessions as [asciicast v2](https://docs.asciinema.org/manual/asciicast/v2/) (`.cast`) files and index their plain-text output in SQLite FTS5 for instant full-text search and terminal replay.

Recording is **opt-in** and **off by default**.

## What Is Recorded

- **Remote output only (`"o"` events)** and terminal resize events (`"r"` events).
- **Keystrokes and input are never recorded.** Passwords typed at prompts (`sudo`, `kinit`, passphrase prompts) are not echoed by the remote host and never reach the recording file.
- Non-interactive commands (`bssh exec`, SFTP transfers, port forwards, SOCKS proxies) are not recorded.

## Enabling Recording

Enable or disable session recording globally for the active environment:

```bash
# Enable recording for all connections
bssh config --recording on

# Record only connections carrying specific tags
bssh config --recording on --recording-tags prod,critical

# Clear the tag filter (record all connections when enabled)
bssh config --recording-tags ""

# Disable recording
bssh config --recording off
```

Override the configuration for a single session with `--record` or `--no-record`:

```bash
bssh connect web-prod --record
bssh connect bastion --no-record
```

In `config.json`, recording settings live under the `recording` key:

```json
{
  "recording": {
    "enabled": true,
    "tags": ["prod"],
    "max_bytes_per_session": 52428800,
    "retention_days": 90
  }
}
```

## Storage, Permissions & Retention

- **Location:** `<env_dir>/recordings/<session_id>.cast` (for example `~/.config/bayesian-ssh/environments/default/recordings/<session_id>.cast`).
- **Permissions:** The `recordings/` directory is created with mode `0700` and each `.cast` file with mode `0600`.
- **Size limit:** Each session stops writing output events once it reaches `max_bytes_per_session` (default `50 MiB`), appending a single `[bssh: recording truncated]` marker. The plain-text search index is capped at the first `4 MiB` of stripped output per session.
- **Retention:** Recordings older than `retention_days` (default `90` days; `0` keeps recordings forever) are automatically pruned after sessions end or when running `bssh close --cleanup`. Pruning removes the `.cast` file, clears `recording_path`, and deletes the FTS5 index entry while keeping the session metadata row in history.

## Searching Recorded Output

Use `bssh history --grep` (`-g`) to search across recorded terminal output:

```bash
# Find which server and session printed a command or error
bssh history --grep "systemctl restart nginx"

# Combine with connection, day window, and limit filters
bssh history -g "Out of memory" -c web-prod -D 30 -n 10
```

User queries are matched as literal phrases in SQLite FTS5, so special characters (`"`, `*`, `OR`, `NEAR(`) in search strings are handled safely without syntax errors.

In regular `bssh history` output, every session displays its 8-character session ID prefix and a `●` indicator when a recording is available.

## Replaying Sessions in the Terminal

Replay any recorded session by its full UUID or a unique prefix (at least 6 characters):

```bash
# Replay at original speed (idle pauses capped at 2 seconds)
bssh replay 3f2a91c0

# Replay at 2x speed with a 1-second idle cap
bssh replay 3f2a91c0 --speed 2.0 --idle-cap 1.0
```

Because `.cast` files use standard asciicast v2 format, they can also be played directly with `asciinema play`.

## Desktop GUI

- **Settings → Features:** Toggle **Record sessions** on or off and optionally restrict recording to a comma-separated list of tags in **Only for tags**.
- **History view:** Recorded sessions display a **Recorded** badge and a **Replay** button. Use **Search recorded output…** in the toolbar to search full-text session output with highlighted snippets.
- **Replay modal:** Click **Replay** on any recorded session or search hit to open a read-only terminal player with play/pause, `1×`/`2×`/`4×` speed controls, a 2-second idle cap, progress scrubbing, and **Jump to end**.

## Privacy Warning

> **Important:** Terminal output can contain secrets printed to the screen (environment variables, API tokens, configuration files, database query results).
>
> - Restrict recording to the environments or tags where you need an audit trail.
> - Use `bssh connect <target> --no-record` before inspecting sensitive material.
> - Database backups created with `bssh backup` include the `session_text` FTS5 search index (plain-text session output), even though the `.cast` files themselves live separately under `recordings/`.
