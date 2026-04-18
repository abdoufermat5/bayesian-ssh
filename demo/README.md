# bayesian-ssh Demo Environment

A 3-VM Vagrant environment for integration testing all bayesian-ssh features:
SSH key auth, password auth, Kerberos/GSSAPI, SFTP, port forwarding, SOCKS5 proxy, and the full CLI surface.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  Host Machine                           │
│                                                         │
│   bayesian-ssh binary ◄── test-all.sh                   │
│         │                                               │
│         ▼           192.168.56.0/24                      │
│  ┌──────────────┬──────────────┬──────────────┐         │
│  │   bastion     │   target1    │   target2    │         │
│  │ .56.10        │ .56.11       │ .56.12       │         │
│  │               │              │              │         │
│  │ • Jump host   │ • nginx:8080 │ • Minimal    │         │
│  │ • Kerberos KDC│ • SFTP data  │ • Multi-host │         │
│  │ • SSH gateway │ • Primary    │ • Tag testing │         │
│  └──────────────┴──────────────┴──────────────┘         │
└─────────────────────────────────────────────────────────┘
```

| VM | IP | Role | RAM |
|----|-----|------|-----|
| `bastion` | 192.168.56.10 | Jump host + Kerberos KDC | 1 GB |
| `target1` | 192.168.56.11 | Primary target, nginx :8080, SFTP test files | 1 GB |
| `target2` | 192.168.56.12 | Secondary target (multi-host, tag testing) | 512 MB |

## Prerequisites

1. **Vagrant** (>= 2.3)
   ```bash
   vagrant --version
   ```

2. **vagrant-libvirt plugin**
   ```bash
   vagrant plugin install vagrant-libvirt
   ```

3. **libvirt / QEMU / KVM**
   ```bash
   sudo apt install -y qemu-kvm libvirt-daemon-system libvirt-dev
   sudo usermod -aG libvirt $USER
   # Log out and back in for group change to take effect
   ```

4. **Rust toolchain** (for building the binary)
   ```bash
   rustup show   # verify Rust is installed
   ```

## Quick Start

```bash
# From the project root:
cd demo

# Boot all 3 VMs (first run downloads the box image ~500 MB)
vagrant up

# Run the full test suite
bash test-all.sh

# Or use Makefile targets from project root:
cd ..
make demo-up      # vagrant up
make demo-test    # run tests
make demo-down    # vagrant destroy
```

## Test Users

| User | Password | SSH Key | Kerberos Principal |
|------|----------|---------|-------------------|
| `testuser` | `testpass` | `demo/keys/id_ed25519` | `testuser@BSSH.TEST` |
| `demouser` | `demopass` | `demo/keys/id_ed25519` | `demouser@BSSH.TEST` |

The Ed25519 keypair is auto-generated into `demo/keys/` on the first `vagrant up`.

## What Gets Tested

The `test-all.sh` runner exercises **all CLI commands**:

| # | Category | Commands Tested |
|---|----------|----------------|
| 1 | Connection mgmt | `add`, `list`, `show`, `edit`, `duplicate`, `remove` |
| 2 | Tags & aliases | `edit --add-tags`, `groups`, `alias add/list/remove` |
| 3 | Connectivity | `ping` (all 3 VMs) |
| 4 | Remote exec | `exec target1 -- hostname`, `exec -- whoami`, `exec -- uname` |
| 5 | SFTP | `upload`, `download`, verify file contents |
| 6 | Port forward | `forward -L 18080:127.0.0.1:8080`, curl through tunnel |
| 7 | SOCKS5 proxy | `proxy -D 11080`, curl via SOCKS5 |
| 8 | Import/Export | `export --format json`, `export --format toml`, `import -f` |
| 9 | Backup/Restore | `backup`, `restore --force` |
| 10 | History/Stats | `history`, `stats` |
| 11 | Configuration | `config --search-mode` |
| 12 | Environments | `env create/list/use/remove` |
| 13 | Completions | `completions bash/zsh/fish` |
| 14 | Session mgmt | `close --cleanup` |

## Manual Testing

SSH into VMs directly:

```bash
# Via Vagrant
vagrant ssh bastion
vagrant ssh target1
vagrant ssh target2

# Via SSH (using the demo key)
ssh -i keys/id_ed25519 testuser@192.168.56.11

# Via bastion (jump host)
ssh -i keys/id_ed25519 -J testuser@192.168.56.10 testuser@192.168.56.11

# With password
sshpass -p testpass ssh testuser@192.168.56.11
```

Test Kerberos:

```bash
# Get a ticket (from host, or from bastion)
vagrant ssh bastion -c "echo testpass | kinit testuser@BSSH.TEST && klist"

# Verify GSSAPI auth to target1
vagrant ssh bastion -c "ssh -K testuser@target1.bssh.test hostname"
```

Test port forwarding manually:

```bash
# From the host machine:
bayesian-ssh forward target1 -L 8888:127.0.0.1:8080
# In another terminal:
curl http://127.0.0.1:8888/health
# → {"status":"ok","host":"target1"}
```

## Provisioning Details

| Script | Runs On | Purpose |
|--------|---------|---------|
| `common.sh` | All VMs | Packages, test users, /etc/hosts, SSH key distribution |
| `setup-kdc.sh` | Bastion | Kerberos KDC: realm BSSH.TEST, principals, keytabs |
| `setup-krb-client.sh` | Targets | Kerberos client config, host keytab, PAM |
| `setup-ssh.sh` | All VMs | sshd: key + password + GSSAPI + TCP forwarding |
| `setup-services.sh` | Target1 | nginx :8080, SFTP test directory tree |

## Teardown

```bash
# Destroy all VMs
cd demo && vagrant destroy -f

# Or from project root
make demo-down

# Clean up generated keys too
rm -f demo/keys/id_ed25519 demo/keys/id_ed25519.pub demo/keys/*.keytab
```

## Troubleshooting

**VMs fail to boot**
- Check libvirt is running: `sudo systemctl status libvirtd`
- Check KVM support: `kvm-ok` or `cat /proc/cpuinfo | grep vmx`
- Verify vagrant-libvirt plugin: `vagrant plugin list`

**SSH connection refused**
- VMs may need a moment after `vagrant up`: wait 30s and retry
- Check VM status: `vagrant status`
- Check sshd inside VM: `vagrant ssh target1 -c "systemctl status sshd"`

**Kerberos auth fails**
- Ensure bastion is provisioned first: `vagrant up bastion` then `vagrant up target1 target2`
- Check KDC is running: `vagrant ssh bastion -c "systemctl status krb5-kdc"`
- Verify principals: `vagrant ssh bastion -c "kadmin.local -q listprincs"`
- Check keytab exists on target: `vagrant ssh target1 -c "klist -k /etc/krb5.keytab"`

**Port forward / proxy tests fail**
- Ensure nginx is running on target1: `vagrant ssh target1 -c "curl -s localhost:8080/health"`
- Check for port conflicts on the host: `ss -tlnp | grep 18080`
- The test uses ephemeral ports to avoid conflicts; adjust in `test-all.sh` if needed
