pub(super) async fn list_nodes(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    let nodes: Vec<crate::models::NodeResponse> = store
        .list_nodes()
        .into_iter()
        .map(|node| crate::models::NodeResponse::from(&node))
        .collect();

    axum::Json(nodes)
}

pub(super) async fn register_node(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::Json(req): axum::Json<crate::models::RegisterNodeRequest>,
) -> impl axum::response::IntoResponse {
    tracing::info!(
        "Registering node: {} at {}:{}",
        req.name,
        req.address,
        req.port
    );

    if req.name.is_empty() {
        return crate::api::json_error(
            axum::http::StatusCode::BAD_REQUEST,
            "Node name cannot be empty",
        );
    }

    {
        let store = state.store.read().await;
        if store.get_node(&req.name).is_some() {
            tracing::info!("Node '{}' re-registering", req.name);
        }
    }

    let node = crate::models::Node::new(req.name.clone(), req.address, req.port, req.capacity);

    let response = crate::models::NodeResponse::from(&node);

    {
        let mut store = state.store.write().await;
        store.register_node(node);
    }

    tracing::info!(
        "Node '{}' registered with capacity: {}m CPU, {}Mi memory",
        req.name,
        req.capacity.cpu_millis,
        req.capacity.memory_mb
    );

    (
        axum::http::StatusCode::CREATED,
        axum::Json(serde_json::to_value(response).unwrap()),
    )
}

pub(super) async fn get_node(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    match store.get_node(&name) {
        Some(node) => {
            let response = crate::models::NodeResponse::from(node);
            (
                axum::http::StatusCode::OK,
                axum::Json(serde_json::to_value(response).unwrap()),
            )
        }
        None => crate::api::json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Node '{}' not found", name),
        ),
    }
}

pub(super) async fn delete_node(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Deleting node: {}", name);

    let mut store = state.store.write().await;

    match store.delete_node(&name) {
        Some(_) => {
            tracing::info!("Node '{}' deleted", name);
            (
                axum::http::StatusCode::OK,
                axum::Json(serde_json::json!({ "message": format!("Node '{}' deleted", name) })),
            )
        }
        None => crate::api::json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Node '{}' not found", name),
        ),
    }
}

pub(super) async fn node_heartbeat(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    axum::Json(req): axum::Json<crate::models::HeartbeatRequest>,
) -> impl axum::response::IntoResponse {
    tracing::debug!("Heartbeat from node: {}", name);

    let mut store = state.store.write().await;

    if store.get_node(&name).is_none() {
        return crate::api::json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Node '{}' not found", name),
        );
    }

    store.update_node_heartbeat(&name);
    store.update_node_resources(&name, req.used);

    for pod_status in &req.pod_statuses {
        if let Some(pod) = store.get_pod_mut(&pod_status.pod_id) {
            if pod.status != pod_status.status
                && !matches!(
                    pod.status,
                    crate::models::PodStatus::Terminated | crate::models::PodStatus::Terminating
                )
            {
                tracing::debug!(
                    "Pod {} status update from agent: {:?} -> {:?}",
                    pod.name,
                    pod.status,
                    pod_status.status
                );
                pod.status = pod_status.status;
            }
            if let Some(ref container_id) = pod_status.container_id {
                pod.container_id = Some(container_id.clone());
            }
        }
    }

    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({ "status": "ok" })),
    )
}
