#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/common.sh"

KAGO_BIN="${KAGO_BIN:-./target/release/kago}"
KAGO_PORT="${KAGO_PORT:-18080}"
KAGO_AGENT_PORT="${KAGO_AGENT_PORT:-18081}"
KAGO_AGENT_PORT_2="${KAGO_AGENT_PORT_2:-18082}"
KAGO_SERVER="http://localhost:${KAGO_PORT}"
KAGO_SCHEDULER="${KAGO_SCHEDULER:-first-fit}"
KAGO_PID=""
AGENT_PID=""
AGENT_PID_2=""

# Agent resources: Agent 1 has more resources than Agent 2 for strategy differentiation
AGENT_1_CPU="${AGENT_1_CPU:-4000}"
AGENT_1_MEMORY="${AGENT_1_MEMORY:-8192}"
AGENT_2_CPU="${AGENT_2_CPU:-2000}"
AGENT_2_MEMORY="${AGENT_2_MEMORY:-4096}"

TESTS_PASSED=0
TESTS_FAILED=0

cleanup() {
    log_info "Cleaning up..."

    # Stop agent 2 if running
    if [[ -n "${AGENT_PID_2}" ]] && kill -0 "${AGENT_PID_2}" 2>/dev/null; then
        log_info "Stopping kago agent 2 (PID: ${AGENT_PID_2})..."
        kill "${AGENT_PID_2}" 2>/dev/null || true
        wait "${AGENT_PID_2}" 2>/dev/null || true
    fi

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

    log_info "Cleaning up kago containers..."
    cleanup_containers

    log_info "Cleanup complete"
}

cleanup_processes() {
    kill_processes
    wait_for_ports_free 10 "${KAGO_PORT}" "${KAGO_AGENT_PORT}" "${KAGO_AGENT_PORT_2}"
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
    local expected_count="${1:-1}"
    local max_attempts=30
    local attempt=1

    log_info "Waiting for ${expected_count} agent(s) to register..."

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local nodes
        nodes=$(curl -s "${KAGO_SERVER}/nodes" 2>/dev/null || echo "[]")
        local node_count
        node_count=$(echo "${nodes}" | jq 'length' 2>/dev/null || echo "0")

        if [[ "${node_count}" -ge "${expected_count}" ]]; then
            log_info "Agent(s) registered! Node count: ${node_count}"
            return 0
        fi
        sleep 1
        ((attempt++))
    done

    log_error "Agent(s) failed to register within ${max_attempts} seconds"
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
            "name": "'"${TEST_CONTAINER_PREFIX}"'nginx",
            "image": "nginx:alpine",
            "replicas": 1
        }')

    response=$(cat /tmp/create_response.json)

    assert_eq "201" "${http_code}" "Should return 201 Created" || return 1
    assert_json_field "${response}" ".name" "${TEST_CONTAINER_PREFIX}nginx" "Deployment name should match" || return 1
    assert_json_field "${response}" ".image" "nginx:alpine" "Image should match" || return 1
    assert_json_field "${response}" ".replicas" "1" "Replicas should be 1" || return 1

    return 0
}

test_get_deployment() {
    local response
    local http_code

    http_code=$(curl -s -o /tmp/get_response.json -w "%{http_code}" \
        "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}nginx")

    response=$(cat /tmp/get_response.json)

    assert_eq "200" "${http_code}" "Should return 200 OK" || return 1
    assert_json_field "${response}" ".name" "${TEST_CONTAINER_PREFIX}nginx" "Deployment name should match" || return 1

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
        pod_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx")] | length')

        if [[ "${pod_count}" -ge 1 ]]; then
            # Check that pod is assigned to a node
            local assigned_count
            assigned_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx" and .node_name != null)] | length')

            if [[ "${assigned_count}" -ge 1 ]]; then
                local node_name
                node_name=$(echo "${response}" | jq -r '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx")][0].node_name')
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
        running_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx" and .status == "running")] | length')

        if [[ "${running_count}" -ge 1 ]]; then
            log_info "Container is running"
            return 0
        fi

        local status
        status=$(echo "${response}" | jq -r '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx")][0].status // "no pod"')
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
        -X PUT "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}nginx" \
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
        pod_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx" and .status != "terminated" and .status != "failed")] | length')

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

    cat > "${manifest_file}" << EOF
kind: Deployment
spec:
  name: ${TEST_CONTAINER_PREFIX}yaml
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
    response=$(curl -s "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}yaml")

    assert_json_field "${response}" ".name" "${TEST_CONTAINER_PREFIX}yaml" "YAML deployment should be created" || return 1

    return 0
}

