# Configuration

## Configuration File Location

Bayesian SSH automatically creates its configuration directory at:

```
~/.config/bayesian-ssh/
├── config.json          # Application configuration
└── history.db           # SQLite database
```

## Viewing and Updating Configuration

```bash
# View current configuration
bayesian-ssh config

# Update configuration
bayesian-ssh config --use-kerberos --default-user customuser

# Set default bastion
bayesian-ssh config --default-bastion bastion.company.com

# Clear default bastion
bayesian-ssh config --clear-bastion

# Set search mode
bayesian-ssh config --search-mode bayesian   # Smart ranking (default)
bayesian-ssh config --search-mode fuzzy      # Simple pattern matching
```

## Configuration Options

```json
{
  "default_user": "current-system-user",
  "default_bastion": "bastion.company.com",
  "default_bastion_user": "current-system-user",
  "use_kerberos_by_default": false,
  "log_level": "info",
  "auto_save_history": true,
  "max_history_size": 1000,
  "search_mode": "bayesian"
}
```

| Option | Default | Description |
|--------|---------|-------------|
| `default_user` | System user | Default SSH user for new connections |
| `default_bastion` | None | Default bastion host for all connections |
| `default_bastion_user` | System user | Default user for bastion connections |
| `use_kerberos_by_default` | `false` | Enable Kerberos authentication by default |
| `log_level` | `"info"` | Log verbosity: `trace`, `debug`, `info`, `warn`, `error`, `off` |
| `auto_save_history` | `true` | Automatically save session history |
| `max_history_size` | `1000` | Maximum number of history entries |
| `search_mode` | `"bayesian"` | Search mode: `bayesian` or `fuzzy` |

## Multi-Environment Configuration

Manage separate configs per environment:

```bash
# Use a specific environment
bayesian-ssh --env production connect "Server"
bayesian-ssh --env staging list
```
