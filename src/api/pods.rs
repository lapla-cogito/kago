use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;

use super::{AppState, json_error};

pub(super) async fn list_pods(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let store = state.store.read().await;

    let pods: Vec<crate::models::PodResponse> = store
        .list_pods()
        .into_iter()
        .map(|pod| crate::models::PodResponse::from(&pod))
        .collect();

    Json(pods)
}

pub(super) async fn get_pod(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pod_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return json_error(StatusCode::BAD_REQUEST, "Invalid pod ID format");
        }
    };

    let store = state.store.read().await;

    match store.get_pod(&pod_id) {
        Some(pod) => {
            let response = crate::models::PodResponse::from(pod);
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
        }
        None => json_error(StatusCode::NOT_FOUND, format!("Pod '{}' not found", id)),
    }
}

pub(super) async fn delete_pod(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pod_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return json_error(StatusCode::BAD_REQUEST, "Invalid pod ID format");
        }
    };

    let pod_name = {
        let store = state.store.read().await;
        match store.get_pod(&pod_id) {
            Some(pod) => pod.name.clone(),
            None => {
                return json_error(StatusCode::NOT_FOUND, format!("Pod '{}' not found", id));
            }
        }
    };

    tracing::info!("Deleting pod: {} ({})", pod_name, pod_id);

    {
        let mut store = state.store.write().await;
        store.update_pod_status(&pod_id, crate::models::PodStatus::Terminating);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": format!("Pod '{}' is being terminated", pod_name)
        })),
    )
}