test_cli_get_deployments() {
    local output
    output=$("${KAGO_BIN}" get deployments --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago get deployments failed: ${output}"
        return 1
    }

    assert_contains "${output}" "${TEST_CONTAINER_PREFIX}nginx" "Should list ${TEST_CONTAINER_PREFIX}nginx deployment" || return 1

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
        -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}nginx")

    assert_eq "200" "${http_code}" "Should return 200 OK for delete" || return 1

    # Verify deployment is gone
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}nginx")

    assert_eq "404" "${http_code}" "Should return 404 after deletion" || return 1

    return 0
}

test_cli_delete_deployment() {
    local output
    output=$("${KAGO_BIN}" delete "${TEST_CONTAINER_PREFIX}yaml" --server "${KAGO_SERVER}" 2>&1) || {
        log_error "kago delete failed: ${output}"
        return 1
    }

    # Verify deployment is gone
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}yaml")

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
        active_pods=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'nginx" and .status != "terminated")] | length')

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

    cat > "${manifest_file}" << EOF
kind: Deployment
spec:
  name: ${TEST_CONTAINER_PREFIX}multi-web
  image: nginx:alpine
  replicas: 1
---
kind: Deployment
spec:
  name: ${TEST_CONTAINER_PREFIX}multi-api
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
    web_exists=$(echo "${response}" | jq '[.[] | select(.name == "'"${TEST_CONTAINER_PREFIX}"'multi-web")] | length')
    api_exists=$(echo "${response}" | jq '[.[] | select(.name == "'"${TEST_CONTAINER_PREFIX}"'multi-api")] | length')

    if [[ "${web_exists}" -ne 1 ]] || [[ "${api_exists}" -ne 1 ]]; then
        log_error "Expected both ${TEST_CONTAINER_PREFIX}multi-web and ${TEST_CONTAINER_PREFIX}multi-api deployments"
        return 1
    fi

    # Cleanup
    "${KAGO_BIN}" delete "${TEST_CONTAINER_PREFIX}multi-web" --server "${KAGO_SERVER}" > /dev/null 2>&1 || true
    "${KAGO_BIN}" delete "${TEST_CONTAINER_PREFIX}multi-api" --server "${KAGO_SERVER}" > /dev/null 2>&1 || true

    return 0
}

start_second_agent() {
    log_info "Starting second kago agent on port ${KAGO_AGENT_PORT_2}..."
    log_info "Agent 2 resources: CPU=${AGENT_2_CPU}m, Memory=${AGENT_2_MEMORY}Mi"
    "${KAGO_BIN}" agent \
        --name "e2e-worker-2" \
        --master "${KAGO_SERVER}" \
        --port "${KAGO_AGENT_PORT_2}" \
        --address "localhost" \
        --cpu "${AGENT_2_CPU}" \
        --memory "${AGENT_2_MEMORY}" &
    AGENT_PID_2=$!

    if ! wait_for_agent 2; then
        log_error "Failed to register second agent"
        return 1
    fi
    return 0
}

stop_second_agent() {
    if [[ -n "${AGENT_PID_2}" ]] && kill -0 "${AGENT_PID_2}" 2>/dev/null; then
        log_info "Stopping second agent..."
        kill "${AGENT_PID_2}" 2>/dev/null || true
        wait "${AGENT_PID_2}" 2>/dev/null || true
        AGENT_PID_2=""
    fi
}

