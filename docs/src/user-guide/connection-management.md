# Connection Management

## Adding Connections

```bash
# Basic connection
bayesian-ssh add "Server Name" hostname.com

# With specific bastion
bayesian-ssh add "Server Name" hostname.com --bastion bastion.company.com

# Force direct connection (no bastion)
bayesian-ssh add "Server Name" hostname.com --no-bastion

# With tags for organization
bayesian-ssh add "Web Prod" web-prod.company.com --tags production,web

# With custom user and key
bayesian-ssh add "EC2 Web" ec2-web.company.com \
  --user ubuntu \
  --kerberos false \
  --key ~/.ssh/ec2-key.pem \
  --tags ec2,production
```

## Connecting to Servers

```bash
# Exact match
bayesian-ssh connect "Server Name"

# Fuzzy search
bayesian-ssh connect "webprod"            # Finds "web-prod-server"
bayesian-ssh connect "prod"               # Shows all production servers

# With overrides
bayesian-ssh connect "Server Name" --no-bastion --user customuser
```

## Listing Connections

```bash
# List all connections
bayesian-ssh list

# Filter by tag
bayesian-ssh list --tag production
bayesian-ssh list --tag development
```

## Viewing Connection Details

```bash
bayesian-ssh show "Server Name"

# Fuzzy search works here too
bayesian-ssh show "dbprod"
```

## Editing Connections

```bash
bayesian-ssh edit "Server Name"

# Fuzzy search
bayesian-ssh edit "webprod"
```

## Removing Connections

```bash
# With confirmation prompt
bayesian-ssh remove "Server Name"

# Skip confirmation
bayesian-ssh remove "Server Name" --force
```

## Duplicating Connections

Clone an existing connection with a new name:

```bash
bayesian-ssh duplicate "Source Server" "New Server"
```

## Grouping Connections

Organize connections into groups:

```bash
bayesian-ssh groups
```

## Ping / Latency Check

Test connectivity to a server:

```bash
bayesian-ssh ping "Server Name"
```
