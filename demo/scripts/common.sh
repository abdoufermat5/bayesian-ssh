#!/usr/bin/env bash
# demo/scripts/common.sh
# Base provisioner for all VMs: packages, test users, /etc/hosts, SSH key distribution
set -euo pipefail

BASTION_IP="$1"
TARGET1_IP="$2"
TARGET2_IP="$3"
DOMAIN="$4"

export DEBIAN_FRONTEND=noninteractive

echo "📦 Installing base packages..."
apt-get update -qq
apt-get install -y -qq \
  openssh-server \
  openssh-client \
  curl \
  wget \
  net-tools \
  dnsutils \
  jq \
  tree \
  sshpass \
  > /dev/null

# ── /etc/hosts ────────────────────────────────────────────────
echo "🌐 Configuring /etc/hosts..."
cat >> /etc/hosts <<EOF

# bayesian-ssh demo network
${BASTION_IP}  bastion  bastion.${DOMAIN}
${TARGET1_IP}  target1  target1.${DOMAIN}
${TARGET2_IP}  target2  target2.${DOMAIN}
EOF

# ── Test Users ────────────────────────────────────────────────
create_user() {
  local username="$1"
  local password="$2"

  if ! id "$username" &>/dev/null; then
    echo "👤 Creating user: $username"
    useradd -m -s /bin/bash "$username"
    echo "${username}:${password}" | chpasswd
  fi

  # SSH directory
  local ssh_dir="/home/${username}/.ssh"
  mkdir -p "$ssh_dir"
  chmod 700 "$ssh_dir"

  # Distribute the generated public key
  local pubkey_file="/vagrant/keys/id_ed25519.pub"
  if [ -f "$pubkey_file" ]; then
    cp "$pubkey_file" "${ssh_dir}/authorized_keys"
    chmod 600 "${ssh_dir}/authorized_keys"
  fi

  chown -R "${username}:${username}" "$ssh_dir"
}

create_user "testuser" "testpass"
create_user "demouser" "demopass"

# Also allow the vagrant user to use the demo key
if [ -f /vagrant/keys/id_ed25519.pub ]; then
  mkdir -p /home/vagrant/.ssh
  cat /vagrant/keys/id_ed25519.pub >> /home/vagrant/.ssh/authorized_keys
  sort -u -o /home/vagrant/.ssh/authorized_keys /home/vagrant/.ssh/authorized_keys
  chown -R vagrant:vagrant /home/vagrant/.ssh
fi

echo "✅ Common provisioning complete on $(hostname)"
