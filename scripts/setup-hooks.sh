#!/bin/sh
# Install git hooks for HumanSSH development

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
HOOKS_DIR="$REPO_ROOT/.git/hooks"

echo "Installing git hooks..."

# Install pre-commit hook
cp "$SCRIPT_DIR/pre-commit" "$HOOKS_DIR/pre-commit"
chmod +x "$HOOKS_DIR/pre-commit"

echo "✅ Hooks installed!"
echo ""
echo "The following hooks are now active:"
echo "  • pre-commit: runs cargo fmt --check, clippy, and cargo check"
