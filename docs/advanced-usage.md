# Advanced Usage Examples

## Enterprise Environment with Bastion

### Default Configuration
```bash
# Set up default enterprise configuration
bayesian-ssh config \
  --default-user admin \
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

### Multi-Environment Management
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

## Cloud Environment (AWS EC2)

### Direct EC2 Instances
```bash
# Web server instance
bayesian-ssh add "Web EC2" ec2-web.company.com \
  --user ubuntu \
  --kerberos false \
  --key ~/.ssh/ec2-web-key.pem \
  --tags ec2,production,web

# Database instance
bayesian-ssh add "DB EC2" ec2-db.company.com \
  --user ec2-user \
  --kerberos false \
  --key ~/.ssh/ec2-db-key.pem \
  --tags ec2,production,database

# Application instance
bayesian-ssh add "App EC2" ec2-app.company.com \
  --user admin \
  --kerberos false \
  --key ~/.ssh/ec2-app-key.pem \
  --tags ec2,production,application
```

### EC2 via Bastion
```bash
# Private subnet instances
bayesian-ssh add "Private Web" private-web.company.com \
  --user ubuntu \
  --kerberos false \
  --key ~/.ssh/private-key.pem \
  --bastion bastion.company.com \
  --tags ec2,private,production,web

# VPC instances
bayesian-ssh add "VPC App" vpc-app.company.com \
  --user ec2-user \
  --kerberos false \
  --key ~/.ssh/vpc-key.pem \
  --bastion vpc-bastion.company.com \
  --tags ec2,vpc,production,application
```

## Kubernetes and Container Environments

### Pod Access
```bash
# Access to Kubernetes pods
bayesian-ssh add "K8s Web Pod" web-pod.namespace.svc.cluster.local \
  --user root \
  --kerberos false \
  --tags kubernetes,pod,web

# Service access
bayesian-ssh add "K8s Service" web-service.namespace.svc.cluster.local \
  --user admin \
  --kerberos false \
  --tags kubernetes,service,web
```

### Docker Containers
```bash
# Development container
bayesian-ssh add "Dev Container" dev-container.company.com \
  --user developer \
  --port 2222 \
  --kerberos false \
  --tags docker,development

# Production container
bayesian-ssh add "Prod Container" prod-container.company.com \
  --user operator \
  --port 2222 \
  --kerberos false \
  --tags docker,production
```

## Multi-Cloud Setup

### AWS + Azure + GCP
```bash
# AWS instances (direct connection)
bayesian-ssh add "AWS Web" aws-web.company.com \
  --user ubuntu \
  --kerberos false \
  --key ~/.ssh/aws-key.pem \
  --no-bastion \
  --tags aws,production,web

# Azure VMs (direct connection)
bayesian-ssh add "Azure DB" azure-db.company.com \
  --user azureuser \
  --kerberos false \
  --key ~/.ssh/azure-key.pem \
  --no-bastion \
  --tags azure,production,database

# GCP instances (direct connection)
bayesian-ssh add "GCP App" gcp-app.company.com \
  --user gcp-user \
  --kerberos false \
  --key ~/.ssh/gcp-key.pem \
  --no-bastion \
  --tags gcp,production,application
```

## High Availability and Load Balancing

### Load Balancer Backends
```bash
# Primary load balancer
bayesian-ssh add "LB Primary" lb-primary.company.com \
  --user admin \
  --tags loadbalancer,primary,production

# Secondary load balancer
bayesian-ssh add "LB Secondary" lb-secondary.company.com \
  --user admin \
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

## Security and Compliance

### Audit and Monitoring
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

### Compliance Environments
```bash
# SOX compliant servers
bayesian-ssh add "SOX Server" sox.company.com \
  --user sox-user \
  --kerberos true \
  --tags sox,compliance,production

# PCI compliant servers
bayesian-ssh add "PCI Server" pci.company.com \
  --user pci-user \
  --kerberos true \
  --tags pci,compliance,production

# HIPAA compliant servers
bayesian-ssh add "HIPAA Server" hipaa.company.com \
  --user hipaa-user \
  --kerberos true \
  --tags hipaa,compliance,production
```

## Development Workflows

### Feature Branch Development
```bash
# Feature development server
bayesian-ssh add "Feature Dev" feature-dev.company.com \
  --user developer \
  --bastion dev-bastion.company.com \
  --tags development,feature

# Integration testing
bayesian-ssh add "Integration Test" integration.company.com \
  --user tester \
  --bastion test-bastion.company.com \
  --tags testing,integration

# Staging for QA
bayesian-ssh add "QA Staging" qa-staging.company.com \
  --user qa-user \
  --bastion qa-bastion.company.com \
  --tags testing,staging,qa
```

### CI/CD Pipeline Access
```bash
# Jenkins server
bayesian-ssh add "Jenkins" jenkins.company.com \
  --user jenkins \
  --kerberos false \
  --tags ci,jenkins,production

# GitLab server
bayesian-ssh add "GitLab" gitlab.company.com \
  --user git \
  --kerberos false \
  --tags ci,gitlab,production

# Artifactory server
bayesian-ssh add "Artifactory" artifactory.company.com \
  --user artifact \
  --kerberos false \
  --tags ci,artifactory,production
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

## Backup and Recovery

### Backup Servers
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

## Performance Optimization

### Connection Pooling
```bash
# High-performance servers
bayesian-ssh add "Perf Server 1" perf-1.company.com \
  --user perf-user \
  --kerberos false \
  --tags performance,production

bayesian-ssh add "Perf Server 2" perf-2.company.com \
  --user perf-user \
  --kerberos false \
  --tags performance,production
```

### Load Distribution
```bash
# Round-robin load distribution
bayesian-ssh add "Load 1" load-1.company.com \
  --user load-user \
  --kerberos false \
  --tags load,production

bayesian-ssh add "Load 2" load-2.company.com \
  --user load-user \
  --kerberos false \
  --tags load,production

bayesian-ssh add "Load 3" load-3.company.com \
  --user load-user \
  --kerberos false \
  --tags load,production
```

## Troubleshooting Scenarios

### Connection Issues
```bash
# Test basic connectivity
bayesian-ssh connect "Test Server" --debug

# Test with specific user
bayesian-ssh connect "Test Server" --user test-user

# Test with specific key
bayesian-ssh connect "Test Server" --key ~/.ssh/test-key
```

### Kerberos Issues
```bash
# Test Kerberos authentication
bayesian-ssh connect "Kerberos Server" --kerberos

# Test without Kerberos
bayesian-ssh connect "Kerberos Server" --no-kerberos

# Debug Kerberos
klist
kinit -f
```

### Bastion Issues
```bash
# Test bastion connectivity
ssh -t -A -K user@bastion.company.com

# Test with specific bastion user
bayesian-ssh connect "Target Server" --bastion-user admin

# Test bastion port
bayesian-ssh connect "Target Server" --bastion-port 2222
```
