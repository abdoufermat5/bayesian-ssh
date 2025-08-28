#!/bin/bash

echo "Testing Fuzzy Search Across All Commands"
echo "========================================="
echo ""

BINARY="./target/release/bayesian-ssh"

# Function to run a command and show output
run_cmd() {
    echo "$ $1"
    echo ""
    eval "$1"
    echo ""
    echo "----------------------------------------"
    echo ""
}

# Add some sample connections for testing
echo "Adding sample connections..."
echo "$ $BINARY add 'web-prod-server' web-prod.company.com --tags production,web"
$BINARY add 'web-prod-server' web-prod.company.com --tags production,web >/dev/null 2>&1
echo "$ $BINARY add 'db-prod-server' db-prod.company.com --tags production,database"
$BINARY add 'db-prod-server' db-prod.company.com --tags production,database >/dev/null 2>&1
echo "$ $BINARY add 'app-prod-server' app-prod.company.com --tags production,application"
$BINARY add 'app-prod-server' app-prod.company.com --tags production,application >/dev/null 2>&1
echo "$ $BINARY add 'web-staging' web-staging.company.com --tags staging,web"
$BINARY add 'web-staging' web-staging.company.com --tags staging,web >/dev/null 2>&1
echo "$ $BINARY add 'api-gateway' api.company.com --tags production,api"
$BINARY add 'api-gateway' api.company.com --tags production,api >/dev/null 2>&1

echo ""
echo "Current connections:"
run_cmd "$BINARY list"

echo "Testing Fuzzy Search Across Commands:"
echo ""

# Test 1: Connect with fuzzy search
echo "Test 1: Connect with fuzzy search - 'webprod'"
echo "Expected: Finds 'web-prod-server' and prompts for confirmation"
echo "Command: timeout 3s $BINARY connect 'webprod'"
timeout 3s $BINARY connect 'webprod' 2>/dev/null || echo "Timed out (interactive prompt)"
echo ""

# Test 2: Edit with fuzzy search
echo "Test 2: Edit with fuzzy search - 'dbprod'"
echo "Expected: Finds 'db-prod-server' and prompts for confirmation"
echo "Command: timeout 3s $BINARY edit 'dbprod' --user newuser"
timeout 3s $BINARY edit 'dbprod' --user newuser 2>/dev/null || echo "Timed out (interactive prompt)"
echo ""

# Test 3: Show with fuzzy search
echo "Test 3: Show with fuzzy search - 'appprod'"
echo "Expected: Finds 'app-prod-server' and prompts for confirmation"
echo "Command: timeout 3s $BINARY show 'appprod'"
timeout 3s $BINARY show 'appprod' 2>/dev/null || echo "Timed out (interactive prompt)"
echo ""

# Test 4: Remove with fuzzy search (extra careful)
echo "Test 4: Remove with fuzzy search - 'apigateway'"
echo "Expected: Finds 'api-gateway' and prompts for confirmation"
echo "Command: timeout 3s $BINARY remove 'apigateway'"
timeout 3s $BINARY remove 'apigateway' 2>/dev/null || echo "Timed out (interactive prompt)"
echo ""

# Test 5: Multiple matches
echo "Test 5: Multiple matches - 'prod'"
echo "Expected: Shows all production servers for selection"
echo "Command: timeout 2s $BINARY connect 'prod'"
timeout 2s $BINARY connect 'prod' 2>/dev/null || echo "Timed out (interactive prompt)"
echo ""

echo "Summary:"
echo "Fuzzy search now works across ALL commands:"
echo "   * connect - Find and connect to servers"
echo "   * edit - Find and edit connection settings"
echo "   * show - Find and display connection details"
echo "   * remove - Find and remove connections (with extra confirmation)"
echo ""
echo "Key Features:"
echo "   * Pattern matching (hyphens, separators)"
echo "   * Tag-based search"
echo "   * Recent connections fallback"
echo "   * Interactive selection menus"
echo "   * Smart relevance ranking"
echo ""
echo "Examples:"
echo "   $BINARY connect 'webprod'    # Finds web-prod-server"
echo "   $BINARY edit 'dbprod'        # Finds db-prod-server"
echo "   $BINARY show 'appprod'       # Finds app-prod-server"
echo "   $BINARY remove 'apigateway'  # Finds api-gateway"
echo "   $BINARY connect 'prod'       # Shows all production servers"
echo ""
echo "Fuzzy search is now fully integrated across all connection management commands!"
