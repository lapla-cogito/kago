#!/usr/bin/env bash

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

KAGO_BIN="${KAGO_BIN:-./target/release/kago}"
KAGO_PORT="${KAGO_PORT:-18080}"
KAGO_AGENT_PORT="${KAGO_AGENT_PORT:-18081}"
KAGO_SERVER="http://localhost:${KAGO_PORT}"
KAGO_PID=""
AGENT_PID=""

TESTS_PASSED=0
TESTS_FAILED=0

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

cleanup() {
    log_info "Cleaning up..."

    # Stop agent if running
    if [[ -n "${AGENT_PID}" ]] && kill -0 "${AGENT_PID}" 2>/dev/null; then
        log_info "Stopping kago agent (PID: ${AGENT_PID})..."
        kill "${AGENT_PID}" 2>/dev/null || true
        wait "${AGENT_PID}" 2>/dev/null || true
    fi

    # Stop kago server if running
    if [[ -n "${KAGO_PID}" ]] && kill -0 "${KAGO_PID}" 2>/dev/null; then
        log_info "Stopping kago server (PID: ${KAGO_PID})..."
        kill "${KAGO_PID}" 2>/dev/null || true
        wait "${KAGO_PID}" 2>/dev/null || true
    fi

    # Clean up any containers created by kago
    log_info "Cleaning up kago containers..."
    docker ps -a --filter "name=test-nginx" --format "{{.ID}}" | xargs -r docker rm -f 2>/dev/null || true
    docker ps -a --filter "name=yaml-test" --format "{{.ID}}" | xargs -r docker rm -f 2>/dev/null || true
    docker ps -a --filter "name=multi-web" --format "{{.ID}}" | xargs -r docker rm -f 2>/dev/null || true
    docker ps -a --filter "name=multi-api" --format "{{.ID}}" | xargs -r docker rm -f 2>/dev/null || true

    log_info "Cleanup complete"
}

trap cleanup EXIT

wait_for_server() {
    local max_attempts=30
    local attempt=1

    log_info "Waiting for kago server to be ready..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        if curl -s "${KAGO_SERVER}/health" > /dev/null 2>&1; then
            log_info "Server is ready!"
            return 0
        fi
        sleep 1
        ((attempt++))
    done

    log_error "Server failed to start within ${max_attempts} seconds"
    return 1
}

wait_for_agent() {
    local max_attempts=30
    local attempt=1

    log_info "Waiting for agent to register..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local nodes
        nodes=$(curl -s "${KAGO_SERVER}/nodes" 2>/dev/null || echo "[]")
        local node_count
        node_count=$(echo "${nodes}" | jq 'length' 2>/dev/null || echo "0")

        if [[ "${node_count}" -ge 1 ]]; then
            log_info "Agent registered! Node count: ${node_count}"
            return 0
        fi
        sleep 1
        ((attempt++))
    done

    log_error "Agent failed to register within ${max_attempts} seconds"
    return 1
}

assert_eq() {
    local expected="$1"
    local actual="$2"
    local message="${3:-}"

    if [[ "${expected}" == "${actual}" ]]; then
        return 0
    else
        log_error "Assertion failed: ${message}"
        log_error "  Expected: ${expected}"
        log_error "  Actual:   ${actual}"
        return 1
    fi
}

assert_contains() {
    local haystack="$1"
    local needle="$2"
    local message="${3:-}"

    if [[ "${haystack}" == *"${needle}"* ]]; then
        return 0
    else
        log_error "Assertion failed: ${message}"
        log_error "  Expected to contain: ${needle}"
        log_error "  Actual: ${haystack}"
        return 1
    fi
}

assert_json_field() {
    local json="$1"
    local field="$2"
    local expected="$3"
    local message="${4:-}"

    local actual
    actual=$(echo "${json}" | jq -r "${field}" 2>/dev/null || echo "PARSE_ERROR")

    if [[ "${actual}" == "${expected}" ]]; then
        return 0
    else
        log_error "Assertion failed: ${message}"
        log_error "  Field: ${field}"
        log_error "  Expected: ${expected}"
        log_error "  Actual:   ${actual}"
        return 1
    fi
}

