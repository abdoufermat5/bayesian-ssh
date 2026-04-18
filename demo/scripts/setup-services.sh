#!/usr/bin/env bash
# demo/scripts/setup-services.sh
# Set up test services on target1:
#   - nginx on port 8080 (for port-forward testing)
#   - SFTP test directory tree (for browse/upload/delete/mkdir/rename testing)
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

echo "🌐 Installing nginx..."
apt-get install -y -qq nginx > /dev/null

# ── nginx on port 8080 ───────────────────────────────────────
cat > /etc/nginx/sites-available/demo <<'EOF'
server {
    listen 8080 default_server;
    listen [::]:8080 default_server;

    root /var/www/demo;
    index index.html;

    server_name _;

    location / {
        try_files $uri $uri/ =404;
    }

    location /health {
        return 200 '{"status":"ok","host":"target1"}\n';
        add_header Content-Type application/json;
    }
}
EOF

# Create demo web root
mkdir -p /var/www/demo
cat > /var/www/demo/index.html <<'HTML'
<!DOCTYPE html>
<html>
<head><title>bayesian-ssh demo</title></head>
<body>
  <h1>bayesian-ssh target1</h1>
  <p>If you can see this, port forwarding works!</p>
</body>
</html>
HTML

# Enable site, disable default
ln -sf /etc/nginx/sites-available/demo /etc/nginx/sites-enabled/demo
rm -f /etc/nginx/sites-enabled/default

systemctl restart nginx
systemctl enable nginx

echo "   ✅ nginx listening on :8080"

# ── SFTP Test Directory ──────────────────────────────────────
SFTP_DIR="/home/testuser/sftp-test"
echo "📁 Creating SFTP test directory tree at ${SFTP_DIR}..."

mkdir -p "${SFTP_DIR}/subdir"
mkdir -p "${SFTP_DIR}/empty-dir"

cat > "${SFTP_DIR}/readme.txt" <<'TXT'
This is a test file for bayesian-ssh SFTP operations.
You can download, rename, or delete this file to verify SFTP functionality.
TXT

cat > "${SFTP_DIR}/data.csv" <<'CSV'
id,name,value
1,alpha,100
2,beta,200
3,gamma,300
4,delta,400
5,epsilon,500
CSV

cat > "${SFTP_DIR}/subdir/nested.txt" <<'TXT'
This file lives in a subdirectory.
Used to test SFTP directory navigation.
TXT

# Set ownership
chown -R testuser:testuser "${SFTP_DIR}"
chmod -R 755 "${SFTP_DIR}"

echo "   ✅ SFTP test tree created:"
tree "${SFTP_DIR}" 2>/dev/null || find "${SFTP_DIR}" -print

echo "✅ Services provisioned on $(hostname)"
