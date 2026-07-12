# Draft: Ameliorer bssh

## Requirements (confirmed)
- L'utilisateur veut ameliorer `bssh`, ajouter de nouvelles features, et renforcer la gestion d'erreurs.

## Technical Decisions
- Brownfield Rust CLI/TUI: planifier avant implementation, car la surface couvre CLI, TUI, transports SSH/SFTP/SCP, config et base SQLite.
- Ne pas ajouter de dependance sans demande explicite.
- Prioriser des features qui reutilisent les modules existants (`src/cli/commands`, `src/services`, `src/database`, `src/tui`) plutot qu'une nouvelle architecture.
- Recommandation par defaut: premiere vague = durcissement erreurs + `doctor`/diagnostic + tests CLI/database, puis features de confort.

## Research Findings
- `Cargo.toml`: Rust 2021, CLI `clap`, SQLite `rusqlite`, TUI `ratatui`, erreurs via `anyhow` + `thiserror`.
- `src/cli/mod.rs`: commandes existantes riches: connect/add/list/edit/show/import/export/backup/restore/env/history/tui/exec/upload/download/forward/proxy/alias/close.
- `src/errors.rs`: `AppError` existe mais n'est quasiment pas utilise; la plupart des chemins publics retournent `anyhow::Result`.
- `src/main.rs`: les erreurs de commande sont loggees puis le processus sort avec code 1, sans rendu utilisateur centralise et contextualise.
- Tests existants: tests unitaires inline pour transport, known_hosts, auth, forward/proxy parsing; pas de dossier `tests/` d'integration CLI.
- Baseline locale: `cargo test` passe avec 33 tests; `cargo check` passe.
- Risques principaux: `src/database/**`, la majorite des `src/cli/commands/**`, `src/config/mod.rs`, `src/services/ssh.rs`, `src/services/transfer.rs`, et la TUI sont peu ou pas couverts.
- Documentation promet deja des erreurs avec suggestions et degradation gracieuse; l'implementation actuelle est plus fragmentee (`println!`, `eprintln!`, `anyhow`, erreurs typed locales).

## Open Questions
- Choisir les features prioritaires pour la premiere vague.
- Choisir le niveau de changement accepte pour la gestion d'erreurs: minimal CLI UX ou typage plus profond dans services/database.
- Choisir strategie de tests: TDD pour erreurs/features, tests apres, ou QA manuelle seulement.
- Par defaut, proposer 3 epics: `bssh doctor`, erreurs utilisateur actionnables, integration tests CLI avec config temporaire.

## Scope Boundaries
- INCLUDE: plan d'amelioration executable pour features + erreurs, avec validation concrete.
- EXCLUDE: implementation immediate tant que les priorites produit ne sont pas tranchees.
