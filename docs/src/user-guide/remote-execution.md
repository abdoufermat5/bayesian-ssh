# Remote Execution & File Transfer

Bayesian SSH ships with native commands for running remote commands and moving files without dropping into an interactive shell. They work with direct connections, `ProxyJump` (`-J`) bastions, and **interactive bastions** (such as OVH “The Bastion”) that don’t accept inline remote commands.

## `exec` — Run a Command Remotely

```bash
bayesian-ssh exec <host> -- <command...>
```

The `--` separator is required so that flags belonging to the remote command are not interpreted by `bayesian-ssh`.

```bash
# Simple command
bayesian-ssh exec web-prod -- "uptime"

# With pipes / redirection (the whole quoted string runs in the remote shell)
bayesian-ssh exec db-prod -- "psql -c 'SELECT count(*) FROM users'"

# Through a Kerberos + interactive bastion (handled automatically)
bayesian-ssh exec cs-qauth -- "ls -l /home"
```

The remote process’s stdout and stderr are streamed back, and `bayesian-ssh` exits with the remote command’s exit code so it can be chained with shell logic.

### Interactive-Bastion Behaviour

When a connection has both Kerberos and an interactive bastion configured, `exec` can’t pass the command as SSH arguments (the bastion would treat them as a target name). Instead, it opens a PTY shell, drains the bastion banner/MOTD, brackets the command with unique `BSSH_<id>_START` / `BSSH_<id>_END` markers, and extracts the clean output between them. The PTY is widened to 200 columns so column-aware tools (`ls -l`, `ps`, etc.) don’t wrap or pad to 80 columns.

## `upload` — Send Files to a Remote Host

```bash
bayesian-ssh upload <host> <local-path> <remote-path> [--recursive]
```

```bash
# Single file
bayesian-ssh upload web-prod ./config.yml /etc/myapp/config.yml

# Whole directory
bayesian-ssh upload web-prod ./dist /var/www/app --recursive
```

Uses native SFTP via `russh-sftp` when available and falls back to `scp` for transports that don’t support it.

## `download` — Fetch Files from a Remote Host

```bash
bayesian-ssh download <host> <remote-path> <local-path> [--recursive]
```

```bash
# Single file
bayesian-ssh download db-prod /var/log/postgres.log ./postgres.log

# Whole directory
bayesian-ssh download db-prod /var/backups ./backups --recursive
```

## `forward` — Local Port Forwarding

Tunnel a local port to a remote address through the SSH connection.

```bash
bayesian-ssh forward <host> -L <local-port>:<remote-host>:<remote-port>
```

```bash
# Expose remote PostgreSQL on localhost:5432
bayesian-ssh forward db-prod -L 5432:localhost:5432

# Reach an internal-only HTTP service via a bastion-routed host
bayesian-ssh forward jump-host -L 8080:internal-api.local:80
```

The tunnel runs in the foreground; `Ctrl+C` tears it down.

## `proxy` — SOCKS5 Dynamic Proxy

Start a SOCKS5 proxy that tunnels traffic through the SSH connection — useful for reaching multiple internal hosts without setting up one tunnel per port.

```bash
bayesian-ssh proxy <host> -D <local-port>
```

```bash
# Browse the internal network through host bastion-01
bayesian-ssh proxy bastion-01 -D 1080

# Then point a browser / curl at the SOCKS5 proxy:
curl --socks5 localhost:1080 http://internal.local
```

## Choosing the Right Tool

| Use case | Command |
|---|---|
| Run a one-off command and capture output | `exec` |
| Copy one or more files | `upload` / `download` |
| Reach a single remote port from your laptop | `forward` |
| Reach many internal hosts/ports through one SSH session | `proxy` |
| Interactive shell session | `connect` |
