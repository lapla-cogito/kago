pub(super) struct Scheduler<'a> {
    controller: &'a crate::controller::Controller,
}

impl<'a> Scheduler<'a> {
    pub fn new(controller: &'a crate::controller::Controller) -> Self {
        Self { controller }
    }

    pub async fn schedule_pending_pods(&self) {
        let unassigned_pods: Vec<crate::models::Pod> = {
            let store = self.controller.store.read().await;
            store
                .get_unassigned_pods()
                .into_iter()
                .filter(|p| p.status == crate::models::PodStatus::Pending)
                .collect()
        };

        let mut node_cache = self.build_node_cache().await;

        for pod in unassigned_pods {
            let pod_id = pod.id;
            let name = pod.name.clone();
            let image = pod.image.clone();
            let resources = pod.resources;

            if let Some((node_name, node_endpoint)) =
                Self::find_node_for_pod(&mut node_cache, &resources)
            {
                tracing::info!("Scheduling pod {} on node {}", name, node_name);

                {
                    let mut store = self.controller.store.write().await;
                    store.assign_pod_to_node(&pod_id, &node_name);
                    store.allocate_resources_on_node(&node_name, &resources);
                    store.update_pod_status(&pod_id, crate::models::PodStatus::Creating);
                }

                let request = crate::models::CreatePodOnNodeRequest {
                    pod_id,
                    name: name.clone(),
                    image: image.clone(),
                    resources,
                };

                let url = format!("{}/pods", node_endpoint);

                match self
                    .controller
                    .http_client
                    .post(&url)
                    .json(&request)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() {
                            tracing::info!("Pod {} created on node {}", name, node_name);
                            let mut store = self.controller.store.write().await;
                            store.update_pod_status(&pod_id, crate::models::PodStatus::Running);
                        } else {
                            let error = response.text().await.unwrap_or_default();
                            tracing::error!(
                                "Failed to create pod {} on node {}: {}",
                                name,
                                node_name,
                                error
                            );
                            self.mark_pod_failed(&pod_id, &node_name, &resources).await;
                            Self::release_node_reservation(&mut node_cache, &node_name, &resources);
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to communicate with node {} for pod {}: {}",
                            node_name,
                            name,
                            e
                        );
                        self.mark_pod_failed(&pod_id, &node_name, &resources).await;
                        Self::release_node_reservation(&mut node_cache, &node_name, &resources);
                    }
                }
            } else {
                tracing::warn!(
                    "No suitable node found for pod {} (requires {}m CPU, {}Mi memory)",
                    name,
                    resources.cpu_millis,
                    resources.memory_mb
                );
            }
        }
    }

    fn find_node_for_pod(
        node_cache: &mut [NodeCacheEntry],
        resources: &crate::models::Resources,
    ) -> Option<(String, String)> {
        for entry in node_cache.iter_mut() {
            if entry.can_fit(resources) {
                entry.reserve(resources);
                return Some((entry.name.clone(), entry.endpoint.clone()));
            }
        }
        None
    }

    async fn build_node_cache(&self) -> Vec<NodeCacheEntry> {
        let store = self.controller.store.read().await;
        store
            .get_ready_nodes()
            .into_iter()
            .map(|node| NodeCacheEntry {
                name: node.name.clone(),
                endpoint: node.endpoint(),
                available: node.available_resources(),
            })
            .collect()
    }

    fn release_node_reservation(
        node_cache: &mut [NodeCacheEntry],
        node_name: &str,
        resources: &crate::models::Resources,
    ) {
        if let Some(entry) = node_cache.iter_mut().find(|entry| entry.name == node_name) {
            entry.release(resources);
        }
    }

    async fn mark_pod_failed(
        &self,
        pod_id: &uuid::Uuid,
        node_name: &str,
        resources: &crate::models::Resources,
    ) {
        let mut store = self.controller.store.write().await;
        store.update_pod_status(pod_id, crate::models::PodStatus::Failed);
        store.deallocate_resources_on_node(node_name, resources);
    }
}

struct NodeCacheEntry {
    name: String,
    endpoint: String,
    available: crate::models::Resources,
}

impl NodeCacheEntry {
    fn can_fit(&self, request: &crate::models::Resources) -> bool {
        self.available.fits(request)
    }

    fn reserve(&mut self, request: &crate::models::Resources) {
        self.available.cpu_millis = self.available.cpu_millis.saturating_sub(request.cpu_millis);
        self.available.memory_mb = self.available.memory_mb.saturating_sub(request.memory_mb);
    }

    fn release(&mut self, request: &crate::models::Resources) {
        self.available.cpu_millis = self.available.cpu_millis.saturating_add(request.cpu_millis);
        self.available.memory_mb = self.available.memory_mb.saturating_add(request.memory_mb);
    }
}
