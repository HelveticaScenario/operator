#!/bin/bash

# Exit on error
set -e

# Check for uncommitted changes
if [[ -n $(git status --porcelain) ]]; then
    echo "Stashing uncommitted changes..."
    git stash -u
    STASHED=1
else
    STASHED=0
fi

# Function to restore stash on exit
cleanup() {
    if [[ "$STASHED" -eq 1 ]]; then
        echo "Restoring stashed changes..."
        git stash pop
    fi
}
# Set trap to ensure cleanup runs on exit (success or failure)
trap cleanup EXIT

# Bump version in root package.json
yarn version patch

# Get the new version
VERSION=$(node -p "require('./package.json').version")

# Commit changes
git add package.json
git commit -m "Release v$VERSION"

# Create tag
git tag "v$VERSION"

# Push changes and tag
git push origin HEAD
git push origin "v$VERSION"

echo "Successfully created and pushed tag v$VERSION. GitLab CI will now deploy the release."