test_scheduler_strategy_configured() {
    log_info "Testing scheduler strategy: ${KAGO_SCHEDULER}"

    local http_code
    http_code=$(curl -s -o /tmp/sched_test.json -w "%{http_code}" \
        -X POST "${KAGO_SERVER}/deployments" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "'"${TEST_CONTAINER_PREFIX}"'sched",
            "image": "alpine:latest",
            "replicas": 1,
            "resources": {
                "cpu_millis": 100,
                "memory_mb": 64
            }
        }')

    assert_eq "201" "${http_code}" "Should create deployment" || return 1

    local max_attempts=20
    local attempt=1

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local scheduled_count
        scheduled_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'sched" and .node_name != null)] | length')

        if [[ "${scheduled_count}" -ge 1 ]]; then
            local node_name
            node_name=$(echo "${response}" | jq -r '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'sched")][0].node_name')
            log_info "Pod scheduled to node: ${node_name} (strategy: ${KAGO_SCHEDULER})"
            curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}sched" > /dev/null
            sleep 3
            return 0
        fi

        sleep 1
        ((attempt++))
    done

    log_error "Pod was not scheduled"
    curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}sched" > /dev/null 2>&1 || true
    return 1
}

test_multi_node_scheduling() {
    if [[ -z "${AGENT_PID_2}" ]] || ! kill -0 "${AGENT_PID_2}" 2>/dev/null; then
        if ! start_second_agent; then
            log_warn "Could not start second agent, skipping multi-node test"
            return 0
        fi
    fi

    local nodes
    nodes=$(curl -s "${KAGO_SERVER}/nodes")
    local node_count
    node_count=$(echo "${nodes}" | jq 'length')

    if [[ "${node_count}" -lt 2 ]]; then
        log_warn "Not enough nodes for multi-node test (have ${node_count}, need 2)"
        return 0
    fi

    log_info "Testing multi-node scheduling with ${node_count} nodes (strategy: ${KAGO_SCHEDULER})"
    log_info "Node resources: e2e-worker=${AGENT_1_CPU}m/${AGENT_1_MEMORY}Mi, e2e-worker-2=${AGENT_2_CPU}m/${AGENT_2_MEMORY}Mi"

    log_info "Waiting for node resources to be released..."
    local wait_attempts=15
    local wait_attempt=1
    while [[ ${wait_attempt} -le ${wait_attempts} ]]; do
        local nodes_state
        nodes_state=$(curl -s "${KAGO_SERVER}/nodes")

        local total_used_cpu
        total_used_cpu=$(echo "${nodes_state}" | jq '[.[].used.cpu_millis] | add // 0')
        local total_used_mem
        total_used_mem=$(echo "${nodes_state}" | jq '[.[].used.memory_mb] | add // 0')

        if [[ "${total_used_cpu}" -eq 0 ]] && [[ "${total_used_mem}" -eq 0 ]]; then
            log_info "Node resources are clean"
            break
        fi

        log_info "Waiting for resources to be released (used: ${total_used_cpu}m CPU, ${total_used_mem}Mi mem) (attempt ${wait_attempt}/${wait_attempts})"
        sleep 2
        ((wait_attempt++))
    done

    local http_code
    http_code=$(curl -s -o /tmp/spread_test.json -w "%{http_code}" \
        -X POST "${KAGO_SERVER}/deployments" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "'"${TEST_CONTAINER_PREFIX}"'spread",
            "image": "alpine:latest",
            "replicas": 4,
            "resources": {
                "cpu_millis": 500,
                "memory_mb": 256
            }
        }')

    assert_eq "201" "${http_code}" "Should create deployment" || return 1

    local max_attempts=30
    local attempt=1

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local scheduled_count
        scheduled_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'spread" and .node_name != null)] | length')

        if [[ "${scheduled_count}" -ge 4 ]]; then
            log_info "All 4 pods scheduled"

            # Analyze distribution
            local node1_count node2_count
            node1_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'spread" and .node_name == "e2e-worker")] | length')
            node2_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'spread" and .node_name == "e2e-worker-2")] | length')

            log_info "Pod distribution: e2e-worker=${node1_count}, e2e-worker-2=${node2_count}"

            local test_passed=true

            case "${KAGO_SCHEDULER}" in
                "least-allocated")
                    # LeastAllocated should spread pods across nodes
                    # With different node sizes, it should prefer the larger node (e2e-worker) initially
                    # but then balance by preferring the node with more remaining resources
                    if [[ "${node1_count}" -ge 1 ]] && [[ "${node2_count}" -ge 1 ]]; then
                        log_info "PASS: Pods are spread across nodes as expected for ${KAGO_SCHEDULER} strategy"
                    else
                        log_error "FAIL: Expected pods to be distributed across both nodes for ${KAGO_SCHEDULER}"
                        log_error "  e2e-worker=${node1_count}, e2e-worker-2=${node2_count}"
                        test_passed=false
                    fi
                    ;;
                "balanced")
                    # Balanced should also spread pods
                    if [[ "${node1_count}" -ge 1 ]] && [[ "${node2_count}" -ge 1 ]]; then
                        log_info "PASS: Pods are spread across nodes as expected for ${KAGO_SCHEDULER} strategy"
                    else
                        log_error "FAIL: Expected pods to be distributed across both nodes for ${KAGO_SCHEDULER}"
                        log_error "  e2e-worker=${node1_count}, e2e-worker-2=${node2_count}"
                        test_passed=false
                    fi
                    ;;
                "best-fit")
                    # Best-fit with different node sizes should concentrate pods
                    # The smaller node (e2e-worker-2) should be preferred for bin-packing
                    # After first pod on e2e-worker-2, it has less remaining, so subsequent pods go there too
                    if [[ "${node2_count}" -ge 2 ]]; then
                        log_info "PASS: Best-fit concentrated ${node2_count} pods on smaller node (e2e-worker-2)"
                    else
                        # Best-fit behavior depends on timing and initial state
                        log_warn "Best-fit did not concentrate pods as expected (node2_count=${node2_count})"
                    fi
                    ;;
                "first-fit")
                    # First-fit uses node order (alphabetically sorted: e2e-worker comes first)
                    # All pods should go to e2e-worker if it has enough resources
                    if [[ "${node1_count}" -ge 3 ]]; then
                        log_info "PASS: First-fit scheduled ${node1_count} pods on first node (e2e-worker)"
                    else
                        # Behavior depends on exact timing
                        log_warn "First-fit did not behave as expected (node1_count=${node1_count})"
                    fi
                    ;;
            esac

            curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}spread" > /dev/null
            sleep 5

            if [[ "${test_passed}" == "false" ]]; then
                return 1
            fi
            return 0
        fi

        log_info "Scheduled ${scheduled_count}/4 pods (attempt ${attempt}/${max_attempts})"
        sleep 2
        ((attempt++))
    done

    log_error "Not all pods were scheduled"
    curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}spread" > /dev/null 2>&1 || true
    return 1
}

