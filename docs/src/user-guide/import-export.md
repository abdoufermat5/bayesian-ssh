# Import & Export

## Import from SSH Config

Import existing connections from your `~/.ssh/config` file:

```bash
# Import from default location
bayesian-ssh import

# Import from a specific file
bayesian-ssh import --file /path/to/ssh/config
```

This reads your SSH config and creates Bayesian SSH connections for each host entry, preserving hostname, user, port, identity file, and proxy settings.

## Export Connections

Export your connections for sharing or backup:

```bash
bayesian-ssh export
```
