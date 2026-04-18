#!/usr/bin/env bash
# demo/scripts/setup-ssh.sh
# Configure sshd for key + password + GSSAPI authentication
# and enable TCP forwarding for tunnel/proxy testing.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

SSHD_CONFIG="/etc/ssh/sshd_config"

echo "🔧 Configuring sshd..."

# Back up original
cp "$SSHD_CONFIG" "${SSHD_CONFIG}.bak"

# Write a clean sshd_config tailored for the demo
cat > "$SSHD_CONFIG" <<'EOF'
# bayesian-ssh demo sshd configuration

# Basics
Port 22
AddressFamily any
ListenAddress 0.0.0.0

# Host keys
HostKey /etc/ssh/ssh_host_ed25519_key
HostKey /etc/ssh/ssh_host_rsa_key
HostKey /etc/ssh/ssh_host_ecdsa_key

# Authentication methods
PubkeyAuthentication yes
PasswordAuthentication yes
ChallengeResponseAuthentication no
UsePAM yes

# Kerberos / GSSAPI
GSSAPIAuthentication yes
GSSAPICleanupCredentials yes
GSSAPIStrictAcceptorCheck yes
GSSAPIKeyExchange no

# Key exchange & ciphers (accept defaults, just be explicit)
KexAlgorithms curve25519-sha256,curve25519-sha256@libssh.org,ecdh-sha2-nistp256,ecdh-sha2-nistp384,ecdh-sha2-nistp521,diffie-hellman-group-exchange-sha256,diffie-hellman-group16-sha512,diffie-hellman-group18-sha512,diffie-hellman-group14-sha256

# TCP forwarding (needed for tunnel & SOCKS5 proxy testing)
AllowTcpForwarding yes
GatewayPorts no
PermitTunnel yes

# SFTP subsystem
Subsystem sftp /usr/lib/openssh/sftp-server

# Misc
PrintMotd no
AcceptEnv LANG LC_*
MaxAuthTries 6
MaxSessions 10
LoginGraceTime 120
PermitRootLogin no
StrictModes yes
EOF

# Regenerate host keys if missing
ssh-keygen -A 2>/dev/null || true

# Restart sshd (Ubuntu uses 'ssh' not 'sshd')
systemctl restart ssh
systemctl enable ssh

echo "✅ sshd configured and restarted on $(hostname)"
