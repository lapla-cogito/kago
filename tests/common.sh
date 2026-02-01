#!/usr/bin/env bash

TEST_CONTAINER_PREFIX="kago_test_"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_test() {
    echo -e "${YELLOW}[TEST]${NC} $1"
}

log_section() {
    echo -e "${BLUE}[====]${NC} $1"
}

kill_processes() {
    log_info "Stopping any leftover kago processes..."
    pkill -9 -f "target/release/kago" 2>/dev/null || true
    pkill -9 -f "target/debug/kago" 2>/dev/null || true
    sleep 1
}

cleanup_containers() {
    docker ps -a --filter "name=${TEST_CONTAINER_PREFIX}" --format "{{.ID}}" | xargs -r docker rm -f 2>/dev/null || true
}

wait_for_port_free() {
    local port="$1"
    local max_wait="${2:-10}"
    local wait_count=0

    while [[ ${wait_count} -lt ${max_wait} ]]; do
        if ! lsof -i ":${port}" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
        ((wait_count++))
    done

    return 1
}


wait_for_ports_free() {
    local max_wait="$1"
    shift
    local ports=("$@")
    local wait_count=0

    while [[ ${wait_count} -lt ${max_wait} ]]; do
        local ports_in_use=0

        for port in "${ports[@]}"; do
            if lsof -i ":${port}" >/dev/null 2>&1; then
                ports_in_use=$((ports_in_use + 1))
            fi
        done

        if [[ ${ports_in_use} -eq 0 ]]; then
            log_info "All ports are free"
            return 0
        fi

        log_info "Waiting for ports to be released (${ports_in_use} still in use)..."
        sleep 1
        ((wait_count++))
    done

    log_warn "Some ports may still be in use after ${max_wait} seconds"
    return 1
}

check_required_commands() {
    local missing=()
    for cmd in "$@"; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            missing+=("$cmd")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Missing required commands: ${missing[*]}"
        return 1
    fi
    return 0
}

check_docker_running() {
    if ! docker info &> /dev/null; then
        log_error "Docker daemon is not running"
        return 1
    fi
    return 0
}

build_kago() {
    local project_dir="${1:-}"

    if [[ -z "${project_dir}" ]]; then
        if [[ -n "${SCRIPT_DIR:-}" ]]; then
            project_dir="${SCRIPT_DIR}/.."
        else
            log_error "build_kago: project_dir not specified and SCRIPT_DIR not set"
            return 1
        fi
    fi

    log_section "Building Kago"

    if [[ ! -f "${project_dir}/Cargo.toml" ]]; then
        log_error "Cargo.toml not found in ${project_dir}"
        return 1
    fi

    cd "${project_dir}"

    if [[ ! -f "./target/release/kago" ]]; then
        log_info "Building release binary..."
        cargo build --release || {
            log_error "Failed to build kago"
            return 1
        }
        log_info "Build complete"
    else
        log_info "Release binary already exists, skipping build"
    fi

    return 0
}
