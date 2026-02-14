#!/usr/bin/env bash

# Cross-platform build script (Linux/macOS)
# Targets: Linux/macOS/Windows, x86_64 and aarch64

set -euo pipefail

# Colored output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Log helpers
log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Dependencies
check_dependencies() {
  log_info "Checking build dependencies..."
  if ! command -v cargo >/dev/null 2>&1; then
    log_error "cargo not found; please install Rust toolchain"
    exit 1
  fi
  if ! command -v cross >/dev/null 2>&1; then
    log_warn "cross not found; installing (requires network)..."
    cargo install cross --git https://github.com/cross-rs/cross
  fi
}

# Supported targets
TARGETS=(
  # Linux
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl

  # macOS (needs native build or osxcross/SDK)
  x86_64-apple-darwin
  aarch64-apple-darwin

  # Windows
  x86_64-pc-windows-gnu
  aarch64-pc-windows-msvc
  x86_64-pc-windows-msvc
)

# Build one target
build_target() {
  local target=$1
  local output_dir="target/releases"
  log_info "Building target: ${target}"
  mkdir -p "${output_dir}"
  if cross build --target "${target}" --release -p filter-repo-rs; then
    log_info "${target} build succeeded"
    local binary_name="filter-repo-rs"
    local source_path="target/${target}/release/${binary_name}"
    # Add .exe suffix on Windows
    if [[ ${target} == *windows* ]]; then
      source_path+=".exe"
      binary_name+=".exe"
    fi
    if [[ -f "${source_path}" ]]; then
      local dest_name="${binary_name}-${target}"
      cp "${source_path}" "${output_dir}/${dest_name}"
      log_info "Copied binary to: ${output_dir}/${dest_name}"
    else
      log_warn "Build artifact not found: ${source_path}"
    fi
  else
    log_error "${target} build failed"
    return 1
  fi
}

main() {
  log_info "Start cross-platform builds"
  check_dependencies
  local targets
  if [[ $# -gt 0 ]]; then
    targets=("$@")
  else
    targets=("${TARGETS[@]}")
  fi
  local success_count=0
  local total_count=${#targets[@]}
  for target in "${targets[@]}"; do
    if build_target "${target}"; then
      ((success_count++))
    fi
    echo "----------------------------------------"
  done
  log_info "Builds finished: ${success_count}/${total_count} succeeded"
  if [[ -d "target/releases" ]]; then
    log_info "Artifacts:"
    ls -la target/releases/
  fi
}

if [[ "${1-}" == "--help" || "${1-}" == "-h" ]]; then
  echo "Usage: $0 [target1] [target2] ..."
  echo
  echo "Supported targets:"
  for target in "${TARGETS[@]}"; do
    echo "  - ${target}"
  done
  echo
  echo "Examples:"
  echo "  $0                                   # build all targets"
  echo "  $0 x86_64-unknown-linux-gnu         # build Linux x64"
  echo "  $0 x86_64-apple-darwin aarch64-apple-darwin  # build macOS"
  exit 0
fi

main "$@"

