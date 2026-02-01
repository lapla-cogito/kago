/// Tracks the state of pods managed by this agent
#[derive(Debug, Clone)]
pub struct ManagedPod {
    pub pod_id: uuid::Uuid,
    pub name: String,
    pub resources: crate::models::Resources,
    pub container_id: Option<String>,
    pub status: crate::models::PodStatus,
}

/// Agent state shared across handlers
pub struct AgentState {
    pub node_name: String,
    pub master_url: String,
    pub runtime: std::sync::Arc<crate::runtime::ContainerRuntime>,
    pub pods: tokio::sync::RwLock<std::collections::HashMap<uuid::Uuid, ManagedPod>>,
    pub capacity: crate::models::Resources,
}

impl AgentState {
    pub fn new(
        node_name: String,
        master_url: String,
        runtime: std::sync::Arc<crate::runtime::ContainerRuntime>,
        capacity: crate::models::Resources,
    ) -> Self {
        Self {
            node_name,
            master_url,
            runtime,
            pods: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            capacity,
        }
    }

    pub async fn calculate_used_resources(&self) -> crate::models::Resources {
        let pods = self.pods.read().await;
        let mut used = crate::models::Resources::default();
        for pod in pods.values() {
            if matches!(
                pod.status,
                crate::models::PodStatus::Running | crate::models::PodStatus::Creating
            ) {
                used.cpu_millis += pod.resources.cpu_millis;
                used.memory_mb += pod.resources.memory_mb;
            }
        }
        used
    }

    pub async fn get_pod_statuses(&self) -> Vec<crate::models::PodStatusReport> {
        let pods = self.pods.read().await;
        pods.values()
            .map(|p| crate::models::PodStatusReport {
                pod_id: p.pod_id,
                status: p.status,
                container_id: p.container_id.clone(),
            })
            .collect()
    }
}

/// Agent that runs on worker nodes
pub struct Agent {
    state: std::sync::Arc<AgentState>,
    port: u16,
    heartbeat_interval: std::time::Duration,
}

impl Agent {
    pub fn new(
        node_name: String,
        master_url: String,
        runtime: std::sync::Arc<crate::runtime::ContainerRuntime>,
        port: u16,
        capacity: crate::models::Resources,
    ) -> Self {
        let state = std::sync::Arc::new(AgentState::new(node_name, master_url, runtime, capacity));
        Self {
            state,
            port,
            heartbeat_interval: std::time::Duration::from_secs(5),
        }
    }

    pub fn state(&self) -> std::sync::Arc<AgentState> {
        std::sync::Arc::clone(&self.state)
    }

    /// Register this node with the master
    pub async fn register(&self, address: &str) -> crate::error::AgentResult<()> {
        let client = reqwest::Client::new();
        let url = format!("{}/nodes/register", self.state.master_url);

        let request = crate::models::RegisterNodeRequest {
            name: self.state.node_name.clone(),
            address: address.to_string(),
            port: self.port,
            capacity: self.state.capacity,
        };

        tracing::info!(
            "Registering node '{}' with master at {}",
            self.state.node_name,
            self.state.master_url
        );

        let response = client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| crate::error::AgentError::RegistrationFailed(e.to_string()))?;

        if response.status().is_success() {
            tracing::info!("Node '{}' registered successfully", self.state.node_name);
            Ok(())
        } else {
            let error = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(crate::error::AgentError::RegistrationFailed(error))
        }
    }

    /// Start the heartbeat loop
    pub async fn run_heartbeat_loop(&self) {
        let mut interval = tokio::time::interval(self.heartbeat_interval);
        let client = reqwest::Client::new();
        let url = format!(
            "{}/nodes/{}/heartbeat",
            self.state.master_url, self.state.node_name
        );

        loop {
            interval.tick().await;

            // Sync container states before sending heartbeat
            self.sync_pod_statuses().await;

            let used = self.state.calculate_used_resources().await;
            let pod_statuses = self.state.get_pod_statuses().await;

            let heartbeat = crate::models::HeartbeatRequest { used, pod_statuses };

            match client.post(&url).json(&heartbeat).send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        tracing::warn!(
                            "Heartbeat failed: {}",
                            response.text().await.unwrap_or_default()
                        );
                    } else {
                        tracing::debug!("Heartbeat sent successfully");
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to send heartbeat: {}", e);
                }
            }
        }
    }

    /// Sync pod statuses from container runtime
    async fn sync_pod_statuses(&self) {
        let pod_names: Vec<(uuid::Uuid, String)> = {
            let pods = self.state.pods.read().await;
            pods.values()
                .filter(|p| {
                    matches!(
                        p.status,
                        crate::models::PodStatus::Running | crate::models::PodStatus::Creating
                    )
                })
                .map(|p| (p.pod_id, p.name.clone()))
                .collect()
        };

        for (pod_id, name) in pod_names {
            match self.state.runtime.get_container_state(&name).await {
                Ok(status) => {
                    let new_status = match status {
                        crate::runtime::ContainerStatus::Running => {
                            crate::models::PodStatus::Running
                        }
                        crate::runtime::ContainerStatus::Exited
                        | crate::runtime::ContainerStatus::Dead => crate::models::PodStatus::Failed,
                        crate::runtime::ContainerStatus::Created => {
                            crate::models::PodStatus::Creating
                        }
                        _ => continue,
                    };

                    let mut pods = self.state.pods.write().await;
                    if let Some(pod) = pods.get_mut(&pod_id)
                        && pod.status != crate::models::PodStatus::Terminating
                    {
                        pod.status = new_status;
                    }
                }
                Err(crate::error::RuntimeError::ContainerNotFound(_)) => {
                    let mut pods = self.state.pods.write().await;
                    if let Some(pod) = pods.get_mut(&pod_id)
                        && !matches!(
                            pod.status,
                            crate::models::PodStatus::Terminating
                                | crate::models::PodStatus::Terminated
                        )
                    {
                        pod.status = crate::models::PodStatus::Failed;
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to get container state for {}: {}", name, e);
                }
            }
        }
    }

    /// Create the agent API router
    pub fn create_router(state: std::sync::Arc<AgentState>) -> axum::Router {
        axum::Router::new()
            .route("/health", axum::routing::get(health_check))
            .route("/pods", axum::routing::post(create_pod))
            .route("/pods", axum::routing::get(list_pods))
            .route("/pods/{name}", axum::routing::delete(delete_pod))
            .with_state(state)
    }
}