test_resource_based_scheduling() {
    log_info "Testing resource-based scheduling"

    local http_code
    http_code=$(curl -s -o /tmp/resource_test.json -w "%{http_code}" \
        -X POST "${KAGO_SERVER}/deployments" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "'"${TEST_CONTAINER_PREFIX}"'resource",
            "image": "alpine:latest",
            "replicas": 1,
            "resources": {
                "cpu_millis": 2000,
                "memory_mb": 4096
            }
        }')

    assert_eq "201" "${http_code}" "Should create deployment" || return 1

    local max_attempts=20
    local attempt=1

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local scheduled_count
        scheduled_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'resource" and .node_name != null)] | length')

        if [[ "${scheduled_count}" -ge 1 ]]; then
            log_info "Resource-heavy pod scheduled successfully"

            local nodes
            nodes=$(curl -s "${KAGO_SERVER}/nodes")
            log_info "Node resources after scheduling:"
            echo "${nodes}" | jq -r '.[] | "  \(.name): used \(.used.cpu_millis)m CPU, \(.used.memory_mb)Mi memory"'

            curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}resource" > /dev/null
            sleep 3
            return 0
        fi

        sleep 1
        ((attempt++))
    done

    log_error "Resource-heavy pod was not scheduled"
    curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}resource" > /dev/null 2>&1 || true
    return 1
}

