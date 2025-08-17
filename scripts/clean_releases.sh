#!/bin/bash

# Clean Releases and Tags Script for Bayesian SSH
# This script removes all tags and releases both locally and remotely
# Usage: ./scripts/clean_releases.sh [--force] [--remote-only] [--local-only] [--tag TAG_NAME]

set -e

FORCE=false
REMOTE_ONLY=false
LOCAL_ONLY=false
SPECIFIC_TAG=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --force)
            FORCE=true
            shift
            ;;
        --remote-only)
            REMOTE_ONLY=true
            shift
            ;;
        --local-only)
            LOCAL_ONLY=true
            shift
            ;;
        --tag)
            SPECIFIC_TAG="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--force] [--remote-only] [--local-only] [--tag TAG_NAME]"
            exit 1
            ;;
    esac
done

echo "🧹 Bayesian SSH - Clean Releases and Tags Script"
echo "================================================"

# Check if we're in a git repository
if ! git rev-parse --git-dir > /dev/null 2>&1; then
    echo "❌ Error: Not in a git repository"
    exit 1
fi

# Get current branch
CURRENT_BRANCH=$(git branch --show-current)
echo "📍 Current branch: $CURRENT_BRANCH"

# Handle specific tag deletion
if [ -n "$SPECIFIC_TAG" ]; then
    echo ""
    echo "🎯 Deleting specific tag: $SPECIFIC_TAG"
    
    # Check if tag exists locally
    if git tag -l | grep -q "^$SPECIFIC_TAG$"; then
        echo "🗑️  Deleting local tag: $SPECIFIC_TAG"
        git tag -d "$SPECIFIC_TAG"
        echo "✅ Local tag deleted"
    else
        echo "ℹ️  Tag $SPECIFIC_TAG not found locally"
    fi
    
    # Check if tag exists remotely
    if git ls-remote --tags origin | grep -q "refs/tags/$SPECIFIC_TAG$"; then
        echo "🗑️  Deleting remote tag: $SPECIFIC_TAG"
        git push origin --delete "$SPECIFIC_TAG"
        echo "✅ Remote tag deleted"
    else
        echo "ℹ️  Tag $SPECIFIC_TAG not found remotely"
    fi
    
    echo ""
    echo "🎉 Specific tag deletion completed!"
    exit 0
fi

# Safety check for bulk operations
if [ "$FORCE" = false ]; then
    echo ""
    echo "⚠️  WARNING: This script will delete ALL tags and releases!"
    echo "   - Local tags will be removed"
    echo "   - Remote tags will be removed"
    echo "   - GitHub releases will be deleted"
    echo ""
    read -p "Are you sure you want to continue? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "❌ Operation cancelled"
        exit 1
    fi
fi

# Clean local tags
if [ "$REMOTE_ONLY" = false ]; then
    echo ""
    echo "🗑️  Cleaning local tags..."
    
    # Get all local tags
    LOCAL_TAGS=$(git tag -l)
    if [ -z "$LOCAL_TAGS" ]; then
        echo "ℹ️  No local tags found"
    else
        echo "📋 Found local tags:"
        echo "$LOCAL_TAGS"
        echo ""
        
        # Delete local tags
        for tag in $LOCAL_TAGS; do
            echo "🗑️  Deleting local tag: $tag"
            git tag -d "$tag"
        done
        echo "✅ Local tags cleaned"
    fi
fi

# Clean remote tags
if [ "$LOCAL_ONLY" = false ]; then
    echo ""
    echo "🗑️  Cleaning remote tags..."
    
    # Get all remote tags, filter out the ^{} suffix tags
    REMOTE_TAGS=$(git ls-remote --tags origin | grep -o 'refs/tags/.*' | sed 's/refs\/tags\///' | grep -v '\^{}$')
    if [ -z "$REMOTE_TAGS" ]; then
        echo "ℹ️  No remote tags found"
    else
        echo "📋 Found remote tags:"
        echo "$REMOTE_TAGS"
        echo ""
        
        # Delete remote tags
        for tag in $REMOTE_TAGS; do
            echo "🗑️  Deleting remote tag: $tag"
            git push origin --delete "$tag"
        done
        echo "✅ Remote tags cleaned"
    fi
fi

# Clean local references
if [ "$REMOTE_ONLY" = false ]; then
    echo ""
    echo "🧹 Cleaning local references..."
    
    # Prune remote references
    git remote prune origin
    echo "✅ Remote references pruned"
    
    # Clean up any remaining tag references
    git gc --prune=now
    echo "✅ Garbage collection completed"
fi

echo ""
echo "🎉 Cleanup completed successfully!"
echo ""
echo "Summary:"
if [ "$REMOTE_ONLY" = false ]; then
    echo "✅ Local tags removed"
    echo "✅ Local references cleaned"
fi
if [ "$LOCAL_ONLY" = false ]; then
    echo "✅ Remote tags removed"
fi
echo ""
echo "Next steps:"
echo "1. Check status: git status"
echo "2. Verify tags: git tag -l"
echo "3. Check remote: git ls-remote --tags origin"
echo "4. Build and create release: ./scripts/build_and_push.sh --release --create-release"
echo ""
echo "Usage examples:"
echo "  # Delete specific tag: $0 --tag v1.2.3"
echo "  # Clean only remote: $0 --remote-only"
echo "  # Clean only local: $0 --local-only"
echo "  # Skip confirmation: $0 --force"
