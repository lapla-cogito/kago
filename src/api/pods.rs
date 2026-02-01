pub(super) async fn list_pods(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
) -> impl axum::response::IntoResponse {
    let store = state.store.read().await;

    let pods: Vec<crate::models::PodResponse> = store
        .list_pods()
        .into_iter()
        .map(|pod| crate::models::PodResponse::from(&pod))
        .collect();

    axum::Json(pods)
}

pub(super) async fn get_pod(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let pod_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return crate::api::json_error(
                axum::http::StatusCode::BAD_REQUEST,
                "Invalid pod ID format",
            );
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
        None => crate::api::json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("Pod '{}' not found", id),
        ),
    }
}

pub(super) async fn delete_pod(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    let pod_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return crate::api::json_error(
                axum::http::StatusCode::BAD_REQUEST,
                "Invalid pod ID format",
            );
        }
    };

    let pod_name = {
        let store = state.store.read().await;
        match store.get_pod(&pod_id) {
            Some(pod) => pod.name.clone(),
            None => {
                return crate::api::json_error(
                    axum::http::StatusCode::NOT_FOUND,
                    format!("Pod '{}' not found", id),
                );
            }
        }
    };

    tracing::info!("Deleting pod: {} ({})", pod_name, pod_id);

    state.controller.terminate_pod(pod_id).await;

    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({
            "message": format!("Pod '{}' is being terminated", pod_name)
        })),
    )
}