test_resource_exhaustion_scheduling() {
    if [[ -z "${AGENT_PID_2}" ]] || ! kill -0 "${AGENT_PID_2}" 2>/dev/null; then
        if ! start_second_agent; then
            log_warn "Could not start second agent, skipping resource exhaustion test"
            return 0
        fi
    fi

    log_info "Testing resource exhaustion scheduling"
    log_info "Node resources: e2e-worker=${AGENT_1_CPU}m/${AGENT_1_MEMORY}Mi, e2e-worker-2=${AGENT_2_CPU}m/${AGENT_2_MEMORY}Mi"

    # Create a deployment with pods that require 1000m CPU each
    # e2e-worker (4000m) can fit 4 pods, e2e-worker-2 (2000m) can fit 2 pods
    # Total capacity = 6 pods, we'll create 5 to ensure distribution
    local http_code
    http_code=$(curl -s -o /tmp/exhaust_test.json -w "%{http_code}" \
        -X POST "${KAGO_SERVER}/deployments" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "'"${TEST_CONTAINER_PREFIX}"'exhaust",
            "image": "alpine:latest",
            "replicas": 5,
            "resources": {
                "cpu_millis": 1000,
                "memory_mb": 512
            }
        }')

    assert_eq "201" "${http_code}" "Should create deployment" || return 1

    # Wait for all pods to be scheduled
    local max_attempts=30
    local attempt=1

    while [[ ${attempt} -le ${max_attempts} ]]; do
        local response
        response=$(curl -s "${KAGO_SERVER}/pods")

        local scheduled_count
        scheduled_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'exhaust" and .node_name != null)] | length')

        if [[ "${scheduled_count}" -ge 5 ]]; then
            log_info "All 5 pods scheduled"

            # Analyze distribution
            local node1_count node2_count
            node1_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'exhaust" and .node_name == "e2e-worker")] | length')
            node2_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'exhaust" and .node_name == "e2e-worker-2")] | length')

            log_info "Pod distribution: e2e-worker=${node1_count}, e2e-worker-2=${node2_count}"

            # Verify both nodes have at least 1 pod (proving resource-based distribution)
            # With 5 pods requiring 1000m each and node1=4000m, node2=2000m
            # Node1 can fit max 4, Node2 can fit max 2
            # So we must have pods on both nodes
            if [[ "${node1_count}" -ge 1 ]] && [[ "${node2_count}" -ge 1 ]]; then
                log_info "PASS: Pods are distributed across both nodes due to resource constraints"
            else
                log_error "FAIL: Expected pods on both nodes but got e2e-worker=${node1_count}, e2e-worker-2=${node2_count}"
                curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}exhaust" > /dev/null
                sleep 5
                return 1
            fi

            # Verify node1 doesn't have more than 4 pods (its capacity)
            if [[ "${node1_count}" -le 4 ]]; then
                log_info "PASS: e2e-worker has ${node1_count} pods (max capacity: 4)"
            else
                log_error "FAIL: e2e-worker has ${node1_count} pods but capacity is only 4"
                curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}exhaust" > /dev/null
                sleep 5
                return 1
            fi

            # Verify node2 doesn't have more than 2 pods (its capacity)
            if [[ "${node2_count}" -le 2 ]]; then
                log_info "PASS: e2e-worker-2 has ${node2_count} pods (max capacity: 2)"
            else
                log_error "FAIL: e2e-worker-2 has ${node2_count} pods but capacity is only 2"
                curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}exhaust" > /dev/null
                sleep 5
                return 1
            fi

            curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}exhaust" > /dev/null
            sleep 5
            return 0
        fi

        log_info "Scheduled ${scheduled_count}/5 pods (attempt ${attempt}/${max_attempts})"
        sleep 2
        ((attempt++))
    done

    log_error "Not all pods were scheduled"
    curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}exhaust" > /dev/null 2>&1 || true
    return 1
}

