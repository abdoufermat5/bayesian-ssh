# Development Workflows

## Feature Branch Development

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

## CI/CD Pipeline Access

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

## Environment Workflow

Use tags to organize connections by environment and quickly switch contexts:

```bash
# Add servers for each environment
bayesian-ssh add "Web Dev" web-dev.company.com --tags development,web
bayesian-ssh add "Web Staging" web-staging.company.com --tags staging,web
bayesian-ssh add "Web Prod" web-prod.company.com --tags production,web

# List all development servers
bayesian-ssh list --tag development

# List all staging servers
bayesian-ssh list --tag staging

# Quickly connect to any environment
bayesian-ssh connect "Web Dev"
bayesian-ssh connect "Web Staging"
bayesian-ssh connect "Web Prod"
```

## Troubleshooting Connections

```bash
# Test basic connectivity
bayesian-ssh connect "Test Server" --debug

# Test with specific user
bayesian-ssh connect "Test Server" --user test-user

# Test with specific key
bayesian-ssh connect "Test Server" --key ~/.ssh/test-key
```
