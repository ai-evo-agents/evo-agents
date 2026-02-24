#!/usr/bin/env bash
# publish.sh — Build, verify, and push evo-agents to GitHub.
#
# Handles the path→crates.io dependency swap for evo-agent-sdk so CI passes,
# then restores the local path dependency for continued development.
#
# Usage:
#   ./publish.sh                     # Push current changes to main
#   ./publish.sh --release v0.2.0    # Push + create release tag (triggers binary builds + crates.io publish)
#   ./publish.sh --dry-run           # Run checks only, no push
#   ./publish.sh --skip-common       # Skip evo-common push (already up to date)
#
# Prerequisites:
#   - Clean working tree (or changes staged for commit)
#   - evo-common already pushed to GitHub (or use without --skip-common)
#   - gh CLI authenticated
#   - CARGO_REGISTRY_TOKEN set in GitHub repo secrets (for release.yml publish)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"
RUNNER_CARGO="$REPO_ROOT/runner/Cargo.toml"
SDK_CARGO="$REPO_ROOT/evo-agent-sdk/Cargo.toml"
COMMON_DIR="$REPO_ROOT/../evo-common"
COMMON_GIT_URL="https://github.com/ai-evo-agents/evo-common.git"

# ── Parse arguments ──────────────────────────────────────────────────────────

DRY_RUN=false
RELEASE_TAG=""
SKIP_COMMON=false
COMMIT_MSG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)     DRY_RUN=true;   shift ;;
    --release)     RELEASE_TAG="$2"; shift 2 ;;
    --skip-common) SKIP_COMMON=true; shift ;;
    -m)            COMMIT_MSG="$2"; shift 2 ;;
    *)
      echo "Unknown option: $1" >&2
      echo "Usage: ./publish.sh [--dry-run] [--release VERSION] [--skip-common] [-m \"commit message\"]" >&2
      exit 1
      ;;
  esac
done

# ── Helpers ──────────────────────────────────────────────────────────────────

info()  { echo "  [INFO]  $*"; }
ok()    { echo "  [OK]    $*"; }
fail()  { echo "  [FAIL]  $*" >&2; exit 1; }
step()  { echo ""; echo "==> $*"; }

# ── Step 1: Ensure evo-common is pushed ──────────────────────────────────────

if [[ "$SKIP_COMMON" == false ]]; then
  step "Checking evo-common"

  if [[ ! -d "$COMMON_DIR/.git" ]]; then
    fail "evo-common not found at $COMMON_DIR"
  fi

  COMMON_STATUS=$(cd "$COMMON_DIR" && git status --porcelain)
  if [[ -n "$COMMON_STATUS" ]]; then
    info "evo-common has uncommitted changes — pushing first"

    if [[ "$DRY_RUN" == true ]]; then
      info "[dry-run] Would commit and push evo-common"
    else
      (
        cd "$COMMON_DIR"
        git add -A
        git commit -m "chore: sync evo-common before agents publish" || true
        git push origin main
      )
      ok "evo-common pushed"
    fi
  else
    # Check if local is ahead of remote
    COMMON_AHEAD=$(cd "$COMMON_DIR" && git rev-list --count origin/main..HEAD 2>/dev/null || echo "0")
    if [[ "$COMMON_AHEAD" -gt 0 ]]; then
      info "evo-common has $COMMON_AHEAD unpushed commit(s)"
      if [[ "$DRY_RUN" == true ]]; then
        info "[dry-run] Would push evo-common"
      else
        (cd "$COMMON_DIR" && git push origin main)
        ok "evo-common pushed"
      fi
    else
      ok "evo-common is up to date"
    fi
  fi
fi

# ── Step 2: Run local checks ────────────────────────────────────────────────

step "Running local checks (fmt + clippy + test)"

cd "$REPO_ROOT"

info "cargo fmt --check"
cargo fmt --check || fail "cargo fmt failed — run: cargo fmt"
ok "fmt"

info "cargo clippy"
cargo clippy --workspace -- -D warnings 2>&1 || fail "clippy failed"
ok "clippy"

info "cargo test"
cargo test --workspace 2>&1 || fail "tests failed"
ok "tests"

# ── Step 3: Swap path dependency → crates.io for CI ─────────────────────────

step "Preparing Cargo.toml for CI (path → crates.io)"

# Backup originals
cp "$RUNNER_CARGO" "$RUNNER_CARGO.bak"

