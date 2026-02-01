#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
E2E_SCRIPT="${SCRIPT_DIR}/e2e.sh"

source "${SCRIPT_DIR}/common.sh"

STRATEGIES=("first-fit" "best-fit" "least-allocated" "balanced")
MULTI_NODE_FLAG=""
RESULTS=()

SELECTED_STRATEGIES=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --multi-node)
            MULTI_NODE_FLAG="--multi-node"
            shift
            ;;
        --strategy)
            SELECTED_STRATEGIES+=("$2")
            shift 2
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ ${#SELECTED_STRATEGIES[@]} -gt 0 ]]; then
    STRATEGIES=("${SELECTED_STRATEGIES[@]}")
fi

if [[ ! -x "${E2E_SCRIPT}" ]]; then
    log_error "E2E test script not found or not executable: ${E2E_SCRIPT}"
    exit 1
fi

log_section "Cleaning Up"
kill_processes

build_kago || exit 1

# Agent 1 (larger): 4000m CPU, 8192Mi memory
# Agent 2 (smaller): 2000m CPU, 4096Mi memory
export AGENT_1_CPU=4000
export AGENT_1_MEMORY=8192
export AGENT_2_CPU=2000
export AGENT_2_MEMORY=4096

log_info "Agent 1 resources: CPU=${AGENT_1_CPU}m, Memory=${AGENT_1_MEMORY}Mi"
log_info "Agent 2 resources: CPU=${AGENT_2_CPU}m, Memory=${AGENT_2_MEMORY}Mi"

echo ""
log_section "Running E2E Tests with All Scheduling Strategies"
echo "========================================================"
echo ""

TOTAL_PASSED=0
TOTAL_FAILED=0

for strategy in "${STRATEGIES[@]}"; do
    echo ""
    log_section "Testing with scheduler: ${strategy}"
    echo "----------------------------------------"

    if "${E2E_SCRIPT}" --scheduler "${strategy}" ${MULTI_NODE_FLAG}; then
        RESULTS+=("${GREEN}✓${NC} ${strategy}")
        ((TOTAL_PASSED += 1))
    else
        RESULTS+=("${RED}✗${NC} ${strategy}")
        ((TOTAL_FAILED += 1))
    fi

    sleep 2
done

echo ""
echo "========================================================"
log_section "Summary: All Scheduler Strategy Tests"
echo "========================================================"
echo ""

for result in "${RESULTS[@]}"; do
    echo -e "  ${result}"
done

echo ""
echo "----------------------------------------"
echo -e "Strategies Passed: ${GREEN}${TOTAL_PASSED}${NC}"
echo -e "Strategies Failed: ${RED}${TOTAL_FAILED}${NC}"
echo -e "Total Strategies:  ${#STRATEGIES[@]}"
echo ""

if [[ ${TOTAL_FAILED} -gt 0 ]]; then
    log_error "Some scheduler strategy tests failed!"
    exit 1
fi

log_info "All scheduler strategy tests passed!"
exit 0
