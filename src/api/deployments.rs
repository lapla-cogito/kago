pub(super) async fn list_deployments(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    let deployments: Vec<crate::models::DeploymentResponse> = store
        .list_deployments()
        .into_iter()
        .map(|d| {
            let ready = store.count_running_pods_for_deployment(&d.name);
            crate::models::DeploymentResponse::from_deployment(&d, ready)
        })
        .collect();

    axum::Json(deployments)
}

pub(super) async fn create_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::Json(req): axum::Json<crate::models::CreateDeploymentRequest>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Creating deployment: {}", req.name);

    if req.name.is_empty() {
        return crate::api::json_error(
            axum::http::StatusCode::BAD_REQUEST,
            "Deployment name cannot be empty",
        );
    }

    if req.image.is_empty() {
        return crate::api::json_error(
            axum::http::StatusCode::BAD_REQUEST,
            "Image cannot be empty",
        );
    }

    {
        let store = state.store.read().await;
        if store.get_deployment(&req.name).is_some() {
            return crate::api::json_error(
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

pub(super) async fn get_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
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
        None => crate::api::json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Deployment '{}' not found", name),
        ),
    }
}

pub(super) async fn update_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
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
        None => crate::api::json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Deployment '{}' not found", name),
        ),
    }
}

pub(super) async fn delete_deployment(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Deleting deployment: {}", name);

    {
        let mut store = state.store.write().await;
        if store.get_deployment(&name).is_none() {
            return crate::api::json_error(
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