async fn health_check() -> impl axum::response::IntoResponse {
    axum::Json(serde_json::json!({
        "status": "healthy"
    }))
}

async fn create_pod(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AgentState>>,
    axum::Json(req): axum::Json<crate::models::CreatePodOnNodeRequest>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Creating pod: {} ({})", req.name, req.pod_id);

    // Check if pod already exists
    {
        let pods = state.pods.read().await;
        if pods.contains_key(&req.pod_id) {
            return (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({
                    "error": format!("Pod {} already exists", req.name)
                })),
            );
        }
    }

    // Add pod to state as creating
    let managed_pod = ManagedPod {
        pod_id: req.pod_id,
        name: req.name.clone(),
        resources: req.resources,
        container_id: None,
        status: crate::models::PodStatus::Creating,
    };

    {
        let mut pods = state.pods.write().await;
        pods.insert(req.pod_id, managed_pod);
    }

    // Start container
    let cpu = if req.resources.cpu_millis > 0 {
        Some(req.resources.cpu_millis)
    } else {
        None
    };
    let mem = if req.resources.memory_mb > 0 {
        Some(req.resources.memory_mb)
    } else {
        None
    };

    match state
        .runtime
        .run_container(&req.name, &req.image, cpu, mem)
        .await
    {
        Ok(container_id) => {
            let mut pods = state.pods.write().await;
            if let Some(pod) = pods.get_mut(&req.pod_id) {
                pod.container_id = Some(container_id.clone());
                pod.status = crate::models::PodStatus::Running;
            }

            tracing::info!("Pod {} started with container {}", req.name, container_id);

            (
                axum::http::StatusCode::CREATED,
                axum::Json(serde_json::json!({
                    "pod_id": req.pod_id,
                    "name": req.name,
                    "container_id": container_id,
                    "status": "running"
                })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to create container for pod {}: {}", req.name, e);

            let mut pods = state.pods.write().await;
            if let Some(pod) = pods.get_mut(&req.pod_id) {
                pod.status = crate::models::PodStatus::Failed;
            }

            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": format!("Failed to create container: {}", e)
                })),
            )
        }
    }
}

async fn list_pods(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AgentState>>,
) -> impl axum::response::IntoResponse {
    let pods = state.pods.read().await;
    let pod_list: Vec<crate::models::AgentPodStatus> = pods
        .values()
        .map(|p| crate::models::AgentPodStatus {
            pod_id: p.pod_id,
            name: p.name.clone(),
            status: p.status,
            container_id: p.container_id.clone(),
        })
        .collect();

    axum::Json(pod_list)
}

async fn delete_pod(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<AgentState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    tracing::info!("Deleting pod: {}", name);

    // Find the pod by name
    let pod_info = {
        let pods = state.pods.read().await;
        pods.values()
            .find(|p| p.name == name)
            .map(|p| (p.pod_id, p.container_id.clone()))
    };

    let Some((pod_id, container_id)) = pod_info else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": format!("Pod '{}' not found", name)
            })),
        );
    };

    // Mark as terminating
    {
        let mut pods = state.pods.write().await;
        if let Some(pod) = pods.get_mut(&pod_id) {
            pod.status = crate::models::PodStatus::Terminating;
        }
    }

    // Stop and remove container
    if let Some(container_id) = container_id {
        if let Err(e) = state.runtime.stop_container(&container_id).await {
            match e {
                crate::error::RuntimeError::ContainerNotFound(_) => {}
                _ => {
                    tracing::warn!("Failed to stop container {}: {}", name, e);
                }
            }
        }

        if let Err(e) = state.runtime.remove_container(&container_id).await {
            tracing::warn!("Failed to remove container {}: {}", name, e);
        }
    }

    // Also try to remove by name
    let _ = state.runtime.remove_container(&name).await;

    // Remove from state
    {
        let mut pods = state.pods.write().await;
        pods.remove(&pod_id);
    }

    tracing::info!("Pod {} deleted", name);

    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({
            "message": format!("Pod '{}' deleted", name)
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_managed_pod_creation() {
        let pod = ManagedPod {
            pod_id: uuid::Uuid::new_v4(),
            name: "test-pod".to_string(),
            resources: crate::models::Resources {
                cpu_millis: 100,
                memory_mb: 128,
            },
            container_id: None,
            status: crate::models::PodStatus::Pending,
        };

        assert_eq!(pod.name, "test-pod");
        assert_eq!(pod.status, crate::models::PodStatus::Pending);
    }

    #[test]
    fn test_resources_calculation() {
        let r1 = crate::models::Resources {
            cpu_millis: 100,
            memory_mb: 256,
        };
        let r2 = crate::models::Resources {
            cpu_millis: 200,
            memory_mb: 512,
        };

        assert!(r2.fits(&r1));
        assert!(!r1.fits(&r2));

        let diff = r2.subtract(&r1);
        assert_eq!(diff.cpu_millis, 100);
        assert_eq!(diff.memory_mb, 256);
    }
}
