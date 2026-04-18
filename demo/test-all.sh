#!/usr/bin/env bash
# demo/test-all.sh
# Automated test runner for bayesian-ssh
# Exercises all CLI commands against the Vagrant demo environment.
#
# Prerequisites:
#   - Vagrant VMs are up: cd demo && vagrant up
#   - Binary is built: cargo build (done automatically below)
#
# Usage:
#   cd demo && bash test-all.sh
#   cd demo && bash test-all.sh --skip-build   # skip cargo build step
set -euo pipefail

# ── Configuration ─────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

BSSH="${PROJECT_ROOT}/target/debug/bayesian-ssh"
KEY="${SCRIPT_DIR}/keys/id_ed25519"

BASTION_IP="192.168.56.10"
TARGET1_IP="192.168.56.11"
TARGET2_IP="192.168.56.12"

TEST_ENV="demo-test-$$"
PASS=0
FAIL=0
SKIP=0
ERRORS=()

# ── Colors ────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# ── Helpers ───────────────────────────────────────────────────
pass() {
  PASS=$((PASS + 1))
  echo -e "  ${GREEN}✓${NC} $1"
}

fail() {
  FAIL=$((FAIL + 1))
  ERRORS+=("$1: $2")
  echo -e "  ${RED}✗${NC} $1"
  echo -e "    ${RED}→ $2${NC}"
}

skip() {
  SKIP=$((SKIP + 1))
  echo -e "  ${YELLOW}⊘${NC} $1 (skipped: $2)"
}

section() {
  echo ""
  echo -e "${CYAN}${BOLD}━━━ $1 ━━━${NC}"
}

# Run a command and check exit code. Usage: run_test "description" command args...
run_test() {
  local desc="$1"
  shift
  local output
  if output=$("$@" 2>&1); then
    pass "$desc"
    echo "$output"  # return via stdout for callers who capture it
    return 0
  else
    fail "$desc" "exit code $?, output: $(echo "$output" | head -3)"
    return 1
  fi
}

# Run a command and check that output contains a substring
run_test_contains() {
  local desc="$1"
  local expected="$2"
  shift 2
  local output
  if output=$("$@" 2>&1); then
    if echo "$output" | grep -qi "$expected"; then
      pass "$desc"
      return 0
    else
      fail "$desc" "output missing '$expected'"
      return 1
    fi
  else
    fail "$desc" "command failed (exit $?)"
    return 1
  fi
}

# ── Pre-flight ────────────────────────────────────────────────
echo -e "${BOLD}🧪 bayesian-ssh Integration Test Suite${NC}"
echo "   Environment: Vagrant demo (3 VMs)"
echo "   Date: $(date -Iseconds)"
echo ""

# Build unless --skip-build
if [[ "${1:-}" != "--skip-build" ]]; then
  echo "🔨 Building bayesian-ssh..."
  (cd "$PROJECT_ROOT" && cargo build 2>&1) || { echo "❌ Build failed"; exit 1; }
  echo ""
fi

if [ ! -x "$BSSH" ]; then
  echo "❌ Binary not found at $BSSH — run 'cargo build' first"
  exit 1
fi

if [ ! -f "$KEY" ]; then
  echo "❌ SSH key not found at $KEY — run 'vagrant up' first to generate keys"
  exit 1
fi

# Quick connectivity check
echo "🔍 Checking VM connectivity..."
for ip in $BASTION_IP $TARGET1_IP $TARGET2_IP; do
  if ! ssh -i "$KEY" -o StrictHostKeyChecking=no -o ConnectTimeout=5 \
       testuser@"$ip" "echo ok" &>/dev/null; then
    echo "❌ Cannot SSH to $ip — are VMs up? (cd demo && vagrant up)"
    exit 1
  fi
done
echo -e "${GREEN}   All 3 VMs reachable${NC}"

# ── Create isolated test environment ─────────────────────────
section "Environment Setup"
$BSSH env create "$TEST_ENV" 2>/dev/null || true
$BSSH env use "$TEST_ENV" 2>/dev/null
pass "Created and activated test env: $TEST_ENV"

