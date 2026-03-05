# Technical Architecture

## Overview

Bayesian SSH is built with a modular architecture that separates concerns and provides a clean, maintainable codebase.

## Core Components

### 1. CLI Layer (`src/cli/`)

- **Command parsing**: Uses `clap` for robust command-line argument handling
- **Command modules**: Each command is implemented in its own module
- **Shell completions**: Automatic generation for bash, zsh, fish, and more
- **Shared utilities**: Common CLI patterns extracted into `src/cli/utils.rs`

### 2. Configuration Management (`src/config/`)

- **AppConfig**: Central configuration structure
- **File-based config**: JSON configuration stored in `~/.config/bayesian-ssh/`
- **Environment overrides**: Support for environment variable configuration

### 3. Data Models (`src/models/`)

- **Connection**: Represents SSH connection configuration
- **Session**: Tracks active SSH sessions
- **Serialization**: Full serde support for JSON operations

### 4. Database Layer (`src/database/`)

- **SQLite integration**: Using `rusqlite` for data persistence
- **Modular design**: Split into submodules - `connection`, `alias`, `session`, `search`
- **Migration support**: Schema versioning and updates

### 5. Services (`src/services/`)

- **SSH Service**: Core SSH connection logic
- **Kerberos integration**: Automatic ticket management
- **Process management**: Safe process spawning and monitoring

### 6. TUI (`src/tui/`)

- **ratatui-based**: Full-screen terminal interface
- **Application state**: Managed in `app.rs`
- **Rendering**: UI layout and drawing in `ui.rs`

## Database Schema

### Connections Table

```sql
CREATE TABLE connections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    host TEXT NOT NULL,
    user TEXT NOT NULL,
    port INTEGER NOT NULL,
    bastion TEXT,
    bastion_user TEXT,
    use_kerberos BOOLEAN NOT NULL,
    key_path TEXT,
    created_at TEXT NOT NULL,
    last_used TEXT,
    tags TEXT NOT NULL
);
```

### Sessions Table

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    connection_id TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL,
    pid INTEGER,
    exit_code INTEGER
);
```

## Connection Workflow

```
1. Parse command line arguments
2. Load application configuration
3. Search for connection in database (Bayesian-ranked or fuzzy)
4. Check aliases if no direct match
5. Verify Kerberos ticket (if enabled)
6. Automatically create ticket if needed
7. Build SSH command with proper flags
8. Execute SSH process
9. Monitor session status
10. Update database with results
11. Handle cleanup on exit
```

## Error Handling

### Error Types

- **Configuration errors**: Invalid settings or missing files
- **Database errors**: Connection issues or schema problems
- **SSH errors**: Connection failures or authentication issues
- **Kerberos errors**: Ticket creation or renewal failures

### Error Recovery

- **Graceful degradation**: Fallback to basic SSH if features fail
- **Automatic retry**: Retry failed operations with exponential backoff
- **User feedback**: Clear error messages with suggested solutions

## Performance Considerations

### Database Optimization

- Indexed queries for fast lookups by name and tags
- Efficient connection reuse
- Batch operations for grouped database operations

### Memory Management

- Zero-copy operations to minimize memory allocations
- Efficient serialization with serde
- Proper cleanup of file descriptors and processes

## Security Features

### Kerberos Integration

- Ticket verification before use
- Automatic renewal when needed
- Forwardable ticket support

### SSH Security

- Secure handling of SSH keys
- Bastion host support for jump connections
- Safe process spawning and monitoring

## Testing Strategy

### Unit Tests

- Model validation and data structure integrity
- Service logic tested in isolation
- Error condition and recovery testing

### Integration Tests

- Full database workflow testing
- SSH connection testing
- Configuration file handling

## Future Architecture

### Plugin System

- Extension points with hook system for custom functionality
- Stable plugin API for third-party extensions
- Runtime plugin loading and management

### API Layer

- REST API for remote management
- WebSocket support for real-time session monitoring
- Secure API access control
