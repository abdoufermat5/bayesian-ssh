# Changelog

All notable changes to Bayesian SSH will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
