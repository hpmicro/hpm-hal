#!/bin/bash
set -e

# I'm sorry that for convenience, I'm using the hpm-data repo to update the hpm-metapac tag, on my personal account. :P
REPO_URL="https://github.com/andelf/hpm-data.git"
CARGO_TOML="$(dirname "$0")/Cargo.toml"

# Get the commit hash of main branch
COMMIT_HASH=$(git ls-remote "$REPO_URL" refs/heads/main | cut -f1)

if [ -z "$COMMIT_HASH" ]; then
    echo "Error: Failed to get commit hash from $REPO_URL"
    exit 1
fi

NEW_TAG="hpm-data-$COMMIT_HASH"
echo "Latest tag: $NEW_TAG"

# Replace the tag in Cargo.toml
sed -i '' "s/tag = \"hpm-data-[a-f0-9]*\"/tag = \"$NEW_TAG\"/g" "$CARGO_TOML"

echo "Updated Cargo.toml with tag: $NEW_TAG"

