# Changelog

All notable changes to Bayesian SSH will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.0] - 2025-12-30

### Added
- **Bayesian-ranked search**: Smart connection ranking combining:
  - Prior probability (usage frequency with Laplace smoothing)
  - Likelihood (match quality: exact, prefix, word-boundary, contains)
  - Recency (exponential decay based on last use)
  - Success rate (connections that work get boosted)- **Configurable search mode** (`bssh config --search-mode bayesian|fuzzy`)
  - `bayesian`: Smart ranking based on usage patterns (default)
  - `fuzzy`: Simple pattern matching
- **Assets**: SVG icons, banner, architecture and workflow diagrams

### Changed
- Default search mode is now "bayesian" for smarter results

## [1.3.0] - 2025-12-30

### Added- **Interactive TUI mode** (`bssh tui`): Full-screen terminal interface with ratatui
  - Browse, search, and connect to servers
  - Keyboard navigation (vim-style j/k, arrows, PgUp/PgDn)
  - Tag filtering, help overlay, confirmation dialogs
- **Session history command** (`bssh history`): View connection history with statistics
  - Success/failure rates, average duration
  - Filter by connection, days, failed-only
- **Connection aliases** (`bssh alias`): Create shortcuts for connections
  - `bssh alias add db prod-database` → `bssh connect db`
  - Aliases work transparently with connect command
- **Close/kill sessions** (`bssh close`): Manage active SSH sessions
  - List active sessions with PID and stale detection
  - Close specific sessions or all at once
  - `--cleanup` to remove stale sessions (PIDs no longer running)
- **Configurable search mode** (`bssh config --search-mode bayesian|fuzzy`)
  - `bayesian`: Smart ranking based on usage patterns (default)
  - `fuzzy`: Simple pattern matching

### Changed
- Connect command now checks aliases before fuzzy search
- Session tracking improved with accurate active/stale detection
- Default search mode is now "bayesian" for smarter results

### Dependencies
- Added `ratatui` and `crossterm` for TUI
- Enabled `signal` feature in `nix` for process management

## [1.2.0] - 2025-12-22

### Added
- **`--force` flag for remove command**: Skip confirmation prompt with `-f` or `--force`
- **`--clear-bastion` flag for config command**: Clear default bastion settings
- **Shared CLI utilities module**: New `src/cli/utils.rs` for consistent UX across commands
- **Working "search again" feature**: The 's' option in interactive selection now performs actual recursive search
- **Contextual help messages**: Suggestions like "Use 'bssh list' to see all connections" when no matches found

### Changed
- **Single-match auto-connect**: Connect and show commands now auto-select when only one fuzzy match is found (improved UX)
- **Default yes for non-destructive operations**: Confirmation prompts now default to Yes `[Y/n]` for show/connect
- **Simplified remove confirmation**: Changed from typing full connection name to simple y/n prompt (use `--force` to skip)

### Fixed
- **Config update bug**: Fixed double-wrapping of `Option<Option<String>>` for bastion settings that prevented clearing values
- **DateTime parsing panic**: Graceful handling of malformed dates in database instead of crashing
- **Home directory panic**: Better error message when `$HOME` is not set during SSH config import

### Technical
- **Major code deduplication**: Extracted ~300 lines of duplicated code from connect.rs, edit.rs, remove.rs, show.rs into shared utilities
- **Reduced file sizes**: connect.rs (257→65 lines), edit.rs (360→70 lines), remove.rs (235→70 lines), show.rs (246→35 lines)

## [1.1.1] - 2025-08-29

### Fixed
- **Configuration defaults**: Changed default user from hardcoded "admin" to current system user
- **Kerberos default**: Disabled Kerberos by default (changed from `true` to `false`)
- **Documentation**: Updated all examples to reflect new sensible defaults

### Changed
- **Default configuration**: Application now uses current Linux username instead of "admin"
- **Kerberos behavior**: Kerberos authentication is now opt-in rather than default
- **Documentation examples**: Updated configuration commands and JSON examples across all docs

### Technical
- **Dependencies**: Added `whoami` crate for system user detection
- **Configuration**: Updated `AppConfig::default()` implementation
- **Documentation**: Updated README.md, docs/README.md, and docs/advanced-usage.md

## [1.1.0] - 2025-08-28

### Added
- **Intelligent fuzzy search across all commands** - Find connections by partial names, tags, or patterns
  - Enhanced `connect` command with fuzzy search and interactive selection
  - Enhanced `edit` command with fuzzy search for connection management
  - Enhanced `show` command with fuzzy search for connection details
  - Enhanced `remove` command with fuzzy search and extra confirmation

### Fuzzy Search Features
- **Smart pattern matching**: Handles hyphens, underscores, and separators (`webprod` → `web-prod-server`)
- **Tag-based search**: Search within connection tags
- **Recent connections fallback**: Shows recently used connections when no matches found
- **Interactive selection**: Numbered menus for multiple matches with user-friendly prompts
- **Relevance ranking**: Prioritizes recently used and exact matches

### Enhanced Safety
- **Extra confirmation for destructive operations**: `remove` command requires typing full connection name
- **Graceful error handling**: Clear messages and helpful suggestions
- **Backwards compatibility**: All existing functionality preserved

### Documentation
- Updated README with fuzzy search examples across all commands
- Enhanced user guide with practical usage scenarios
- Improved feature descriptions and examples

### Technical Improvements
- Enhanced database layer with fuzzy search algorithms
- Improved error handling and user feedback
- Better code organization and maintainability

## [1.0.0] - 2024-08-23

### Added
- Initial release of Bayesian SSH
- Basic SSH connection management
- Kerberos authentication support
- Bastion host routing
- Tag-based organization
- SQLite database persistence
- Connection history and statistics

### Core Features
- One-click connections to servers
- Automatic Kerberos ticket management
- Smart bastion host routing
- Tag-based organization for easy management
- Complete connection history with statistics
- SQLite database for persistence

---

## Types of changes
- `Added` for new features
- `Changed` for changes in existing functionality
- `Fixed` for any bug fixes
- `Removed` for now removed features
- `Security` in case of vulnerabilities
