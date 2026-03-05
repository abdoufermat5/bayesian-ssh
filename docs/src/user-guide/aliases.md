# Connection Aliases

Create shortcuts for frequently used connections.

## Adding Aliases

```bash
bayesian-ssh alias add p1 Portail01
bayesian-ssh alias add db prod-database
bayesian-ssh alias add staging Portail-staging
```

## Using Aliases

Aliases work transparently with the `connect` command:

```bash
bayesian-ssh connect p1        # Connects to Portail01
bayesian-ssh connect db        # Connects to prod-database
```

## Listing Aliases

```bash
# List all aliases
bayesian-ssh alias list

# List aliases for a specific connection
bayesian-ssh alias list Portail01
```

## Removing Aliases

```bash
bayesian-ssh alias remove p1
```
