#!/usr/bin/env bash
# Opt-in installer for this repo's AI (Claude Code) docs.
# Pulls in the AI-docs submodule and symlinks it into place.
# Skip this script entirely if you don't want any AI tooling in your checkout.
set -euo pipefail
cd "$(dirname "$0")"

CLONE_DIR=".agents"

git submodule update --init "$CLONE_DIR"

ln -sf "$CLONE_DIR/CLAUDE.md" CLAUDE.md
ln -sf "$CLONE_DIR/Docs" Docs
mkdir -p .claude
ln -sf "../$CLONE_DIR/.claude/agents" .claude/agents
ln -sf "../$CLONE_DIR/.claude/skills" .claude/skills

echo "AI docs installed."
