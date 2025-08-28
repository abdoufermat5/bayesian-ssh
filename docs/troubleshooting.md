# Troubleshooting Guide

## Common Issues and Solutions

### Kerberos Authentication Problems

#### No Valid Kerberos Ticket Found
```bash
# Check current ticket status
klist

# Create new forwardable ticket
kinit -f -A

# Verify ticket creation
klist -s
```

**Symptoms:**
- Error: "No valid Kerberos ticket found"
- SSH connection fails with authentication error
- `klist` shows no tickets or expired tickets

**Solutions:**
1. Check ticket status: `klist -s`
2. Create new ticket: `kinit -f -A`
3. Verify realm configuration: Check `/etc/krb5.conf`
4. Check DNS resolution: Ensure realm DNS is working

#### Ticket Expired or Invalid
```bash
# Check ticket expiration
klist

# Renew existing ticket
kinit -R

# Create new ticket if renewal fails
kinit -f -A
```

**Symptoms:**
- Error: "Ticket expired"
- Authentication fails after some time
- `klist` shows expired tickets

**Solutions:**
1. Automatic renewal: `kinit -R`
2. Manual renewal: `kinit -f -A`
3. Check clock sync: Ensure system time is correct
4. Verify KDC: Check KDC server availability

### SSH Connection Issues

#### Connection Refused
```bash
# Test basic connectivity
telnet server.company.com 22

# Check SSH service status
ssh -v server.company.com

# Verify firewall rules
sudo iptables -L
```

**Symptoms:**
- Error: "Connection refused"
- SSH connection times out
- Port 22 unreachable

**Solutions:**
1. Check SSH service: `systemctl status sshd`
2. Verify port: Ensure SSH is listening on correct port
3. Check firewall: Verify firewall allows SSH traffic
4. Network connectivity: Test basic network reachability

#### Authentication Failed
```bash
# Test with verbose output
ssh -v user@server.company.com

# Check key permissions
ls -la ~/.ssh/
chmod 600 ~/.ssh/id_rsa

# Test specific key
ssh -i ~/.ssh/id_rsa user@server.company.com
```

**Symptoms:**
- Error: "Permission denied"
- Authentication fails with valid credentials
- Key-based authentication fails

**Solutions:**
1. Check key permissions: `chmod 600 ~/.ssh/id_rsa`
2. Verify key format: Ensure key is in correct format
3. Check server configuration: Verify `authorized_keys` setup
4. Test manually: Use standard SSH command to test

### Bastion Host Problems

#### Bastion Connection Fails
```bash
# Test bastion connectivity
ssh -t -A -K user@bastion.company.com

# Test with specific port
ssh -p 2222 user@bastion.company.com

# Verify bastion configuration
bayesian-ssh show "Server Name"

# Force direct connection (bypass bastion)
bayesian-ssh connect "Target Server" --no-bastion

# Check if connection is using default bastion
bayesian-ssh show "Target Server"
```

**Symptoms:**
- Error: "Bastion connection failed"
- Cannot reach target through bastion
- Bastion authentication fails

**Solutions:**
1. Test bastion directly: `ssh user@bastion.company.com`
2. Check bastion port: Verify correct port (default: 22)
3. Verify user permissions: Ensure bastion user has access
4. Check network path: Verify bastion is reachable

#### Target Host Unreachable via Bastion
```bash
# Test from bastion to target
ssh -t -A -K user@bastion.company.com "ssh user@target.company.com"

# Check routing on bastion
ssh user@bastion.company.com "route -n"

# Verify target accessibility
ssh user@bastion.company.com "ping target.company.com"
```

**Symptoms:**
- Bastion connects but target is unreachable
- Error: "No route to host"
- Connection times out to target

**Solutions:**
1. Check bastion routing: Verify bastion can reach target
2. Verify target firewall: Ensure target allows bastion traffic
3. Check network segmentation: Verify network policies
4. Test manually: Connect to bastion and test target manually

#### Unexpected Bastion Usage
```bash
# Check if connection is using default bastion
bayesian-ssh show "Server Name"

# Force direct connection
bayesian-ssh connect "Server Name" --no-bastion

# Re-add connection with explicit no-bastion flag
bayesian-ssh remove "Server Name"
bayesian-ssh add "Server Name" hostname.com --no-bastion --tags production
```

**Symptoms:**
- Connection unexpectedly goes through bastion
- Want direct connection but getting bastion routing
- Default bastion being used when not intended

**Solutions:**
1. Use --no-bastion flag: Explicitly disable bastion for specific connections
2. Check connection details: Use `bayesian-ssh show` to see bastion configuration
3. Re-add connection: Remove and re-add with correct bastion settings
4. Verify configuration: Check if default bastion is set in config

### Database Issues

#### Database Connection Failed
```bash
# Check database file
ls -la ~/.config/bayesian-ssh/

# Verify permissions
chmod 755 ~/.config/bayesian-ssh/
chmod 644 ~/.config/bayesian-ssh/history.db

# Recreate database
rm ~/.config/bayesian-ssh/history.db
bayesian-ssh stats
```

**Symptoms:**
- Error: "Database connection failed"
- Cannot save or retrieve connections
- Application crashes on database operations

**Solutions:**
1. Check file permissions: Ensure proper ownership and permissions
2. Verify disk space: Check available disk space
3. Recreate database: Remove corrupted database file
4. Check SQLite version: Ensure compatible SQLite version

#### Database Schema Issues
```bash
# Check database schema
sqlite3 ~/.config/bayesian-ssh/history.db ".schema"

# Verify table structure
sqlite3 ~/.config/bayesian-ssh/history.db "SELECT * FROM connections LIMIT 1;"
```

