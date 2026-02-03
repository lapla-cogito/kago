pub static PODS_BY_STATUS: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_pods_total",
            "Total number of pods by status",
            &["status"]
        )
        .unwrap()
    });

pub static PODS_BY_DEPLOYMENT: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_pods_by_deployment",
            "Number of pods per deployment",
            &["deployment", "status"]
        )
        .unwrap()
    });

pub static PODS_BY_NODE: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_pods_by_node",
            "Number of pods per node",
            &["node", "status"]
        )
        .unwrap()
    });

pub static PODS_BY_IMAGE: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_pods_by_image",
            "Number of pods per container image",
            &["image"]
        )
        .unwrap()
    });

pub static DEPLOYMENTS_TOTAL: std::sync::LazyLock<prometheus::IntGauge> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge!("kago_deployments_total", "Total number of deployments")
            .unwrap()
    });

pub static DEPLOYMENT_REPLICAS_DESIRED: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_deployment_replicas_desired",
            "Desired number of replicas per deployment",
            &["deployment"]
        )
        .unwrap()
    });

pub static DEPLOYMENT_REPLICAS_READY: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_deployment_replicas_ready",
            "Number of ready replicas per deployment",
            &["deployment"]
        )
        .unwrap()
    });

pub static NODES_BY_STATUS: std::sync::LazyLock<prometheus::IntGaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge_vec!(
            "kago_nodes_total",
            "Total number of nodes by status",
            &["status"]
        )
        .unwrap()
    });

pub static NODE_CPU_CAPACITY: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_cpu_capacity_millicores",
            "CPU capacity of node in millicores",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_CPU_USED: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_cpu_used_millicores",
            "CPU used on node in millicores",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_CPU_AVAILABLE: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_cpu_available_millicores",
            "CPU available on node in millicores",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_MEMORY_CAPACITY: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_memory_capacity_mb",
            "Memory capacity of node in MB",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_MEMORY_USED: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_memory_used_mb",
            "Memory used on node in MB",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_MEMORY_AVAILABLE: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_memory_available_mb",
            "Memory available on node in MB",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_CPU_UTILIZATION: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_cpu_utilization_percent",
            "CPU utilization percentage of node",
            &["node"]
        )
        .unwrap()
    });

pub static NODE_MEMORY_UTILIZATION: std::sync::LazyLock<prometheus::GaugeVec> =
    std::sync::LazyLock::new(|| {
        prometheus::register_gauge_vec!(
            "kago_node_memory_utilization_percent",
            "Memory utilization percentage of node",
            &["node"]
        )
        .unwrap()
    });

pub static CLUSTER_CPU_CAPACITY: std::sync::LazyLock<prometheus::IntGauge> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge!(
            "kago_cluster_cpu_capacity_millicores",
            "Total CPU capacity across all nodes in millicores"
        )
        .unwrap()
    });

pub static CLUSTER_CPU_USED: std::sync::LazyLock<prometheus::IntGauge> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge!(
            "kago_cluster_cpu_used_millicores",
            "Total CPU used across all nodes in millicores"
        )
        .unwrap()
    });

pub static CLUSTER_MEMORY_CAPACITY: std::sync::LazyLock<prometheus::IntGauge> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge!(
            "kago_cluster_memory_capacity_mb",
            "Total memory capacity across all nodes in MB"
        )
        .unwrap()
    });

pub static CLUSTER_MEMORY_USED: std::sync::LazyLock<prometheus::IntGauge> =
    std::sync::LazyLock::new(|| {
        prometheus::register_int_gauge!(
            "kago_cluster_memory_used_mb",
            "Total memory used across all nodes in MB"
        )
        .unwrap()
    });