# ══════════════════════════════════════════════════════════════
#  1. CONNECTION MANAGEMENT
# ══════════════════════════════════════════════════════════════
section "1. Connection Management (add, list, show, edit, duplicate, remove)"

# Add connections
run_test "add target1" \
  $BSSH add "target1" "$TARGET1_IP" -u testuser -i "$KEY" -t demo -t primary || true

run_test "add target2" \
  $BSSH add "target2" "$TARGET2_IP" -u testuser -i "$KEY" -t demo -t secondary || true

run_test "add bastion" \
  $BSSH add "bastion" "$BASTION_IP" -u testuser -i "$KEY" -t demo -t infra || true

# Add connection with bastion
run_test "add target1-via-bastion" \
  $BSSH add "target1-bastion" "$TARGET1_IP" -u testuser -i "$KEY" \
    -b "$BASTION_IP" -B testuser -t demo -t bastion-route || true

# List
run_test_contains "list all" "target1" $BSSH list || true
run_test_contains "list --tag demo" "target2" $BSSH list -t demo || true
run_test_contains "list --detailed" "target1" $BSSH list -d || true

# Show
run_test_contains "show target1" "$TARGET1_IP" $BSSH show target1 || true

# Edit
run_test "edit target2 port" $BSSH edit target2 --port 2222 || true
run_test_contains "show target2 after edit" "2222" $BSSH show target2 || true
# Revert port so other tests work
run_test "revert target2 port" $BSSH edit target2 --port 22 || true

# Duplicate
run_test "duplicate target1" $BSSH duplicate target1 "target1-copy" || true
run_test_contains "list shows duplicate" "target1-copy" $BSSH list || true

# Remove the duplicate
run_test "remove duplicate" $BSSH remove target1-copy --force || true

# ══════════════════════════════════════════════════════════════
#  2. TAGS, GROUPS & ALIASES
# ══════════════════════════════════════════════════════════════
section "2. Tags, Groups & Aliases"

# Edit tags
run_test "add tags to target1" $BSSH edit target1 --add-tags production --add-tags web || true

# Groups
run_test_contains "groups lists demo" "demo" $BSSH groups || true
run_test_contains "groups demo members" "target1" $BSSH groups demo || true

# Aliases
run_test "alias add t1" $BSSH alias add t1 target1 || true
run_test "alias add jump" $BSSH alias add jump bastion || true
run_test_contains "alias list" "t1" $BSSH alias list || true
run_test "alias remove t1" $BSSH alias remove t1 || true

# ══════════════════════════════════════════════════════════════
#  3. CONNECTIVITY
# ══════════════════════════════════════════════════════════════
section "3. Connectivity (ping)"

run_test_contains "ping target1" "reachable\|success\|ok\|ms" $BSSH ping target1 || true
run_test_contains "ping target2" "reachable\|success\|ok\|ms" $BSSH ping target2 || true
run_test_contains "ping bastion" "reachable\|success\|ok\|ms" $BSSH ping bastion || true

# ══════════════════════════════════════════════════════════════
#  4. REMOTE EXECUTION
# ══════════════════════════════════════════════════════════════
section "4. Remote Execution (exec)"

run_test_contains "exec hostname on target1" "target1" \
  $BSSH exec target1 -- hostname || true

run_test_contains "exec whoami on target1" "testuser" \
  $BSSH exec target1 -- whoami || true

run_test_contains "exec uname on target2" "Linux" \
  $BSSH exec target2 -- uname -s || true

# ══════════════════════════════════════════════════════════════
#  5. SFTP OPERATIONS
# ══════════════════════════════════════════════════════════════
section "5. SFTP Operations (upload, download)"

UPLOAD_SRC="${SCRIPT_DIR}/test-data/upload-test.txt"
UPLOAD_REMOTE="/home/testuser/uploaded-test.txt"
DOWNLOAD_LOCAL="/tmp/bssh-download-test-$$.txt"

# Upload
run_test "upload file to target1" \
  $BSSH upload target1 "$UPLOAD_SRC" "$UPLOAD_REMOTE" || true

