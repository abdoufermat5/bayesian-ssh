# 🏗️ Technical Architecture

## Overview
Bayesian SSH is built with a modular architecture that separates concerns and provides a clean, maintainable codebase.

## Core Components

### 1. **CLI Layer** (`src/cli/`)
- **Command parsing**: Uses `clap` for robust command-line argument handling
- **Command modules**: Each command is implemented in its own module
- **Shell completions**: Automatic generation for bash, zsh, fish, and more

### 2. **Configuration Management** (`src/config/`)
- **AppConfig**: Central configuration structure
- **File-based config**: JSON configuration stored in `~/.config/bayesian-ssh/`
- **Environment overrides**: Support for environment variable configuration

### 3. **Data Models** (`src/models/`)
- **Connection**: Represents SSH connection configuration
- **Session**: Tracks active SSH sessions
- **Serialization**: Full serde support for JSON operations

### 4. **Database Layer** (`src/database/`)
- **SQLite integration**: Using `rusqlite` for data persistence
- **Connection management**: Efficient connection pooling
- **Migration support**: Schema versioning and updates

### 5. **Services** (`src/services/`)
- **SSH Service**: Core SSH connection logic
- **Kerberos integration**: Automatic ticket management
- **Process management**: Safe process spawning and monitoring

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
3. Search for connection in database
4. Verify Kerberos ticket (if enabled)
5. Automatically create ticket if needed
6. Build SSH command with proper flags
7. Execute SSH process
8. Monitor session status
9. Update database with results
10. Handle cleanup on exit
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
- **Indexed queries**: Fast lookups by name and tags
- **Connection pooling**: Efficient database connection reuse
- **Batch operations**: Group multiple database operations

### Memory Management
- **Zero-copy operations**: Minimize memory allocations
- **Efficient serialization**: Use serde for fast JSON operations
- **Resource cleanup**: Proper cleanup of file descriptors and processes

## Security Features

### Kerberos Integration
- **Ticket verification**: Check existing tickets before use
- **Automatic renewal**: Create new tickets when needed
- **Forwardable tickets**: Support for ticket forwarding

### SSH Security
- **Key management**: Secure handling of SSH keys
- **Bastion support**: Secure jump host connections
- **Process isolation**: Safe process spawning and monitoring

## Testing Strategy

### Unit Tests
- **Model validation**: Test data structure integrity
- **Service logic**: Test business logic in isolation
- **Error handling**: Test error conditions and recovery

### Integration Tests
- **Database operations**: Test full database workflows
- **SSH connections**: Test actual SSH operations
- **Configuration loading**: Test configuration file handling

## Future Architecture

### Plugin System
- **Extension points**: Hook system for custom functionality
- **Plugin API**: Stable interface for third-party extensions
- **Dynamic loading**: Runtime plugin loading and management

### API Layer
- **REST API**: HTTP interface for remote management
- **WebSocket support**: Real-time session monitoring
- **Authentication**: Secure API access control

### Microservices
- **Service separation**: Split into focused microservices
- **Message queues**: Async communication between services
- **Load balancing**: Distribute load across multiple instances