run_test() {
    local test_name="$1"
    local test_func="$2"

    log_test "Running: ${test_name}"

    if ${test_func}; then
        log_info "PASSED: ${test_name}"
        ((TESTS_PASSED++))
        return 0
    else
        log_error "FAILED: ${test_name}"
        ((TESTS_FAILED++))
        return 1
    fi
}

test_health_endpoint() {
    local response
    response=$(curl -s "${KAGO_SERVER}/health")

    assert_contains "${response}" "healthy" "Health endpoint should return healthy status"
}

test_list_nodes() {
    local response
    local http_code

    http_code=$(curl -s -o /tmp/nodes_response.json -w "%{http_code}" \
        "${KAGO_SERVER}/nodes")

    response=$(cat /tmp/nodes_response.json)

    assert_eq "200" "${http_code}" "Should return 200 OK" || return 1

    local node_count
    node_count=$(echo "${response}" | jq 'length')

    if [[ "${node_count}" -lt 1 ]]; then
        log_error "Expected at least 1 node, got ${node_count}"
        return 1
    fi

    # Check node is ready
    local ready_count
    ready_count=$(echo "${response}" | jq '[.[] | select(.status == "ready")] | length')

    if [[ "${ready_count}" -lt 1 ]]; then
        log_error "Expected at least 1 ready node"
        return 1
    fi

    return 0
}

test_cli_get_nodes() {
    local output
    output=$("${KAGO_BIN}" get nodes --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago get nodes failed: ${output}"
        return 1
    }

    assert_contains "${output}" "e2e-worker" "Should list e2e-worker node" || return 1

    return 0
}

test_create_deployment_via_api() {
    local response
    local http_code

    # Create deployment
    http_code=$(curl -s -o /tmp/create_response.json -w "%{http_code}" \
        -X POST "${KAGO_SERVER}/deployments" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "test-nginx",
            "image": "nginx:alpine",
            "replicas": 1
        }')

    response=$(cat /tmp/create_response.json)

    assert_eq "201" "${http_code}" "Should return 201 Created" || return 1
    assert_json_field "${response}" ".name" "test-nginx" "Deployment name should match" || return 1
    assert_json_field "${response}" ".image" "nginx:alpine" "Image should match" || return 1
    assert_json_field "${response}" ".replicas" "1" "Replicas should be 1" || return 1

    return 0
}

test_get_deployment() {
    local response
    local http_code

    http_code=$(curl -s -o /tmp/get_response.json -w "%{http_code}" \
        "${KAGO_SERVER}/deployments/test-nginx")

    response=$(cat /tmp/get_response.json)

    assert_eq "200" "${http_code}" "Should return 200 OK" || return 1
    assert_json_field "${response}" ".name" "test-nginx" "Deployment name should match" || return 1

    return 0
}

test_list_deployments() {
    local response
    local http_code

    http_code=$(curl -s -o /tmp/list_response.json -w "%{http_code}" \
        "${KAGO_SERVER}/deployments")

    response=$(cat /tmp/list_response.json)

    assert_eq "200" "${http_code}" "Should return 200 OK" || return 1

    # Check that the response is an array containing our deployment
    local count
    count=$(echo "${response}" | jq 'length')

    if [[ "${count}" -lt 1 ]]; then
        log_error "Expected at least 1 deployment, got ${count}"
        return 1
    fi

    return 0
}

test_pods_created_and_scheduled() {
    local max_attempts=30
    local attempt=1

    log_info "Waiting for pods to be created and scheduled to node..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local pod_count
        pod_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "test-nginx")] | length')

        if [[ "${pod_count}" -ge 1 ]]; then
            # Check that pod is assigned to a node
            local assigned_count
            assigned_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "test-nginx" and .node_name != null)] | length')

            if [[ "${assigned_count}" -ge 1 ]]; then
                local node_name
                node_name=$(echo "${response}" | jq -r '[.[] | select(.deployment_name == "test-nginx")][0].node_name')
                log_info "Found ${pod_count} pod(s), assigned to node: ${node_name}"
                return 0
            fi
        fi

        sleep 2
        ((attempt++))
    done

    log_error "No pods created/scheduled within ${max_attempts} attempts"
    return 1
}

