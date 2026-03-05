# Bastion Hosts

Bayesian SSH provides flexible bastion (jump host) management for enterprise and cloud environments.

## Default Bastion

Set a default bastion that all connections will use automatically:

```bash
bayesian-ssh config --default-bastion bastion.company.com
```

Connections added after this will route through the default bastion unless overridden.

## Direct Connections (Bypassing Bastion)

Force a direct connection, bypassing the default bastion:

```bash
# At connection creation
bayesian-ssh add "Cloud Server" cloud.company.com --no-bastion

# At connection time
bayesian-ssh connect "Cloud Server" --no-bastion
```

## Custom Bastion per Connection

Override the default bastion with a specific one:

```bash
bayesian-ssh add "DMZ Server" dmz.company.com \
  --bastion dmz-bastion.company.com
```

## Mixed Environment Example

```bash
# Internal servers (use default bastion automatically)
bayesian-ssh add "App Server" app.company.com --tags internal,production

# Cloud servers (direct connection)
bayesian-ssh add "Cloud App" cloud.company.com --no-bastion --tags cloud,production

# Special network (custom bastion)
bayesian-ssh add "Special Server" special.company.com \
  --bastion special-bastion.company.com \
  --tags special,production
```

## Bastion Troubleshooting

### Test Bastion Connectivity

```bash
ssh -t -A -K user@bastion.company.com
```

### Override Bastion User

```bash
bayesian-ssh connect "Target Server" --bastion-user customuser
```

### Check Connection Configuration

```bash
bayesian-ssh show "Server Name"
```
