#!/usr/bin/env bash
# demo/scripts/setup-krb-client.sh
# Install Kerberos client libraries, configure krb5.conf, install host keytab,
# and enable PAM GSSAPI authentication on target servers.
set -euo pipefail

BASTION_IP="$1"
DOMAIN="$2"
KRB_REALM="$3"

export DEBIAN_FRONTEND=noninteractive

echo "🔐 Installing Kerberos client packages..."

# Pre-seed debconf
debconf-set-selections <<DEBEOF
krb5-config krb5-config/default_realm string ${KRB_REALM}
krb5-config krb5-config/kerberos_servers string bastion.${DOMAIN}
krb5-config krb5-config/admin_server string bastion.${DOMAIN}
krb5-config krb5-config/add_servers boolean true
DEBEOF

apt-get install -y -qq \
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

# ── Install Host Keytab ──────────────────────────────────────
HOSTNAME=$(hostname -s)
KEYTAB_SRC="/vagrant/keys/${HOSTNAME}.keytab"

if [ -f "$KEYTAB_SRC" ]; then
  echo "🔑 Installing host keytab from ${KEYTAB_SRC}..."
  cp "$KEYTAB_SRC" /etc/krb5.keytab
  chmod 600 /etc/krb5.keytab
else
  echo "⚠️  Keytab not found at ${KEYTAB_SRC} — Kerberos auth may not work."
  echo "   Ensure bastion is provisioned first (vagrant up bastion)."
fi

# ── PAM Configuration ────────────────────────────────────────
# Ensure pam_krb5 is loaded for SSH sessions
if ! grep -q "pam_krb5" /etc/pam.d/common-auth 2>/dev/null; then
  echo "📝 Adding pam_krb5 to PAM auth stack..."
  echo "auth    sufficient    pam_krb5.so minimum_uid=1000" >> /etc/pam.d/common-auth
fi

if ! grep -q "pam_krb5" /etc/pam.d/common-session 2>/dev/null; then
  echo "session optional    pam_krb5.so minimum_uid=1000" >> /etc/pam.d/common-session
fi

echo "✅ Kerberos client configured on $(hostname) (realm: ${KRB_REALM})"