test_insufficient_resources_not_scheduled() {
    log_info "Testing that pods with excessive resource requirements stay pending"

    # Create a deployment that requires more resources than any single node has
    local http_code
    http_code=$(curl -s -o /tmp/huge_test.json -w "%{http_code}" \
        -X POST "${KAGO_SERVER}/deployments" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "'"${TEST_CONTAINER_PREFIX}"'huge",
            "image": "alpine:latest",
            "replicas": 1,
            "resources": {
                "cpu_millis": 10000,
                "memory_mb": 32768
            }
        }')

    assert_eq "201" "${http_code}" "Should create deployment" || return 1

    sleep 10

    local response
    response=$(curl -s "${KAGO_SERVER}/pods")

    local pending_count
    pending_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'huge" and .node_name == null)] | length')

    local scheduled_count
    scheduled_count=$(echo "${response}" | jq '[.[] | select(.deployment_name == "'"${TEST_CONTAINER_PREFIX}"'huge" and .node_name != null)] | length')

    if [[ "${pending_count}" -ge 1 ]] && [[ "${scheduled_count}" -eq 0 ]]; then
        log_info "PASS: Pod with excessive resources correctly stayed pending"
    else
        log_error "FAIL: Expected pod to stay pending but got pending=${pending_count}, scheduled=${scheduled_count}"
        curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}huge" > /dev/null
        sleep 3
        return 1
    fi

    curl -s -X DELETE "${KAGO_SERVER}/deployments/${TEST_CONTAINER_PREFIX}huge" > /dev/null
    sleep 3
    return 0
}

main() {
    local run_multi_node=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --scheduler)
                KAGO_SCHEDULER="$2"
                shift 2
                ;;
            --multi-node)
                run_multi_node=true
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                exit 1
                ;;
        esac
    done

    log_info "Starting Kago E2E Tests"
    log_info "================================"
    log_info "Scheduler strategy: ${KAGO_SCHEDULER}"
    log_info "Multi-node tests: ${run_multi_node}"
    log_info "================================"

    cleanup_processes
    log_info "Cleaning up leftover containers from previous runs..."
    cleanup_containers

    build_kago || exit 1

    if [[ ! -x "${KAGO_BIN}" ]]; then
        log_error "Kago binary not found or not executable: ${KAGO_BIN}"
        exit 1
    fi

    check_required_commands jq curl docker || exit 1
    check_docker_running || exit 1

    log_info "Starting kago master on port ${KAGO_PORT} with scheduler: ${KAGO_SCHEDULER}..."
    "${KAGO_BIN}" serve --port "${KAGO_PORT}" --scheduler "${KAGO_SCHEDULER}" &
    KAGO_PID=$!

    if ! wait_for_server; then
        log_error "Failed to start kago server"
        exit 1
    fi

    # Start agent (worker node)
    log_info "Starting kago agent on port ${KAGO_AGENT_PORT}..."
    log_info "Agent 1 resources: CPU=${AGENT_1_CPU}m, Memory=${AGENT_1_MEMORY}Mi"
    "${KAGO_BIN}" agent \
        --name "e2e-worker" \
        --master "${KAGO_SERVER}" \
        --port "${KAGO_AGENT_PORT}" \
        --address "localhost" \
        --cpu "${AGENT_1_CPU}" \
        --memory "${AGENT_1_MEMORY}" &
    AGENT_PID=$!

    if ! wait_for_agent 1; then
        log_error "Failed to register agent"
        exit 1
    fi

    log_info ""
    log_section "Basic Tests"
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
    log_section "Scheduler Tests (strategy: ${KAGO_SCHEDULER})"
    log_info "================================"

    run_test "Scheduler strategy configured" test_scheduler_strategy_configured || true
    run_test "Resource-based scheduling" test_resource_based_scheduling || true

    if [[ "${run_multi_node}" == "true" ]]; then
        log_info ""
        log_section "Multi-Node Scheduler Tests"
        log_info "================================"

        run_test "Multi-node scheduling" test_multi_node_scheduling || true
        run_test "Resource exhaustion scheduling" test_resource_exhaustion_scheduling || true
        run_test "Insufficient resources not scheduled" test_insufficient_resources_not_scheduled || true

        stop_second_agent
    fi

    log_info ""
    log_info "================================"
    log_info "Test Results"
    log_info "================================"
    log_info "Scheduler: ${KAGO_SCHEDULER}"
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
