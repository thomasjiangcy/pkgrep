#!/bin/sh
set -eu

usage() {
  cat <<'USAGE'
Install the pkgrep agent skill into a skill directory.

Usage:
  scripts/install-skill.sh [--mode project|global] [--target <skills-dir>] [--force]

Options:
  --mode    Install mode. Defaults to project.
            project -> <cwd>/.agents/skills
            global  -> $HOME/.agents/skills
  --target  Explicit skills directory. Overrides --mode destination root.
  --force   Replace existing installed skill directory.
  -h, --help  Show this help text.
USAGE
}

MODE="project"
TARGET_DIR=""
FORCE="0"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode)
      shift
      [ "$#" -gt 0 ] || { echo "error: --mode requires a value" >&2; exit 1; }
      MODE="$1"
      ;;
    --target)
      shift
      [ "$#" -gt 0 ] || { echo "error: --target requires a value" >&2; exit 1; }
      TARGET_DIR="$1"
      ;;
    --force)
      FORCE="1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
SOURCE_DIR="$REPO_ROOT/skills/pkgrep-usage"

if [ ! -d "$SOURCE_DIR" ]; then
  echo "error: skill source directory not found: $SOURCE_DIR" >&2
  exit 1
fi

if [ -z "$TARGET_DIR" ]; then
  case "$MODE" in
    project)
      TARGET_DIR="$(pwd)/.agents/skills"
      ;;
    global)
      TARGET_DIR="$HOME/.agents/skills"
      ;;
    *)
      echo "error: --mode must be 'project' or 'global'" >&2
      exit 1
      ;;
  esac
fi

DEST_DIR="$TARGET_DIR/pkgrep-usage"

mkdir -p "$TARGET_DIR"

if [ -e "$DEST_DIR" ]; then
  if [ "$FORCE" = "1" ]; then
    rm -rf "$DEST_DIR"
  else
    echo "error: destination already exists: $DEST_DIR" >&2
    echo "hint: rerun with --force to replace the existing installation" >&2
    exit 1
  fi
fi

cp -R "$SOURCE_DIR" "$DEST_DIR"

echo "installed skill: $DEST_DIR"
echo "restart your agent runtime to load new skills"
