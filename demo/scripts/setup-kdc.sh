#!/usr/bin/env bash
# demo/scripts/setup-kdc.sh
# Install and configure MIT Kerberos KDC on the bastion host.
# Creates realm BSSH.TEST, user and host principals, and exports keytabs.
set -euo pipefail

BASTION_IP="$1"
TARGET1_IP="$2"
TARGET2_IP="$3"
DOMAIN="$4"
KRB_REALM="$5"

export DEBIAN_FRONTEND=noninteractive

echo "🔐 Installing Kerberos KDC packages..."

# Pre-seed debconf to avoid interactive prompts
debconf-set-selections <<DEBEOF
krb5-config krb5-config/default_realm string ${KRB_REALM}
krb5-config krb5-config/kerberos_servers string bastion.${DOMAIN}
krb5-config krb5-config/admin_server string bastion.${DOMAIN}
krb5-config krb5-config/add_servers boolean true
DEBEOF

apt-get install -y -qq \
  krb5-kdc \
  krb5-admin-server \
  krb5-user \
  libpam-krb5 \
  > /dev/null

# ── krb5.conf ─────────────────────────────────────────────────
echo "📝 Writing /etc/krb5.conf..."
cat > /etc/krb5.conf <<KRBEOF
[libdefaults]
    default_realm = ${KRB_REALM}
    dns_lookup_realm = false
    dns_lookup_kdc = false
    ticket_lifetime = 24h
    renew_lifetime = 7d
    forwardable = true
    rdns = false

[realms]
    ${KRB_REALM} = {
        kdc = bastion.${DOMAIN}
        admin_server = bastion.${DOMAIN}
    }

[domain_realm]
    .${DOMAIN} = ${KRB_REALM}
    ${DOMAIN} = ${KRB_REALM}
KRBEOF

# ── KDC Configuration ────────────────────────────────────────
cat > /etc/krb5kdc/kdc.conf <<KDCEOF
[kdcdefaults]
    kdc_ports = 88
    kdc_tcp_ports = 88

[realms]
    ${KRB_REALM} = {
        database_name = /var/lib/krb5kdc/principal
        admin_keytab = FILE:/etc/krb5kdc/kadm5.keytab
        acl_file = /etc/krb5kdc/kadm5.acl
        key_stash_file = /etc/krb5kdc/stash
        kdc_ports = 88
        max_life = 10h 0m 0s
        max_renewable_life = 7d 0h 0m 0s
        master_key_type = aes256-cts
        supported_enctypes = aes256-cts:normal aes128-cts:normal
    }
KDCEOF

# ── ACL (allow all principals with /admin suffix to administer)
echo "*/admin@${KRB_REALM} *" > /etc/krb5kdc/kadm5.acl

# ── Create KDC Database ──────────────────────────────────────
echo "🗄️  Creating KDC database..."
kdb5_util create -s -r "${KRB_REALM}" -P "kdc-master-password" <<< "y" 2>/dev/null || true

# ── Start Services ────────────────────────────────────────────
systemctl restart krb5-kdc
systemctl restart krb5-admin-server
systemctl enable krb5-kdc
systemctl enable krb5-admin-server

# ── Create Principals ────────────────────────────────────────
echo "👤 Creating Kerberos principals..."

# User principals
kadmin.local -q "addprinc -pw testpass testuser@${KRB_REALM}" 2>/dev/null || true
kadmin.local -q "addprinc -pw demopass demouser@${KRB_REALM}" 2>/dev/null || true

# Admin principal
kadmin.local -q "addprinc -pw adminpass admin/admin@${KRB_REALM}" 2>/dev/null || true

# Host principals for target servers
kadmin.local -q "addprinc -randkey host/target1.${DOMAIN}@${KRB_REALM}" 2>/dev/null || true
kadmin.local -q "addprinc -randkey host/target2.${DOMAIN}@${KRB_REALM}" 2>/dev/null || true
kadmin.local -q "addprinc -randkey host/bastion.${DOMAIN}@${KRB_REALM}" 2>/dev/null || true

# ── Export Keytabs ────────────────────────────────────────────
# These keytabs are shared via /vagrant/keys/ so target VMs can install them.
echo "🔑 Exporting host keytabs..."
KEYTAB_DIR="/vagrant/keys"
mkdir -p "$KEYTAB_DIR"

kadmin.local -q "ktadd -k ${KEYTAB_DIR}/target1.keytab host/target1.${DOMAIN}@${KRB_REALM}" 2>/dev/null || true
kadmin.local -q "ktadd -k ${KEYTAB_DIR}/target2.keytab host/target2.${DOMAIN}@${KRB_REALM}" 2>/dev/null || true
kadmin.local -q "ktadd -k ${KEYTAB_DIR}/bastion.keytab host/bastion.${DOMAIN}@${KRB_REALM}" 2>/dev/null || true

# Install bastion's own keytab
cp "${KEYTAB_DIR}/bastion.keytab" /etc/krb5.keytab
chmod 600 /etc/krb5.keytab

# ── Verify ────────────────────────────────────────────────────
echo "📋 Kerberos principals:"
kadmin.local -q "listprincs" 2>/dev/null | grep -v "^Authenticating"

echo "✅ Kerberos KDC configured on bastion (realm: ${KRB_REALM})"
