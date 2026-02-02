pub async fn metrics_handler(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::api::AppState>>,
) -> axum::response::Response {
    crate::metrics::update_metrics(&state.store).await;
    let metrics = crate::metrics::encode_metrics();

    axum::response::IntoResponse::into_response((
        axum::http::StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        metrics,
    ))
}
