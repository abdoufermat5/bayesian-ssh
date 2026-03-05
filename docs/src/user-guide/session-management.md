# Session Management

## View Session History

```bash
# Recent sessions with stats
bayesian-ssh history

# Filter by connection
bayesian-ssh history --connection prod

# Last 7 days, failures only
bayesian-ssh history --days 7 --failed

# Limit results
bayesian-ssh history --limit 50
```

## Manage Active Sessions

```bash
# List active sessions (shows PIDs and stale detection)
bayesian-ssh close

# Close specific session
bayesian-ssh close "Prod Server"

# Clean up stale sessions (PIDs no longer running)
bayesian-ssh close --cleanup

# Force close all
bayesian-ssh close --all --force
```

## View Statistics

```bash
bayesian-ssh stats
```

Statistics include success/failure rates, average session duration, and usage frequency across all connections.

## Backup and Restore

### Backup

```bash
bayesian-ssh backup
```

### Restore

```bash
bayesian-ssh restore
```

This backs up and restores the SQLite database containing all connections, sessions, and aliases.
