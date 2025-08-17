#!/bin/bash

# Build and Release script for Bayesian SSH
# Usage: ./scripts/build_and_push.sh [--release] [--create-release] [--version VERSION]

set -e

RELEASE_BUILD=false
CREATE_RELEASE=false
VERSION=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --release)
            RELEASE_BUILD=true
            shift
            ;;
        --create-release)
            CREATE_RELEASE=true
            shift
            ;;
        --version)
            VERSION="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--release] [--create-release] [--version VERSION]"
            exit 1
            ;;
    esac
done

echo "🚀 Bayesian SSH - Build and Release Script"
echo "==========================================="

# Check if we're in a git repository
if ! git rev-parse --git-dir > /dev/null 2>&1; then
    echo "❌ Error: Not in a git repository"
    exit 1
fi

# Get current branch
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CREATE_RELEASE" = true ] && [ "$CURRENT_BRANCH" != "main" ]; then
    echo "⚠️  Warning: Creating release from non-main branch: $CURRENT_BRANCH"
    read -p "Continue anyway? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "❌ Release creation cancelled"
        exit 1
    fi
fi

# Version management
if [ -n "$VERSION" ]; then
    echo "📋 Updating version to: $VERSION"
    
    # Validate version format
    if [[ ! $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "❌ Invalid version format: $VERSION (expected: X.Y.Z)"
        exit 1
    fi
    
    # Update Cargo.toml
    CURRENT_VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)
    echo "🔄 Updating version from $CURRENT_VERSION to $VERSION in Cargo.toml"
    sed -i "s/version = \"$CURRENT_VERSION\"/version = \"$VERSION\"/" Cargo.toml
    
    # Update README.md if it has version references
    if grep -q "Version-$CURRENT_VERSION" README.md; then
        echo "🔄 Updating version in README.md"
        sed -i "s/Version-$CURRENT_VERSION/Version-$VERSION/g" README.md
    fi
    
    # Update docs files
    echo "🔄 Updating version in documentation files"
    find docs/ -name "*.md" -exec sed -i "s/$CURRENT_VERSION/$VERSION/g" {} \; 2>/dev/null || true
    
    echo "✅ Version updated to $VERSION in all files"
fi

# Build the project
echo "📦 Building project..."
if [ "$RELEASE_BUILD" = true ]; then
    echo "🔨 Building release version..."
    cargo build --release
    echo "✅ Release build completed"
else
    echo "🔨 Building debug version..."
    cargo build
    echo "✅ Debug build completed"
fi

# Run tests
echo "🧪 Running tests..."
cargo test
echo "✅ Tests passed"

# Check formatting
echo "🎨 Checking code formatting..."
cargo fmt --all -- --check
echo "✅ Code formatting is correct"

# Show build info
if [ "$RELEASE_BUILD" = true ]; then
    echo ""
    echo "📊 Build Information:"
    echo "  - Binary location: target/release/bayesian-ssh"
    echo "  - Binary size: $(ls -lh target/release/bayesian-ssh | awk '{print $5}')"
    echo "  - Build time: $(date)"
fi

# Create release if requested
if [ "$CREATE_RELEASE" = true ]; then
    echo ""
    echo "🏷️  Creating release..."
    
    # Check working directory status
    if [ -n "$(git status --porcelain)" ]; then
        echo "⚠️  Working directory has uncommitted changes:"
        git status --short
        echo ""
        read -p "Do you want to commit these changes before release? (y/N): " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            read -p "Enter commit message: " commit_msg
            if [ -z "$commit_msg" ]; then
                commit_msg="chore: prepare for release"
            fi
            git add .
            git commit -m "$commit_msg"
            echo "✅ Changes committed"
        else
            echo "❌ Please commit or stash changes before release"
            exit 1
        fi
    fi
    
    # Get version from Cargo.toml
    VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)
    echo "📋 Using version from Cargo.toml: $VERSION"
    
    # Check if tag already exists
    if git tag -l | grep -q "^v$VERSION$"; then
        echo "❌ Tag v$VERSION already exists"
        exit 1
    fi
    
    # Create tag
    echo "🏷️  Creating tag v$VERSION..."
    git tag -a "v$VERSION" -m "Release v$VERSION"
    echo "✅ Tag created locally"
    
    # Push tag to remote
    echo "📤 Pushing tag to remote..."
    git push origin "v$VERSION"
    echo "✅ Tag pushed to remote"
    
    echo ""
    echo "🎉 Release v$VERSION created successfully!"
    echo "📋 Next: Create GitHub release manually with tag v$VERSION"
fi

echo ""
echo "🎉 Build and release process completed!"
echo ""
echo "Next steps:"
if [ "$RELEASE_BUILD" = true ]; then
    echo "1. Test the release binary: ./target/release/bayesian-ssh --help"
    echo "2. Install globally: sudo cp target/release/bayesian-ssh /usr/local/bin/"
fi
if [ "$CREATE_RELEASE" = true ]; then
    echo "3. Create GitHub release with tag v$VERSION"
    echo "4. Upload binary from target/release/bayesian-ssh"
fi
if [ "$CREATE_RELEASE" = false ]; then
    echo "1. To create release: $0 --create-release"
fi
echo "2. To build release: $0 --release"
echo "3. To update version: $0 --version X.Y.Z"
