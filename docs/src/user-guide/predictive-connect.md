# Predictive Connect

`bssh` ranks connections using not only how often and how recently you use them, but also **where** you are, **when** you connect, and **what you just connected to**. Everything is counted locally from your SQLite session history at query time—no background daemons or external services are used.

## What Is Captured

When you start an interactive session, `bssh` records the launch context alongside the session entry:

- **Git repository & working directory** (`git_root`, `cwd`): found by walking up parent directories for a `.git` directory or worktree file.
- **Local time**: the local hour of day (`0–23`) and weekday (`0–6`).
- **Previous connection** (`prev_connection_id`): the connection of the most recent session started within the last 30 minutes (across CLI and desktop).

### Privacy & Configuration

You can disable working-directory capture or the context factor entirely via `bssh config`:

```bash
# Stop recording cwd and git_root on new sessions
bssh config --capture-cwd off

# Disable context-aware ranking (falls back to frequency/recency/success only)
bssh config --prediction off

# Re-enable both
bssh config --prediction on --capture-cwd on
```

In `config.json`, these live under the `prediction` section (`enabled`, `capture_cwd`, and `window_days`, which defaults to `180`).

## How the Context Factor Works

Over the configured rolling window (`window_days`, default 180 days), `bssh` compares how often each active context feature `f ∈ {repo, hour, after_prev}` occurs on connection `c` against how often it occurs across all sessions globally. A feature is only used when its current value is known and it has been observed at least 3 times overall (`N_f ≥ 3`), using Laplace smoothing (`α = 1`) and circular `±1` hour matching (`23`, `0`, `1` are neighbours):

```text
lr_f(c) = ((hits_f(c) + α) / (total_w(c) + 2α)) / ((N_f + α) / (N + 2α))      (α = 1)
context_factor(c) = clamp( ∏_f lr_f(c), 0.25, 8.0 )
score(c) = prior × likelihood × recency × success × context_factor(c)
```

## Quick Connect (`bssh` with No Arguments)

Running `bssh` without arguments shows your top 3 predicted connections for the current context along with the strongest reason each host was suggested:

```text
$ bssh
Suggested connections
 1. web-prod      deploy@web1.example.com   · 12/15 of your sessions from ~/work/billing
 2. db-prod       postgres@db1              · usually opened right after web-prod
 3. bastion-eu    admin@bastion             · used often around 09:00
Enter = 1 · number · text to search · q to quit:
```

- Press **Enter** to connect immediately to `#1`.
- Type a **number** (`1`–`3`) to connect to that suggestion.
- Type **any search text** to run a ranked search over your connections.
- Type **`q`** (or press `Ctrl-D`) to quit.
- When `stdin` is not a TTY, `bssh` prints the suggestions and exits immediately.

## Explaining a Score (`bssh why`)

Use `bssh why <target>` to inspect the exact factor-by-factor score breakdown and rank of any connection in your current context:

```text
$ bssh why web-prod
web-prod — score 3.42 (rank 1 of 37 in the current context)
  frequency prior      0.73   (15 sessions)
  recency              0.91   (last used 4h ago)
  success rate         0.97
  repo ~/work/billing  ×4.10  (12 of 15 sessions)
  hour 09:00 ±1        ×1.30  (6 of 15)
  after db-prod        —      (no previous session in the last 30 min)
  context factor       ×5.33
```
