# Environments

Environments are isolated profiles, each with its own connection database. They let you keep `personal`, `work`, `client-A`, and `client-B` connections completely separate without juggling config files.

## Listing Environments

```bash
bayesian-ssh env list
```

The currently active environment is highlighted.

## Creating an Environment

```bash
bayesian-ssh env create work
bayesian-ssh env create client-acme
```

Each environment gets its own SQLite database under the application data directory.

## Switching Environments

Switch the **active** environment (persisted across invocations):

```bash
bayesian-ssh env use work
```

Override the environment for a **single command** without changing the active one:

```bash
bayesian-ssh --env client-acme list
bayesian-ssh --env client-acme connect web-prod
```

The active environment name is shown in the TUI header and in the tracing logs.

## Removing an Environment

```bash
bayesian-ssh env remove client-acme
```

> ⚠️ This deletes the environment’s database and all connections it contains.

## TUI Integration

You can manage environments without leaving the TUI from the **Config** tab (`3`):

| Key | Action |
|-----|--------|
| `Enter` | Switch to the highlighted environment |
| `a` | Create a new environment |
| `d` | Delete the highlighted environment |

See [TUI Mode](./tui.md#config-tab) for the full keybinding list.

## Typical Workflows

**Per-client isolation**

```bash
bayesian-ssh env create acme
bayesian-ssh --env acme import ~/.ssh/acme_config
bayesian-ssh --env acme list
```

**Quick context switch**

```bash
bayesian-ssh env use work    # default environment for the session
bayesian-ssh connect db-prod # uses 'work' environment
bayesian-ssh --env personal connect home-nas  # one-off in 'personal'
```
