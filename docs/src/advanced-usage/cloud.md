# Cloud Infrastructure

## AWS EC2

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
  --user currentuser \
  --kerberos false \
  --key ~/.ssh/ec2-app-key.pem \
  --tags ec2,production,application
```

### EC2 via Bastion (Private Subnets)

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

## Multi-Cloud Setup (AWS + Azure + GCP)

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
  --user currentuser \
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
