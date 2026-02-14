#!/usr/bin/env bash

# Verify cross-built artifacts under target/releases

set -euo pipefail

RELEASE_DIR="target/releases"
FAILED_COUNT=0
TOTAL_COUNT=0

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error(){ echo -e "${RED}[ERROR]${NC} $1"; }
log_section(){ echo -e "${BLUE}[SECTION]${NC} $1"; }

run_with_timeout() {
  # run_with_timeout <seconds> <cmd>...
  local secs=$1; shift
  if command -v timeout >/dev/null 2>&1; then
    timeout "${secs}" "$@"
  else
    "$@"
  fi
}

verify_binary() {
  local binary_path=$1
  local target=$2

  echo "  Checking: $(basename "${binary_path}")"

  if [[ ! -f "${binary_path}" ]]; then
    log_error "  File not found"
    return 1
  fi

  # File size (Linux/macOS)
  local size
  size=$(stat -c%s "${binary_path}" 2>/dev/null || stat -f%z "${binary_path}" 2>/dev/null || echo "0")
  if [[ "${size}" -lt 1000000 ]]; then
    log_warn "  Small file size: ${size} bytes"
  else
    echo "  Size: ${size} bytes"
  fi

  # Executable bit for non-Windows
  if [[ "${target}" != *windows* ]]; then
    if [[ -x "${binary_path}" ]]; then
      echo "  Executable bit present"
    else
      log_warn "  Missing +x; fixing"
      chmod +x "${binary_path}"
    fi
  fi

  # Try --help on current platform targets
  local current_arch current_os can_run=false
  current_arch=$(uname -m)
  current_os=$(uname -s | tr '[:upper:]' '[:lower:]')
  case "${target}" in
    *linux*x86_64*)  [[ ${current_os} == linux  && ${current_arch} == x86_64 ]] && can_run=true ;;
    *linux*aarch64*) [[ ${current_os} == linux  && ${current_arch} == aarch64 ]] && can_run=true ;;
    *darwin*x86_64*) [[ ${current_os} == darwin && ${current_arch} == x86_64 ]] && can_run=true ;;
    *darwin*aarch64*)[[ ${current_os} == darwin && ${current_arch} == arm64  ]] && can_run=true ;;
  esac

  if ${can_run}; then
    echo "  Smoke test --help..."
    if run_with_timeout 10 "${binary_path}" --help >/dev/null 2>&1; then
      echo "  Smoke test passed"
    else
      log_error "  Smoke test failed"
      return 1
    fi
  else
    echo "  Skip run (different platform)"
  fi

  return 0
}

main() {
  log_section "Verify cross-built artifacts"
  if [[ ! -d "${RELEASE_DIR}" ]]; then
    log_error "Release dir not found: ${RELEASE_DIR}"
    log_info "Run: ./scripts/build-cross.sh"
    exit 1
  fi

  log_info "Release dir: ${RELEASE_DIR}"
  while IFS= read -r -d '' binary; do
    if [[ $(basename "${binary}") == filter-repo-rs* ]]; then
      ((TOTAL_COUNT++))
      local filename target
      filename=$(basename "${binary}")
      target=${filename#filter-repo-rs-}
      target=${target%.exe}
      echo
      log_section "Target: ${target}"
      if verify_binary "${binary}" "${target}"; then
        log_info "${target} OK"
      else
        log_error "${target} FAILED"
        ((FAILED_COUNT++))
      fi
    fi
  done < <(find "${RELEASE_DIR}" -maxdepth 1 -type f -print0)

  echo
  log_section "Summary"
  local success_count=$((TOTAL_COUNT - FAILED_COUNT))
  echo "Total: ${TOTAL_COUNT}"
  echo "Success: ${success_count}"
  echo "Failed: ${FAILED_COUNT}"
  if [[ ${FAILED_COUNT} -eq 0 ]]; then
    log_info "All binaries verified"
    echo
    log_info "Available binaries:"
    ls -la "${RELEASE_DIR}"/filter-repo-rs-* || true
    exit 0
  else
    log_error "${FAILED_COUNT} binaries failed"
    exit 1
  fi
}

if [[ "${1-}" == "--help" || "${1-}" == "-h" ]]; then
  echo "Verify build artifacts"
  echo
  echo "Usage: $0"
  echo "- check existence and size"
  echo "- ensure executable bit (non-Windows)"
  echo "- run --help smoke test when possible"
  echo
  echo "Run build first: ./scripts/build-cross.sh"
  exit 0
fi

main "$@"