# Verify upload landed
if ssh -i "$KEY" -o StrictHostKeyChecking=no testuser@"$TARGET1_IP" \
   "test -f $UPLOAD_REMOTE" 2>/dev/null; then
  pass "uploaded file exists on remote"
else
  fail "uploaded file exists on remote" "file not found at $UPLOAD_REMOTE"
fi

# Download
run_test "download /etc/hostname from target1" \
  $BSSH download target1 /etc/hostname "$DOWNLOAD_LOCAL" || true

if [ -f "$DOWNLOAD_LOCAL" ]; then
  if grep -q "target1" "$DOWNLOAD_LOCAL"; then
    pass "downloaded file contains expected content"
  else
    fail "downloaded file contains expected content" "content: $(cat "$DOWNLOAD_LOCAL")"
  fi
  rm -f "$DOWNLOAD_LOCAL"
else
  fail "downloaded file exists locally" "file not written to $DOWNLOAD_LOCAL"
fi

# Clean up remote upload
ssh -i "$KEY" -o StrictHostKeyChecking=no testuser@"$TARGET1_IP" \
  "rm -f $UPLOAD_REMOTE" 2>/dev/null || true

# ══════════════════════════════════════════════════════════════
#  6. PORT FORWARDING
# ══════════════════════════════════════════════════════════════
section "6. Port Forwarding (forward)"

LOCAL_FWD_PORT=18080

# Start tunnel in background
$BSSH forward target1 -L "127.0.0.1:${LOCAL_FWD_PORT}:127.0.0.1:8080" &
FWD_PID=$!
sleep 2

# Test the tunnel
if curl -sf "http://127.0.0.1:${LOCAL_FWD_PORT}/health" 2>/dev/null | grep -q "ok"; then
  pass "port-forward tunnel works (localhost:${LOCAL_FWD_PORT} → target1:8080)"
else
  fail "port-forward tunnel works" "curl to forwarded port failed"
fi

# Clean up
kill "$FWD_PID" 2>/dev/null || true
wait "$FWD_PID" 2>/dev/null || true

# ══════════════════════════════════════════════════════════════
#  7. SOCKS5 PROXY
# ══════════════════════════════════════════════════════════════
section "7. SOCKS5 Proxy"

SOCKS_PORT=11080

# Start proxy in background
$BSSH proxy target1 -D "$SOCKS_PORT" &
PROXY_PID=$!
sleep 2

# Test the proxy — curl through SOCKS5 to target1's nginx
if curl -sf --socks5 "127.0.0.1:${SOCKS_PORT}" "http://${TARGET1_IP}:8080/health" 2>/dev/null | grep -q "ok"; then
  pass "SOCKS5 proxy works (localhost:${SOCKS_PORT} → target1:8080)"
else
  fail "SOCKS5 proxy works" "curl via SOCKS5 failed"
fi

# Clean up
kill "$PROXY_PID" 2>/dev/null || true
wait "$PROXY_PID" 2>/dev/null || true

# ══════════════════════════════════════════════════════════════
#  8. IMPORT / EXPORT
# ══════════════════════════════════════════════════════════════
section "8. Import / Export"

EXPORT_FILE="/tmp/bssh-export-$$.json"
EXPORT_TOML="/tmp/bssh-export-$$.toml"

# Export JSON
run_test "export to JSON" $BSSH export --format json -o "$EXPORT_FILE" || true
if [ -f "$EXPORT_FILE" ] && [ -s "$EXPORT_FILE" ]; then
  if jq empty "$EXPORT_FILE" 2>/dev/null; then
    pass "exported JSON is valid"
  else
    fail "exported JSON is valid" "jq parse failed"
  fi
else
  fail "export file created" "file missing or empty"
fi

# Export TOML
run_test "export to TOML" $BSSH export --format toml -o "$EXPORT_TOML" || true

# Import SSH config (into a fresh env to avoid conflicts)
SSH_CONFIG_SAMPLE="${SCRIPT_DIR}/test-data/ssh_config_sample"
if [ -f "$SSH_CONFIG_SAMPLE" ]; then
  run_test "import SSH config" $BSSH import -f "$SSH_CONFIG_SAMPLE" --no-bastion || true
