#!/usr/bin/env bash
# PPA EQuart one-click build
# usage: ./build.sh [release|debug] [install] [trace]
#   trace: build with `--features dpi-trace`, which writes a per-second frame
#          report (fps, ui/render/present times, adapter) to
#          %LOCALAPPDATA%/f0lock/diagnostics on Windows
set -euo pipefail
cd "$(dirname "$0")"

PROFILE="--release"
OUTDIR="target/bundled"
DO_INSTALL=0
FEATURES=

for arg in "$@"; do
  case "$arg" in
    debug)   PROFILE=""; OUTDIR="target/debug/bundled" ;;
    release) ;;
    install) DO_INSTALL=1 ;;
    trace)   FEATURES="--features dpi-trace" ;;
    *) echo "unknown argument: $arg"; echo "usage: ./build.sh [release|debug] [install] [trace]"; exit 1 ;;
  esac
done

if ! command -v cargo >/dev/null 2>&1; then
  echo "[error] cargo not found in PATH. Install Rust first: https://rustup.rs" >&2
  exit 1
fi

echo "[1/2] cargo xtask bundle equart $PROFILE $FEATURES"
# shellcheck disable=SC2086 # both flags are deliberately split into words
cargo xtask bundle equart $PROFILE $FEATURES

echo
echo "[2/2] bundle output:"
ls -d "$OUTDIR"/equart.clap 2>/dev/null || true
ls -d "$OUTDIR"/equart.vst3 2>/dev/null || true

if [[ "$DO_INSTALL" == 1 ]]; then
  case "$(uname -s)" in
    Darwin) CLAP_DIR="$HOME/Library/Audio/Plug-Ins/CLAP"; VST3_DIR="$HOME/Library/Audio/Plug-Ins/VST3" ;;
    *)      CLAP_DIR="$HOME/.clap";                       VST3_DIR="$HOME/.vst3" ;;
  esac
  mkdir -p "$CLAP_DIR" "$VST3_DIR"
  [[ -d "$OUTDIR/equart.clap" || -f "$OUTDIR/equart.clap" ]] && cp -r "$OUTDIR/equart.clap" "$CLAP_DIR/"
  [[ -d "$OUTDIR/equart.vst3" ]] && cp -r "$OUTDIR/equart.vst3" "$VST3_DIR/"
  echo "installed to $CLAP_DIR and $VST3_DIR"
fi

echo "=== done ==="
