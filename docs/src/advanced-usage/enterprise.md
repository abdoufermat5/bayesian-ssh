# Enterprise Environments

## Enterprise Bastion Configuration

### Default Configuration

```bash
# Set up default enterprise configuration
bayesian-ssh config \
  --default-user currentuser \
  --default-bastion bastion-server.company.priv \
  --use-kerberos

# Add production servers with tags (will use default bastion)
bayesian-ssh add "Web Prod" web-prod.company.com --tags production,web
bayesian-ssh add "DB Prod" db-prod.company.com --tags production,database
bayesian-ssh add "App Prod" app-prod.company.com --tags production,application

# Quick connection to production
bayesian-ssh connect "Web Prod"
```

### Bastion Management Strategies

#### 1. Default Bastion for Internal Servers

```bash
# These will automatically use your default bastion
bayesian-ssh add "Internal Web" internal-web.company.com --tags internal,production
bayesian-ssh add "Internal DB" internal-db.company.com --tags internal,production
```

#### 2. Direct Connections for Cloud Instances

```bash
# Force direct connection, bypassing default bastion
bayesian-ssh add "EC2 Web" ec2-web.company.com \
  --user ubuntu \
  --kerberos false \
  --key ~/.ssh/ec2-key.pem \
  --no-bastion \
  --tags ec2,production,web
```

#### 3. Custom Bastion for Specific Networks

```bash
# Override default bastion with specific one
bayesian-ssh add "DMZ Server" dmz.company.com \
  --bastion dmz-bastion.company.com \
  --tags dmz,production
```

#### 4. Mixed Environment Setup

```bash
# Internal servers (use default bastion)
bayesian-ssh add "App Server" app.company.com --tags internal,production

# Cloud servers (direct connection)
bayesian-ssh add "Cloud App" cloud.company.com --no-bastion --tags cloud,production

# Special network (custom bastion)
bayesian-ssh add "Special Server" special.company.com \
  --bastion special-bastion.company.com \
  --tags special,production
```

## Multi-Environment Management

```bash
# Development environment
bayesian-ssh add "Web Dev" web-dev.company.com \
  --user dev-user \
  --bastion dev-bastion.company.com \
  --tags development,web

# Staging environment
bayesian-ssh add "Web Staging" web-staging.company.com \
  --user staging-user \
  --bastion staging-bastion.company.com \
  --tags staging,web

# Production environment
bayesian-ssh add "Web Prod" web-prod.company.com \
  --user prod-user \
  --bastion prod-bastion.company.com \
  --tags production,web

# List by environment
bayesian-ssh list --tag production
bayesian-ssh list --tag development
```

## Network Segmentation

### DMZ Servers

```bash
# Web servers in DMZ
bayesian-ssh add "DMZ Web" dmz-web.company.com \
  --user web-user \
  --kerberos false \
  --tags dmz,web,production

# API servers in DMZ
bayesian-ssh add "DMZ API" dmz-api.company.com \
  --user api-user \
  --kerberos false \
  --tags dmz,api,production
```

### Internal Network

```bash
# Internal application servers
bayesian-ssh add "Internal App" internal-app.company.com \
  --user app-user \
  --bastion internal-bastion.company.com \
  --tags internal,application,production

# Database servers
bayesian-ssh add "Internal DB" internal-db.company.com \
  --user db-user \
  --bastion internal-bastion.company.com \
  --tags internal,database,production
```

## High Availability and Load Balancing

### Load Balancer Backends

```bash
# Primary load balancer
bayesian-ssh add "LB Primary" lb-primary.company.com \
  --user currentuser \
  --tags loadbalancer,primary,production

# Secondary load balancer
bayesian-ssh add "LB Secondary" lb-secondary.company.com \
  --user currentuser \
  --tags loadbalancer,secondary,production

# Backend servers
bayesian-ssh add "Backend 1" backend-1.company.com \
  --user app-user \
  --bastion lb-primary.company.com \
  --tags backend,production,web

bayesian-ssh add "Backend 2" backend-2.company.com \
  --user app-user \
  --bastion lb-secondary.company.com \
  --tags backend,production,web
```

## Backup and Recovery

```bash
# Primary backup server
bayesian-ssh add "Backup Primary" backup-primary.company.com \
  --user backup \
  --kerberos true \
  --tags backup,primary,production

# Secondary backup server
bayesian-ssh add "Backup Secondary" backup-secondary.company.com \
  --user backup \
  --kerberos true \
  --tags backup,secondary,production

# Disaster recovery server
bayesian-ssh add "DR Server" dr.company.com \
  --user dr-user \
  --kerberos true \
  --tags disaster-recovery,production
```