test_container_running() {
    local max_attempts=30
    local attempt=1

    log_info "Waiting for container to be running..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local running_count
        running_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "test-nginx" and .status == "running")] | length')

        if [[ "${running_count}" -ge 1 ]]; then
            log_info "Container is running"
            return 0
        fi

        local status
        status=$(echo "${response}" | jq -r '[.[] | select(.deployment_name == "test-nginx")][0].status // "no pod"')
        log_info "Current pod status: ${status} (attempt ${attempt}/${max_attempts})"

        sleep 2
        ((attempt++))
    done

    log_error "Container not running within ${max_attempts} attempts"
    return 1
}

test_scale_deployment() {
    local response
    local http_code

    # Scale up to 2 replicas
    http_code=$(curl -s -o /tmp/scale_response.json -w "%{http_code}" \
        -X PUT "${KAGO_SERVER}/deployments/test-nginx" \
        -H "Content-Type: application/json" \
        -d '{"replicas": 2}')

    assert_eq "200" "${http_code}" "Should return 200 OK for scale operation" || return 1

    # Wait for second pod to be created
    local max_attempts=30
    local attempt=1

    log_info "Waiting for scale-up to complete..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        response=$(curl -s "${KAGO_SERVER}/pods")

        local pod_count
        pod_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "test-nginx" and .status != "terminated" and .status != "failed")] | length')

        if [[ "${pod_count}" -ge 2 ]]; then
            log_info "Scale-up complete: ${pod_count} pods"
            return 0
        fi

        log_info "Current pod count: ${pod_count} (attempt ${attempt}/${max_attempts})"
        sleep 2
        ((attempt++))
    done

    log_error "Scale-up did not complete within ${max_attempts} attempts"
    return 1
}

test_apply_yaml_manifest() {
    local manifest_file="/tmp/test-deployment.yml"

    cat > "${manifest_file}" << 'EOF'
kind: Deployment
spec:
  name: yaml-test
  image: alpine:latest
  replicas: 1
EOF

    local output
    output=$("${KAGO_BIN}" apply -f "${manifest_file}" --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago apply failed: ${output}"
        return 1
    }

    log_info "Apply output: ${output}"

    local response
    response=$(curl -s "${KAGO_SERVER}/deployments/yaml-test")

    assert_json_field "${response}" ".name" "yaml-test" "YAML deployment should be created" || return 1

    return 0
}

test_cli_get_deployments() {
    local output
    output=$("${KAGO_BIN}" get deployments --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago get deployments failed: ${output}"
        return 1
    }

    assert_contains "${output}" "test-nginx" "Should list test-nginx deployment" || return 1

    return 0
}

test_cli_get_pods() {
    local output
    output=$("${KAGO_BIN}" get pods --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago get pods failed: ${output}"
        return 1
    }

    # Output should be valid JSON array
    echo "${output}" | jq '.' > /dev/null 2>&1 || {
        log_error "Output is not valid JSON: ${output}"
        return 1
    }

    return 0
}

test_delete_deployment() {
    local http_code

    # Delete via API
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        -X DELETE "${KAGO_SERVER}/deployments/test-nginx")

    assert_eq "200" "${http_code}" "Should return 200 OK for delete" || return 1

    # Verify deployment is gone
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        "${KAGO_SERVER}/deployments/test-nginx")

    assert_eq "404" "${http_code}" "Should return 404 after deletion" || return 1

    return 0
}

test_cli_delete_deployment() {
    local output
    output=$("${KAGO_BIN}" delete yaml-test --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago delete failed: ${output}"
        return 1
    }

    # Verify deployment is gone
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        "${KAGO_SERVER}/deployments/yaml-test")

    assert_eq "404" "${http_code}" "Should return 404 after CLI deletion" || return 1

    return 0
}

test_pods_terminated_after_delete() {
    local max_attempts=30
    local attempt=1

    log_info "Waiting for pods to be terminated..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local active_pods
        active_pods=$(echo "${response}" | jq '[.[] | select(.deployment_name == "test-nginx" and .status != "terminated")] | length')

        if [[ "${active_pods}" -eq 0 ]]; then
            log_info "All pods terminated"
            return 0
        fi

        log_info "Active pods remaining: ${active_pods} (attempt ${attempt}/${max_attempts})"
        sleep 2
        ((attempt++))
    done

    log_error "Pods not terminated within ${max_attempts} attempts"
    return 1
}

