#!/bin/sh
export PROJECT_ROOT="${ZED_WORKTREE_ROOT:-$(pwd)}"
exec /Users/rageltd/Projects/cartographer/target/release/cartographer "$@"
