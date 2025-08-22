#!/usr/bin/env zsh

# Generate and source the completion
eval "$(./target/debug/xlaude completions zsh)"

# Test the completion function directly
echo "Testing Zsh completion grouping..."
echo

# Simulate completion context
CURRENT=3
words=(xlaude open "")

echo "Calling _xlaude_worktrees function:"
echo "=================================="

# Call the worktree completion function
_xlaude_worktrees

echo
echo "Raw grouped data from complete-worktrees:"
echo "=========================================="
./target/debug/xlaude complete-worktrees --format=grouped | head -10