# Runner uses evo-agent-sdk via path locally; swap to crates.io version for CI
if grep -q 'path = "../evo-agent-sdk"' "$RUNNER_CARGO"; then
  # Extract SDK version from SDK Cargo.toml
  SDK_VERSION=$(grep '^version' "$SDK_CARGO" | head -1 | sed 's/.*"\(.*\)".*/\1/')
  sed -i.sed "s|path = \"../evo-agent-sdk\"|version = \"$SDK_VERSION\"|" "$RUNNER_CARGO"
  rm -f "$RUNNER_CARGO.sed"
  ok "Runner: swapped evo-agent-sdk path → version $SDK_VERSION"
else
  info "Runner: already using crates.io dependency — no swap needed"
fi

# Regenerate Cargo.lock
info "Regenerating Cargo.lock"
cargo generate-lockfile 2>&1 || true

# ── Step 4: Commit and push ──────────────────────────────────────────────────

step "Committing and pushing"

if [[ -z "$COMMIT_MSG" ]]; then
  # Auto-generate from git status
  CHANGED_FILES=$(git diff --name-only HEAD 2>/dev/null | head -5 | tr '\n' ', ' | sed 's/,$//')
  COMMIT_MSG="chore: publish evo-agents (${CHANGED_FILES:-sync})"
fi

if [[ "$DRY_RUN" == true ]]; then
  info "[dry-run] Would commit: $COMMIT_MSG"
  info "[dry-run] Would push to origin/main"
else
  git add -A
  git commit -m "$COMMIT_MSG" || info "Nothing to commit"
  git push origin main
  ok "Pushed to origin/main"
fi

# ── Step 5: Restore local path dependency ────────────────────────────────────

step "Restoring local development dependency"

mv "$RUNNER_CARGO.bak" "$RUNNER_CARGO"

# Regenerate Cargo.lock with path dependency
cargo generate-lockfile 2>&1 || true

ok "Restored path dependency for local development"

# ── Step 6: Create release tag (optional) ────────────────────────────────────

if [[ -n "$RELEASE_TAG" ]]; then
  step "Creating release: $RELEASE_TAG"

  # Validate tag format
  if [[ ! "$RELEASE_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+ ]]; then
    fail "Release tag must match vX.Y.Z format (got: $RELEASE_TAG)"
  fi

  if [[ "$DRY_RUN" == true ]]; then
    info "[dry-run] Would create tag: $RELEASE_TAG"
    info "[dry-run] Would push tag to origin (triggers release.yml: publish SDK + build binaries)"
  else
    # Re-swap to crates.io dep for the tagged commit
    cp "$RUNNER_CARGO" "$RUNNER_CARGO.bak"

    SDK_VERSION=$(grep '^version' "$SDK_CARGO" | head -1 | sed 's/.*"\(.*\)".*/\1/')
    sed -i.sed "s|path = \"../evo-agent-sdk\"|version = \"$SDK_VERSION\"|" "$RUNNER_CARGO"
    rm -f "$RUNNER_CARGO.sed"

    # Update versions in both Cargo.toml files
    SEMVER="${RELEASE_TAG#v}"
    sed -i.sed "s/^version = \".*\"/version = \"$SEMVER\"/" "$RUNNER_CARGO"
    rm -f "$RUNNER_CARGO.sed"
    sed -i.sed "s/^version = \".*\"/version = \"$SEMVER\"/" "$SDK_CARGO"
    rm -f "$SDK_CARGO.sed"

    cargo generate-lockfile 2>&1 || true

    git add -A
    git commit -m "release: $RELEASE_TAG"
    git tag -a "$RELEASE_TAG" -m "Release $RELEASE_TAG"
    git push origin main --tags

    ok "Tag $RELEASE_TAG pushed — release.yml will publish SDK + build binaries"

    # Restore local dev
    mv "$RUNNER_CARGO.bak" "$RUNNER_CARGO"
    cargo generate-lockfile 2>&1 || true
    ok "Restored local development dependency"
  fi
fi

# ── Done ─────────────────────────────────────────────────────────────────────

step "Done"

if [[ "$DRY_RUN" == true ]]; then
  info "Dry run complete — no changes were pushed"
else
  ok "evo-agents published successfully"
  if [[ -n "$RELEASE_TAG" ]]; then
    ok "Release $RELEASE_TAG will be built by GitHub Actions"
    echo "  Track: https://github.com/ai-evo-agents/evo-agents/actions"
  fi
fi