**Symptoms:**
- Error: "Table not found"
- Database operations fail with schema errors
- Missing tables or columns

**Solutions:**
1. Check schema: Verify table structure
2. Recreate database: Remove and recreate database
3. Check migrations: Ensure schema is up to date
4. Verify SQLite: Check SQLite version compatibility

### Configuration Problems

#### Configuration File Not Found
```bash
# Check configuration directory
ls -la ~/.config/bayesian-ssh/

# Create default configuration
bayesian-ssh config

# Verify configuration
cat ~/.config/bayesian-ssh/config.json
```

**Symptoms:**
- Error: "Configuration file not found"
- Application uses default values
- Configuration changes not saved

**Solutions:**
1. Create directory: `mkdir -p ~/.config/bayesian-ssh/`
2. Generate config: Run `bayesian-ssh config` to create default
3. Check permissions: Ensure directory is writable
4. Verify path: Check configuration file path

#### Invalid Configuration Values
```bash
# View current configuration
bayesian-ssh config

# Reset to defaults
rm ~/.config/bayesian-ssh/config.json
bayesian-ssh config

# Validate configuration
cat ~/.config/bayesian-ssh/config.json | jq .
```

**Symptoms:**
- Error: "Invalid configuration"
- Application fails to start
- Configuration values ignored

**Solutions:**
1. Validate JSON: Check JSON syntax
2. Reset configuration: Remove and recreate config file
3. Check values: Verify configuration parameter values
4. Use defaults: Start with minimal configuration

### Performance Issues

#### Slow Connection Establishment
```bash
# Check DNS resolution time
time nslookup server.company.com

# Test connection speed
time ssh -o ConnectTimeout=10 user@server.company.com

# Profile application
bayesian-ssh --log-level debug connect "Server Name"
```

**Symptoms:**
- Long connection times
- Slow response to commands
- High latency

**Solutions:**
1. Check DNS: Verify DNS resolution speed
2. Network latency: Test network performance
3. Server load: Check target server performance
4. Optimize configuration: Use connection pooling

#### High Memory Usage
```bash
# Check memory usage
ps aux | grep bayesian-ssh

# Monitor resource usage
top -p $(pgrep bayesian-ssh)

# Check for memory leaks
valgrind --tool=memcheck ./target/debug/bayesian-ssh
```

**Symptoms:**
- High memory consumption
- Application becomes unresponsive
- System memory pressure

**Solutions:**
1. Check for leaks: Monitor memory usage over time
2. Optimize queries: Review database query efficiency
3. Limit connections: Reduce concurrent connections
4. Update dependencies: Ensure latest library versions

### Network and Firewall Issues

#### Firewall Blocking Connections
```bash
# Check local firewall
sudo ufw status
sudo iptables -L

# Test port accessibility
telnet server.company.com 22
nmap -p 22 server.company.com

# Check corporate firewall
# Contact network administrator
```

**Symptoms:**
- Connection blocked by firewall
- Port 22 unreachable
- Network policy violations

**Solutions:**
1. Check local firewall: Verify local firewall settings
2. Corporate policies: Contact network administrator
3. Alternative ports: Use non-standard SSH ports
4. VPN access: Connect through corporate VPN

#### DNS Resolution Issues
```bash
# Check DNS resolution
nslookup server.company.com
dig server.company.com

# Test with IP address
ssh user@192.168.1.100

# Check /etc/hosts
cat /etc/hosts
```

**Symptoms:**
- Hostname not found
- DNS resolution failures
- Connection timeouts

**Solutions:**
1. Check DNS servers: Verify DNS configuration
2. Use IP addresses: Connect directly with IP
3. Check /etc/hosts: Verify local host entries
4. Network configuration: Check network settings

### Application Crashes

#### Segmentation Faults
```bash
# Run with debugger
gdb ./target/debug/bayesian-ssh

# Check core dumps
coredumpctl list
coredumpctl info

# Run with valgrind
valgrind --tool=memcheck ./target/debug/bayesian-ssh
```

**Symptoms:**
- Application crashes with segfault
- Core dumps generated
- Unpredictable behavior

**Solutions:**
1. Debug build: Use debug version for better error reporting
2. Check dependencies: Verify library compatibility
3. Memory issues: Look for memory corruption
4. Update Rust: Ensure latest Rust version

#### Panic Errors
```bash
# Enable backtrace
RUST_BACKTRACE=1 ./target/release/bayesian-ssh

# Check logs
tail -f ~/.config/bayesian-ssh/bayesian-ssh.log

# Run with verbose output
./target/release/bayesian-ssh --log-level debug
```

**Symptoms:**
- Rust panic errors
- Application terminates unexpectedly
- Error messages with backtraces

**Solutions:**
1. Enable backtraces: Set `RUST_BACKTRACE=1`
2. Check logs: Review application logs
3. Update code: Fix panic conditions
4. Error handling: Improve error handling

## Getting Help

### Debug Information
When reporting issues, include:
- Error messages: Complete error output
- Environment: OS version, Rust version, dependencies
- Configuration: Relevant configuration files
- Steps to reproduce: Detailed reproduction steps
- Logs: Application and system logs

### Useful Commands
```bash
# Enable debug logging
bayesian-ssh --log-level debug

# Check system information
uname -a
rustc --version
cargo --version

# Verify dependencies
ldd ./target/release/bayesian-ssh

# Check file permissions
ls -la ~/.config/bayesian-ssh/
ls -la ~/.ssh/
```

### Community Support
- GitHub Issues: Report bugs and request features
- Discussions: Ask questions and share solutions
- Documentation: Check existing documentation
- Code examples: Review example configurations