else
  skip "import SSH config" "sample file missing"
fi

# Clean up
rm -f "$EXPORT_FILE" "$EXPORT_TOML"

# ══════════════════════════════════════════════════════════════
#  9. BACKUP / RESTORE
# ══════════════════════════════════════════════════════════════
section "9. Backup / Restore"

BACKUP_FILE="/tmp/bssh-backup-$$.db"

run_test "backup database" $BSSH backup -o "$BACKUP_FILE" || true

if [ -f "$BACKUP_FILE" ] && [ -s "$BACKUP_FILE" ]; then
  pass "backup file created and non-empty"
  run_test "restore database" $BSSH restore "$BACKUP_FILE" --force || true
else
  fail "backup file created" "file missing or empty"
  skip "restore database" "no backup to restore"
fi

rm -f "$BACKUP_FILE"

# ══════════════════════════════════════════════════════════════
# 10. HISTORY & STATS
# ══════════════════════════════════════════════════════════════
section "10. History & Stats"

run_test "history" $BSSH history -n 5 || true
run_test "stats" $BSSH stats || true

# ══════════════════════════════════════════════════════════════
# 11. CONFIGURATION
# ══════════════════════════════════════════════════════════════
section "11. Configuration"

run_test "config set search-mode" $BSSH config --search-mode fuzzy || true
run_test "config revert search-mode" $BSSH config --search-mode bayesian || true

# ══════════════════════════════════════════════════════════════
# 12. ENVIRONMENT MANAGEMENT
# ══════════════════════════════════════════════════════════════
section "12. Environment Management"

run_test_contains "env list" "$TEST_ENV" $BSSH env list || true

# ══════════════════════════════════════════════════════════════
# 13. COMPLETIONS (just verify generation, not installation)
# ══════════════════════════════════════════════════════════════
section "13. Shell Completions"

for shell in bash zsh fish; do
  if $BSSH completions "$shell" > /dev/null 2>&1; then
    pass "completions $shell"
  else
    fail "completions $shell" "generation failed"
  fi
done

# ══════════════════════════════════════════════════════════════
# 14. CLOSE SESSIONS
# ══════════════════════════════════════════════════════════════
section "14. Close Sessions"

run_test "close --cleanup" $BSSH close --cleanup || true

# ══════════════════════════════════════════════════════════════
#  CLEANUP
# ══════════════════════════════════════════════════════════════
section "Cleanup"

# Switch back to default env and remove test env
$BSSH env use default 2>/dev/null || true
$BSSH env remove "$TEST_ENV" 2>/dev/null || true
pass "Cleaned up test env: $TEST_ENV"

# Kill any lingering background processes
jobs -p 2>/dev/null | xargs -r kill 2>/dev/null || true

# ══════════════════════════════════════════════════════════════
#  SUMMARY
# ══════════════════════════════════════════════════════════════
echo ""
echo -e "${BOLD}═══════════════════════════════════════════════════════${NC}"
TOTAL=$((PASS + FAIL + SKIP))
echo -e "${BOLD}  Test Results: ${TOTAL} total${NC}"
echo -e "  ${GREEN}✓ Passed:  ${PASS}${NC}"
echo -e "  ${RED}✗ Failed:  ${FAIL}${NC}"
echo -e "  ${YELLOW}⊘ Skipped: ${SKIP}${NC}"
echo -e "${BOLD}═══════════════════════════════════════════════════════${NC}"

if [ ${#ERRORS[@]} -gt 0 ]; then
  echo ""
  echo -e "${RED}${BOLD}Failures:${NC}"
  for err in "${ERRORS[@]}"; do
    echo -e "  ${RED}• ${err}${NC}"
  done
fi

echo ""
if [ "$FAIL" -eq 0 ]; then
  echo -e "${GREEN}${BOLD}🎉 All tests passed!${NC}"
  exit 0
else
  echo -e "${RED}${BOLD}💥 ${FAIL} test(s) failed${NC}"
  exit 1
fi
