# Security & Compliance

## Kerberos Authentication

Bayesian SSH integrates with Kerberos for enterprise authentication:

```bash
# Enable Kerberos by default
bayesian-ssh config --use-kerberos

# Per-connection Kerberos
bayesian-ssh add "Secure Server" secure.company.com --kerberos true
```

### Kerberos Ticket Management

```bash
# Check current ticket status
klist

# Create new forwardable ticket
kinit -f -A

# Verify ticket creation
klist -s
```

Bayesian SSH will automatically verify tickets before connecting and create new ones when needed.

## Audit and Monitoring

```bash
# Audit servers
bayesian-ssh add "Audit Server" audit.company.com \
  --user auditor \
  --kerberos true \
  --tags audit,compliance,production

# Monitoring servers
bayesian-ssh add "Monitoring" monitoring.company.com \
  --user monitor \
  --kerberos true \
  --tags monitoring,production

# Log servers
bayesian-ssh add "Log Server" logs.company.com \
  --user logger \
  --kerberos true \
  --tags logging,production
```

## Compliance Environments

### SOX Compliance

```bash
bayesian-ssh add "SOX Server" sox.company.com \
  --user sox-user \
  --kerberos true \
  --tags sox,compliance,production
```

### PCI Compliance

```bash
bayesian-ssh add "PCI Server" pci.company.com \
  --user pci-user \
  --kerberos true \
  --tags pci,compliance,production
```

### HIPAA Compliance

```bash
bayesian-ssh add "HIPAA Server" hipaa.company.com \
  --user hipaa-user \
  --kerberos true \
  --tags hipaa,compliance,production
```

## Security Features

### Authentication
- **Kerberos integration** - Enterprise authentication with automatic ticket management
- **SSH key management** - Secure key handling per connection
- **Bastion support** - Secure jump host connections

### Access Control
- **Audit logging** - Complete connection history with timestamps and outcomes
- **Tag-based organization** - Categorize servers by compliance requirements
- **Session tracking** - Monitor active sessions with PID-level tracking

### Best Practices

1. Always use Kerberos for internal/enterprise servers
2. Use separate SSH keys per environment (dev, staging, production)
3. Route internal servers through bastion hosts
4. Review session history regularly with `bayesian-ssh history`
5. Use `--force` carefully on destructive operations
6. Backup your database regularly with `bayesian-ssh backup`
