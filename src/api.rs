pub struct AppState {
    pub store: crate::store::SharedStore,
    pub controller: std::sync::Arc<crate::controller::Controller>,
}

fn json_error<S: Into<String>>(
    status: axum::http::StatusCode,
    message: S,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        status,
        axum::Json(serde_json::json!({
            "error": message.into()
        })),
    )
}

pub fn create_router(
    store: crate::store::SharedStore,
    controller: std::sync::Arc<crate::controller::Controller>,
) -> axum::Router {
    let state = std::sync::Arc::new(AppState { store, controller });

    axum::Router::new()
        .route("/health", axum::routing::get(health_check))
        .route("/deployments", axum::routing::get(list_deployments))
        .route("/deployments", axum::routing::post(create_deployment))
        .route("/deployments/{name}", axum::routing::get(get_deployment))
        .route("/deployments/{name}", axum::routing::put(update_deployment))
        .route(
            "/deployments/{name}",
            axum::routing::delete(delete_deployment),
        )
        .route("/pods", axum::routing::get(list_pods))
        .route("/pods/{id}", axum::routing::get(get_pod))
        .route("/pods/{id}", axum::routing::delete(delete_pod))
        .with_state(state)
}

async fn health_check() -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({
        "status": "healthy"
    }))
}

async fn list_deployments(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    let deployments: Vec<crate::models::DeploymentResponse> = store
        .list_deployments()
        .iter()
        .map(|d| {
            let ready = store.count_running_pods_for_deployment(&d.name);
            crate::models::DeploymentResponse::from_deployment(d, ready)
        })
        .collect();

    axum::Json(deployments)
}

async fn create_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    axum::Json(req): axum::Json<crate::models::CreateDeploymentRequest>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Creating deployment: {}", req.name);

    if req.name.is_empty() {
        return json_error(
            axum::http::StatusCode::BAD_REQUEST,
            "Deployment name cannot be empty",
        );
    }

    if req.image.is_empty() {
        return json_error(axum::http::StatusCode::BAD_REQUEST, "Image cannot be empty");
    }

    {
        let store = state.store.read().await;
        if store.get_deployment(&req.name).is_some() {
            return json_error(
                axum::http::StatusCode::CONFLICT,
                format!("Deployment '{}' already exists", req.name),
            );
        }
    }

    let deployment = crate::models::Deployment {
        name: req.name,
        image: req.image,
        replicas: req.replicas,
        resources: req.resources,
    };

    let response_body = serde_json::json!({
        "name": &deployment.name,
        "image": &deployment.image,
        "replicas": deployment.replicas,
        "resources": deployment.resources
    });

    tracing::info!(
        "Deployment {} created with {} replicas",
        deployment.name,
        deployment.replicas
    );

    {
        let mut store = state.store.write().await;
        store.upsert_deployment(deployment);
    }

    (axum::http::StatusCode::CREATED, axum::Json(response_body))
}

async fn get_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    match store.get_deployment(&name) {
        Some(deployment) => {
            let ready = store.count_running_pods_for_deployment(&name);
            let response = crate::models::DeploymentResponse::from_deployment(deployment, ready);
            (
                axum::http::StatusCode::OK,
                axum::Json(serde_json::to_value(response).unwrap()),
            )
        }
        None => json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Deployment '{}' not found", name),
        ),
    }
}

async fn update_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    axum::Json(req): axum::Json<crate::models::UpdateDeploymentRequest>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Updating deployment: {}", name);

    let mut store = state.store.write().await;

    match store.get_deployment(&name).cloned() {
        Some(mut deployment) => {
            if let Some(replicas) = req.replicas {
                deployment.replicas = replicas;
            }
            if let Some(image) = req.image {
                deployment.image = image;
            }

            store.upsert_deployment(deployment.clone());

            let ready = store.count_running_pods_for_deployment(&name);
            let response = crate::models::DeploymentResponse::from_deployment(&deployment, ready);

            tracing::info!(
                "Deployment {} updated: replicas={}, image={}",
                name,
                deployment.replicas,
                deployment.image
            );

            (
                axum::http::StatusCode::OK,
                axum::Json(serde_json::to_value(response).unwrap()),
            )
        }
        None => json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Deployment '{}' not found", name),
        ),
    }
}

async fn delete_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Deleting deployment: {}", name);

    {
        let mut store = state.store.write().await;
        if store.get_deployment(&name).is_none() {
            return json_error(
                axum::http::StatusCode::NOT_FOUND,
                format!("Deployment '{}' not found", name),
            );
        }
        store.delete_deployment(&name);
    }

    state.controller.terminate_deployment(&name).await;

    tracing::info!("Deployment {} deleted", name);

    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({
            "message": format!("Deployment '{}' deleted", name)
        })),
    )
}

async fn list_pods(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    let pods: Vec<crate::models::PodResponse> = store
        .list_pods()
        .iter()
        .map(|p| crate::models::PodResponse::from(*p))
        .collect();

    axum::Json(pods)
}

async fn get_pod(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let pod_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return json_error(axum::http::StatusCode::BAD_REQUEST, "Invalid pod ID format");
        }
    };

    let store = state.store.read().await;

    match store.get_pod(&pod_id) {
        Some(pod) => {
            let response = crate::models::PodResponse::from(pod);
            (
                axum::http::StatusCode::OK,
                axum::Json(serde_json::to_value(response).unwrap()),
            )
        }
        None => json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Pod '{}' not found", id),
        ),
    }
}

async fn delete_pod(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let pod_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return json_error(axum::http::StatusCode::BAD_REQUEST, "Invalid pod ID format");
        }
    };

    let pod_name = {
        let store = state.store.read().await;
        match store.get_pod(&pod_id) {
            Some(pod) => pod.name.clone(),
            None => {
                return json_error(
                    axum::http::StatusCode::NOT_FOUND,
                    format!("Pod '{}' not found", id),
                );
            }
        }
    };

    tracing::info!("Deleting pod: {} ({})", pod_name, pod_id);

    {
        let mut store = state.store.write().await;
        store.update_pod_status(&pod_id, crate::models::PodStatus::Terminating);
    }

    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({
            "message": format!("Pod '{}' is being terminated", pod_name)
        })),
    )
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
}
