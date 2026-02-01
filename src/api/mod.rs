mod deployments;
mod health;
mod nodes;
mod pods;

pub struct AppState {
    pub store: crate::store::SharedStore,
    pub controller: std::sync::Arc<crate::controller::Controller>,
}

pub(crate) fn json_error<S: Into<String>>(
    status: axum::http::StatusCode,
    message: S,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        status,
        axum::Json(serde_json::json!({ "error": message.into() })),
    )
}

pub fn create_router(
    store: crate::store::SharedStore,
    controller: std::sync::Arc<crate::controller::Controller>,
) -> axum::Router {
    let state = std::sync::Arc::new(AppState { store, controller });

    axum::Router::new()
        .route("/health", axum::routing::get(health::health_check))
        .route(
            "/deployments",
            axum::routing::get(deployments::list_deployments),
        )
        .route(
            "/deployments",
            axum::routing::post(deployments::create_deployment),
        )
        .route(
            "/deployments/{name}",
            axum::routing::get(deployments::get_deployment),
        )
        .route(
            "/deployments/{name}",
            axum::routing::put(deployments::update_deployment),
        )
        .route(
            "/deployments/{name}",
            axum::routing::delete(deployments::delete_deployment),
        )
        .route("/pods", axum::routing::get(pods::list_pods))
        .route("/pods/{id}", axum::routing::get(pods::get_pod))
        .route("/pods/{id}", axum::routing::delete(pods::delete_pod))
        .route("/nodes", axum::routing::get(nodes::list_nodes))
        .route("/nodes/register", axum::routing::post(nodes::register_node))
        .route("/nodes/{name}", axum::routing::get(nodes::get_node))
        .route("/nodes/{name}", axum::routing::delete(nodes::delete_node))
        .route(
            "/nodes/{name}/heartbeat",
            axum::routing::post(nodes::node_heartbeat),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_create_deployment_request_parsing() {
        let json = r#"{"name": "web", "image": "nginx:latest", "replicas": 3}"#;
        let req: crate::models::CreateDeploymentRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "web");
        assert_eq!(req.image, "nginx:latest");
        assert_eq!(req.replicas, 3);
    }

    #[test]
    fn test_update_deployment_request_parsing() {
        let json = r#"{"replicas": 5}"#;
        let req: crate::models::UpdateDeploymentRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.replicas, Some(5));
        assert_eq!(req.image, None);
    }

    #[test]
    fn test_register_node_request_parsing() {
        let json = r#"{"name": "worker-1", "address": "192.168.1.10", "port": 8081, "capacity": {"cpu_millis": 4000, "memory_mb": 8192}}"#;
        let req: crate::models::RegisterNodeRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "worker-1");
        assert_eq!(req.address, "192.168.1.10");
        assert_eq!(req.port, 8081);
        assert_eq!(req.capacity.cpu_millis, 4000);
        assert_eq!(req.capacity.memory_mb, 8192);
    }

    #[test]
    fn test_heartbeat_request_parsing() {
        let json = r#"{"used": {"cpu_millis": 1000, "memory_mb": 2048}, "pod_statuses": []}"#;
        let req: crate::models::HeartbeatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.used.cpu_millis, 1000);
        assert_eq!(req.used.memory_mb, 2048);
        assert!(req.pod_statuses.is_empty());
    }
}