test_multi_deployment_manifest() {
    local manifest_file="/tmp/multi-deployment.yml"

    cat > "${manifest_file}" << 'EOF'
kind: Deployment
spec:
  name: multi-web
  image: nginx:alpine
  replicas: 1
---
kind: Deployment
spec:
  name: multi-api
  image: httpd:alpine
  replicas: 1
EOF

    local output
    output=$("${KAGO_BIN}" apply -f "${manifest_file}" --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago apply multi failed: ${output}"
        return 1
    }

    # Verify both deployments were created
    local response
    response=$(curl -s "${KAGO_SERVER}/deployments")

    local web_exists api_exists
    web_exists=$(echo "${response}" | jq '[.[] | select(.name == "multi-web")] | length')
    api_exists=$(echo "${response}" | jq '[.[] | select(.name == "multi-api")] | length')

    if [[ "${web_exists}" -ne 1 ]] || [[ "${api_exists}" -ne 1 ]]; then
        log_error "Expected both multi-web and multi-api deployments"
        return 1
    fi

    # Cleanup
    "${KAGO_BIN}" delete multi-web --server "${KAGO_SERVER}" > /dev/null 2>&1 || true
    "${KAGO_BIN}" delete multi-api --server "${KAGO_SERVER}" > /dev/null 2>&1 || true

    return 0
}

main() {
    log_info "Starting Kago E2E Tests (Multi-Node Mode)"
    log_info "================================"

    if [[ ! -x "${KAGO_BIN}" ]]; then
        log_error "Kago binary not found or not executable: ${KAGO_BIN}"
        log_error "Please build with: cargo build --release"
        exit 1
    fi

    for cmd in jq curl docker; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            log_error "$cmd is not installed"
            exit 1
        fi
    done

    if ! docker info &> /dev/null; then
        log_error "Docker daemon is not running"
        exit 1
    fi

    # Start master (control plane)
    log_info "Starting kago master on port ${KAGO_PORT}..."
    "${KAGO_BIN}" serve --port "${KAGO_PORT}" &
    KAGO_PID=$!

    if ! wait_for_server; then
        log_error "Failed to start kago server"
        exit 1
    fi

    # Start agent (worker node)
    log_info "Starting kago agent on port ${KAGO_AGENT_PORT}..."
    "${KAGO_BIN}" agent \
        --name "e2e-worker" \
        --master "${KAGO_SERVER}" \
        --port "${KAGO_AGENT_PORT}" \
        --address "localhost" \
        --cpu 4000 \
        --memory 8192 &
    AGENT_PID=$!

    if ! wait_for_agent; then
        log_error "Failed to register agent"
        exit 1
    fi

    log_info ""
    log_info "Running tests..."
    log_info "================================"

    run_test "Health endpoint" test_health_endpoint || true
    run_test "List nodes" test_list_nodes || true
    run_test "CLI get nodes" test_cli_get_nodes || true
    run_test "Create deployment via API" test_create_deployment_via_api || true
    run_test "Get deployment" test_get_deployment || true
    run_test "List deployments" test_list_deployments || true
    run_test "Pods created and scheduled" test_pods_created_and_scheduled || true
    run_test "Container running" test_container_running || true
    run_test "Scale deployment" test_scale_deployment || true
    run_test "Apply YAML manifest" test_apply_yaml_manifest || true
    run_test "CLI get deployments" test_cli_get_deployments || true
    run_test "CLI get pods" test_cli_get_pods || true
    run_test "Multi-deployment manifest" test_multi_deployment_manifest || true
    run_test "Delete deployment" test_delete_deployment || true
    run_test "Pods terminated after delete" test_pods_terminated_after_delete || true
    run_test "CLI delete deployment" test_cli_delete_deployment || true

    log_info ""
    log_info "================================"
    log_info "Test Results"
    log_info "================================"
    log_info "Passed: ${TESTS_PASSED}"
    log_info "Failed: ${TESTS_FAILED}"
    log_info "Total:  $((TESTS_PASSED + TESTS_FAILED))"

    if [[ ${TESTS_FAILED} -gt 0 ]]; then
        log_error "Some test(s) failed!"
        exit 1
    fi

    log_info "All tests passed!"
    exit 0
}

main "$@"