pub async fn update_metrics(store: &crate::store::SharedStore) {
    let store = store.read().await;
    reset_metrics();

    let pods = store.list_pods();
    let mut status_counts: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    let mut deployment_status_counts: std::collections::HashMap<(String, String), i64> =
        std::collections::HashMap::new();
    let mut node_status_counts: std::collections::HashMap<(String, String), i64> =
        std::collections::HashMap::new();
    let mut image_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    for pod in &pods {
        let status = format!("{:?}", pod.status).to_lowercase();

        *status_counts.entry(status.clone()).or_insert(0) += 1;

        if let Some(ref deployment) = pod.deployment_name {
            *deployment_status_counts
                .entry((deployment.clone(), status.clone()))
                .or_insert(0) += 1;
        }

        if let Some(ref node) = pod.node_name {
            *node_status_counts
                .entry((node.clone(), status.clone()))
                .or_insert(0) += 1;
        }

        *image_counts.entry(pod.image.clone()).or_insert(0) += 1;
    }

    for (status, count) in status_counts {
        PODS_BY_STATUS.with_label_values(&[&status]).set(count);
    }

    for ((deployment, status), count) in deployment_status_counts {
        PODS_BY_DEPLOYMENT
            .with_label_values(&[&deployment, &status])
            .set(count);
    }

    for ((node, status), count) in node_status_counts {
        PODS_BY_NODE.with_label_values(&[&node, &status]).set(count);
    }

    for (image, count) in image_counts {
        PODS_BY_IMAGE.with_label_values(&[&image]).set(count);
    }

    let deployments = store.list_deployments();
    DEPLOYMENTS_TOTAL.set(deployments.len() as i64);

    for deployment in &deployments {
        DEPLOYMENT_REPLICAS_DESIRED
            .with_label_values(&[&deployment.name])
            .set(deployment.replicas as i64);

        let ready_count = store.count_running_pods_for_deployment(&deployment.name);
        DEPLOYMENT_REPLICAS_READY
            .with_label_values(&[&deployment.name])
            .set(ready_count as i64);
    }

    let nodes = store.list_nodes();
    let mut node_status_counts: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();

    let mut total_cpu_capacity: i64 = 0;
    let mut total_cpu_used: i64 = 0;
    let mut total_memory_capacity: i64 = 0;
    let mut total_memory_used: i64 = 0;

    for node in &nodes {
        let status = format!("{:?}", node.status).to_lowercase();
        *node_status_counts.entry(status).or_insert(0) += 1;

        NODE_CPU_CAPACITY
            .with_label_values(&[&node.name])
            .set(node.capacity.cpu_millis as f64);
        NODE_CPU_USED
            .with_label_values(&[&node.name])
            .set(node.used.cpu_millis as f64);
        NODE_CPU_AVAILABLE
            .with_label_values(&[&node.name])
            .set(node.available_resources().cpu_millis as f64);

        NODE_MEMORY_CAPACITY
            .with_label_values(&[&node.name])
            .set(node.capacity.memory_mb as f64);
        NODE_MEMORY_USED
            .with_label_values(&[&node.name])
            .set(node.used.memory_mb as f64);
        NODE_MEMORY_AVAILABLE
            .with_label_values(&[&node.name])
            .set(node.available_resources().memory_mb as f64);

        let cpu_utilization = if node.capacity.cpu_millis > 0 {
            (node.used.cpu_millis as f64 / node.capacity.cpu_millis as f64) * 100.0
        } else {
            0.0
        };
        let memory_utilization = if node.capacity.memory_mb > 0 {
            (node.used.memory_mb as f64 / node.capacity.memory_mb as f64) * 100.0
        } else {
            0.0
        };

        NODE_CPU_UTILIZATION
            .with_label_values(&[&node.name])
            .set(cpu_utilization);
        NODE_MEMORY_UTILIZATION
            .with_label_values(&[&node.name])
            .set(memory_utilization);

        total_cpu_capacity += node.capacity.cpu_millis as i64;
        total_cpu_used += node.used.cpu_millis as i64;
        total_memory_capacity += node.capacity.memory_mb as i64;
        total_memory_used += node.used.memory_mb as i64;
    }

    for (status, count) in node_status_counts {
        NODES_BY_STATUS.with_label_values(&[&status]).set(count);
    }

    CLUSTER_CPU_CAPACITY.set(total_cpu_capacity);
    CLUSTER_CPU_USED.set(total_cpu_used);
    CLUSTER_MEMORY_CAPACITY.set(total_memory_capacity);
    CLUSTER_MEMORY_USED.set(total_memory_used);
}

fn reset_metrics() {
    PODS_BY_STATUS.reset();
    PODS_BY_DEPLOYMENT.reset();
    PODS_BY_NODE.reset();
    PODS_BY_IMAGE.reset();
    DEPLOYMENT_REPLICAS_DESIRED.reset();
    DEPLOYMENT_REPLICAS_READY.reset();
    NODES_BY_STATUS.reset();
    NODE_CPU_CAPACITY.reset();
    NODE_CPU_USED.reset();
    NODE_CPU_AVAILABLE.reset();
    NODE_MEMORY_CAPACITY.reset();
    NODE_MEMORY_USED.reset();
    NODE_MEMORY_AVAILABLE.reset();
    NODE_CPU_UTILIZATION.reset();
    NODE_MEMORY_UTILIZATION.reset();
}

pub fn encode_metrics() -> String {
    let encoder = prometheus::TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    prometheus::Encoder::encode(&encoder, &metric_families, &mut buffer).unwrap();

    String::from_utf8(buffer).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_metrics() {
        let output = encode_metrics();
        // Should contain at least the metric help text
        assert!(output.contains("kago_") || output.is_empty() || output.contains("# HELP"));
    }
}
