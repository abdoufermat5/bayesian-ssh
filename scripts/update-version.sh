#!/bin/bash

# Update version script for Bayesian SSH
# Usage: ./scripts/update-version.sh [patch|minor|major]

set -e

if [ $# -eq 0 ]; then
    echo "Usage: $0 [patch|minor|major]"
    echo "Example: $0 patch"
    exit 1
fi

VERSION_TYPE=$1
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)

echo "Current version: $CURRENT_VERSION"

# Parse current version
IFS='.' read -ra VERSION_PARTS <<< "$CURRENT_VERSION"
MAJOR=${VERSION_PARTS[0]}
MINOR=${VERSION_PARTS[1]}
PATCH=${VERSION_PARTS[2]}

case $VERSION_TYPE in
    "patch")
        NEW_PATCH=$((PATCH + 1))
        NEW_VERSION="$MAJOR.$MINOR.$NEW_PATCH"
        ;;
    "minor")
        NEW_MINOR=$((MINOR + 1))
        NEW_VERSION="$MAJOR.$NEW_MINOR.0"
        ;;
    "major")
        NEW_MAJOR=$((MAJOR + 1))
        NEW_VERSION="$NEW_MAJOR.0.0"
        ;;
    *)
        echo "Invalid version type: $VERSION_TYPE"
        echo "Use: patch, minor, or major"
        exit 1
        ;;
esac

echo "New version: $NEW_VERSION"

# Update Cargo.toml
sed -i "s/version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml

# Update README badge
sed -i "s/Version-$CURRENT_VERSION/Version-$NEW_VERSION/g" README.md

# Update docs if they reference version
find docs/ -name "*.md" -exec sed -i "s/$CURRENT_VERSION/$NEW_VERSION/g" {} \;

echo "✅ Version updated to $NEW_VERSION in:"
echo "  - Cargo.toml"
echo "  - README.md"
echo "  - docs/ files"

echo ""
echo "Next steps:"
echo "1. Review changes: git diff"
echo "2. Commit: git commit -am 'chore: bump version to $NEW_VERSION'"
echo "3. Tag: git tag -a v$NEW_VERSION -m 'Release v$NEW_VERSION'"
echo "4. Push: git push origin main && git push origin v$NEW_VERSION"
