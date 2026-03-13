#!/bin/sh
# Automatic semantic versioning based on conventional commits.
#
# Usage:
#   scripts/release.sh [--dry-run] [--no-tag] [--no-commit]
#
# Commit prefixes and their version bumps:
#   - feat!:, fix!:, BREAKING CHANGE: → major
#   - feat: → minor
#   - fix:, refactor:, test:, style:, perf:, ci:, build:, revert: → patch
#   - docs:, chore: → no release (internal changes)

set -e

# ============================================================================
# Parse arguments
# ============================================================================

DRY_RUN=false
NO_TAG=false
NO_COMMIT=false

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=true ;;
    --no-tag) NO_TAG=true ;;
    --no-commit) NO_COMMIT=true ;;
  esac
done

# ============================================================================
# Version files to keep in sync
# ============================================================================

VERSIONED_FILES="Cargo.toml .claude-plugin/plugin.json"

# ============================================================================
# Git helpers
# ============================================================================

get_last_tag() {
  git describe --tags --abbrev=0 2>/dev/null || echo ""
}

get_current_version() {
  grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'
}

# ============================================================================
# Commit analysis
# ============================================================================

analyze_commits() {
  LAST_TAG="$1"

  if [ -n "$LAST_TAG" ]; then
    RANGE="${LAST_TAG}..HEAD"
  else
    RANGE="HEAD"
  fi

  COMMITS=$(git log "$RANGE" --format="%H|%s" 2>/dev/null || echo "")

  if [ -z "$COMMITS" ]; then
    echo "none|0"
    return
  fi

  HAS_BREAKING=false
  HAS_FEAT=false
  HAS_PATCH_TRIGGER=false
  COUNT=0

  echo "$COMMITS" | while IFS='|' read -r hash msg; do
    COUNT=$((COUNT + 1))

    # Check for breaking changes
    case "$msg" in
      *"BREAKING CHANGE"*|*"BREAKING-CHANGE"*) HAS_BREAKING=true ;;
    esac
    # Check for type!: pattern
    if echo "$msg" | grep -qE '^[a-z]+!:'; then
      HAS_BREAKING=true
    fi
    # Check for feat:
    if echo "$msg" | grep -qE '^feat(\(.+\))?:'; then
      HAS_FEAT=true
    fi
    # Check for patch-triggering types
    if echo "$msg" | grep -qE '^(fix|refactor|test|style|perf|ci|build|revert)(\(.+\))?:'; then
      HAS_PATCH_TRIGGER=true
    fi

    # Determine bump type (output on last iteration via subshell)
    echo "$HAS_BREAKING|$HAS_FEAT|$HAS_PATCH_TRIGGER|$COUNT"
  done | tail -1
}

# ============================================================================
# Version bumping
# ============================================================================

bump_version() {
  CURRENT="$1"
  BUMP_TYPE="$2"

  MAJOR=$(echo "$CURRENT" | cut -d. -f1)
  MINOR=$(echo "$CURRENT" | cut -d. -f2)
  PATCH=$(echo "$CURRENT" | cut -d. -f3)

  case "$BUMP_TYPE" in
    major) echo "$((MAJOR + 1)).0.0" ;;
    minor) echo "$MAJOR.$((MINOR + 1)).0" ;;
    patch) echo "$MAJOR.$MINOR.$((PATCH + 1))" ;;
    *) echo "$CURRENT" ;;
  esac
}

update_cargo_toml() {
  NEW_VERSION="$1"
  # Update the first version = "..." line in Cargo.toml
  sed -i.bak "s/^version = \".*\"/version = \"$NEW_VERSION\"/" Cargo.toml
  rm -f Cargo.toml.bak
}

update_plugin_json() {
  NEW_VERSION="$1"
  PLUGIN_FILE=".claude-plugin/plugin.json"
  if [ -f "$PLUGIN_FILE" ]; then
    # Update version field in plugin.json
    sed -i.bak "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" "$PLUGIN_FILE"
    rm -f "${PLUGIN_FILE}.bak"
  fi
}

# ============================================================================
# Main
# ============================================================================

echo "[release] Analyzing commits..."
echo ""

CURRENT_VERSION=$(get_current_version)
LAST_TAG=$(get_last_tag)

# Get commits since last tag
if [ -n "$LAST_TAG" ]; then
  RANGE="${LAST_TAG}..HEAD"
else
  RANGE="HEAD"
fi

COMMITS=$(git log "$RANGE" --format="%s" 2>/dev/null || echo "")

if [ -z "$COMMITS" ]; then
  echo "[release] No commits since last release. Nothing to do."
  exit 0
fi

# Analyze commits
HAS_BREAKING=false
HAS_FEAT=false
HAS_PATCH_TRIGGER=false
COMMIT_COUNT=0

echo "$COMMITS" | while IFS= read -r msg; do
  [ -z "$msg" ] && continue
  COMMIT_COUNT=$((COMMIT_COUNT + 1))
done

# Re-analyze in main shell (avoid subshell variable scope issues)
for msg in $(git log "$RANGE" --format="%s" 2>/dev/null); do
  : # counted above
done

# Direct analysis without subshell
RESULT_BREAKING=false
RESULT_FEAT=false
RESULT_PATCH=false
RESULT_COUNT=0

while IFS= read -r msg; do
  [ -z "$msg" ] && continue
  RESULT_COUNT=$((RESULT_COUNT + 1))

  case "$msg" in
    *"BREAKING CHANGE"*|*"BREAKING-CHANGE"*) RESULT_BREAKING=true ;;
  esac
  if echo "$msg" | grep -qE '^[a-z]+!:'; then
    RESULT_BREAKING=true
  fi
  if echo "$msg" | grep -qE '^feat(\(.+\))?:'; then
    RESULT_FEAT=true
  fi
  if echo "$msg" | grep -qE '^(fix|refactor|test|style|perf|ci|build|revert)(\(.+\))?:'; then
    RESULT_PATCH=true
  fi
done <<EOF
$COMMITS
EOF

# Determine bump type
if $RESULT_BREAKING; then
  BUMP_TYPE="major"
elif $RESULT_FEAT; then
  BUMP_TYPE="minor"
elif $RESULT_PATCH; then
  BUMP_TYPE="patch"
else
  echo "[release] No version-bumping commits found. Nothing to do."
  exit 0
fi

NEW_VERSION=$(bump_version "$CURRENT_VERSION" "$BUMP_TYPE")

# Display summary
echo "[release] Last tag: ${LAST_TAG:-(none)}"
echo "[release] Commits analyzed: $RESULT_COUNT"
echo ""
echo "[release] Bump type: $BUMP_TYPE"
echo "[release] Version: $CURRENT_VERSION → $NEW_VERSION"
echo ""

if $DRY_RUN; then
  echo "[release] Dry run - no changes made."
  exit 0
fi

# Update version files
echo "[release] Updating Cargo.toml..."
update_cargo_toml "$NEW_VERSION"
echo "[release] Updating plugin version files..."
update_plugin_json "$NEW_VERSION"

# Create commit
if ! $NO_COMMIT; then
  echo "[release] Creating release commit..."
  git add $VERSIONED_FILES
  git commit -m "chore(release): v${NEW_VERSION}"
fi

# Create tag
if ! $NO_TAG && ! $NO_COMMIT; then
  echo "[release] Creating git tag..."
  git tag -a "v${NEW_VERSION}" -m "Release v${NEW_VERSION}"
fi

echo ""
echo "[release] Released v${NEW_VERSION}"

if ! $NO_TAG && ! $NO_COMMIT; then
  echo "[release] Run 'git push --follow-tags' to publish."
fi
