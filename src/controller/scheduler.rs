#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SchedulingStrategy {
    /// Select the first node that fits
    #[default]
    FirstFit,
    /// Select the node with the least remaining resources after scheduling (bin packing)
    /// This maximizes cluster utilization by packing pods tightly
    BestFit,
    /// Select the node with the most remaining resources after scheduling (load balancing)
    LeastAllocated,
    /// Balanced strategy: considers both CPU and memory utilization equally
    Balanced,
}

pub(super) struct Scheduler<'a> {
    controller: &'a crate::controller::Controller,
    strategy: SchedulingStrategy,
}

impl<'a> Scheduler<'a> {
    pub fn new(controller: &'a crate::controller::Controller) -> Self {
        Self {
            controller,
            strategy: SchedulingStrategy::default(),
        }
    }

    pub fn with_strategy(mut self, strategy: SchedulingStrategy) -> Self {
        self.strategy = strategy;
        self
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

            let mut best_choice: Option<(usize, i64)> = None;

            for (idx, node) in node_cache.iter().enumerate() {
                if !self.node_passes_filters(node, &resources) {
                    continue;
                }
                let score = self.calculate_node_score(node, &resources);
                match best_choice {
                    Some((_, best_score)) if best_score >= score => {}
                    _ => best_choice = Some((idx, score)),
                }
            }

            let Some((selected_idx, best_score)) = best_choice else {
                tracing::warn!(
                    "No suitable node found for pod {} (requires {}m CPU, {}Mi memory)",
                    name,
                    resources.cpu_millis,
                    resources.memory_mb
                );

                continue;
            };

            let selected_node = &mut node_cache[selected_idx];
            let node_name = selected_node.name.clone();
            let node_endpoint = selected_node.endpoint.clone();

            tracing::info!(
                "Scheduling pod {} on node {} (strategy: {:?}, score: {})",
                name,
                node_name,
                self.strategy,
                best_score
            );

            selected_node.reserve(&resources);

            self.bind_pod_to_node(
                pod_id,
                &name,
                &image,
                &resources,
                &node_name,
                &node_endpoint,
                &mut node_cache,
            )
            .await;
        }
    }

    fn node_passes_filters(
        &self,
        node: &NodeCacheEntry,
        resources: &crate::models::Resources,
    ) -> bool {
        if !node.can_fit(resources) {
            return false;
        }

        // TODO: Add more filters

        true
    }

    fn calculate_node_score(
        &self,
        node: &NodeCacheEntry,
        resources: &crate::models::Resources,
    ) -> i64 {
        match self.strategy {
            SchedulingStrategy::FirstFit => 0,
            SchedulingStrategy::BestFit => self.score_best_fit(node, resources),
            SchedulingStrategy::LeastAllocated => self.score_least_allocated(node, resources),
            SchedulingStrategy::Balanced => self.score_balanced(node, resources),
        }
    }

    fn score_best_fit(&self, node: &NodeCacheEntry, resources: &crate::models::Resources) -> i64 {
        let remaining_cpu = node
            .available
            .cpu_millis
            .saturating_sub(resources.cpu_millis);
        let remaining_mem = node.available.memory_mb.saturating_sub(resources.memory_mb);

        // Normalize
        let cpu_remaining_pct = if node.capacity.cpu_millis > 0 {
            (remaining_cpu as f64 / node.capacity.cpu_millis as f64) * 100.0
        } else {
            0.0
        };
        let mem_remaining_pct = if node.capacity.memory_mb > 0 {
            (remaining_mem as f64 / node.capacity.memory_mb as f64) * 100.0
        } else {
            0.0
        };

        let score = 200.0 - (cpu_remaining_pct + mem_remaining_pct);

        score as i64
    }

    fn score_least_allocated(
        &self,
        node: &NodeCacheEntry,
        resources: &crate::models::Resources,
    ) -> i64 {
        let remaining_cpu = node
            .available
            .cpu_millis
            .saturating_sub(resources.cpu_millis);
        let remaining_mem = node.available.memory_mb.saturating_sub(resources.memory_mb);

        // Normalize
        let cpu_remaining_pct = if node.capacity.cpu_millis > 0 {
            (remaining_cpu as f64 / node.capacity.cpu_millis as f64) * 100.0
        } else {
            0.0
        };
        let mem_remaining_pct = if node.capacity.memory_mb > 0 {
            (remaining_mem as f64 / node.capacity.memory_mb as f64) * 100.0
        } else {
            0.0
        };

        (cpu_remaining_pct + mem_remaining_pct) as i64
    }

    fn score_balanced(&self, node: &NodeCacheEntry, resources: &crate::models::Resources) -> i64 {
        let remaining_cpu = node
            .available
            .cpu_millis
            .saturating_sub(resources.cpu_millis);
        let remaining_mem = node.available.memory_mb.saturating_sub(resources.memory_mb);

        let cpu_remaining_pct = if node.capacity.cpu_millis > 0 {
            (remaining_cpu as f64 / node.capacity.cpu_millis as f64) * 100.0
        } else {
            0.0
        };
        let mem_remaining_pct = if node.capacity.memory_mb > 0 {
            (remaining_mem as f64 / node.capacity.memory_mb as f64) * 100.0
        } else {
            0.0
        };

        let availability_score = cpu_remaining_pct + mem_remaining_pct;
        let balance_penalty = (cpu_remaining_pct - mem_remaining_pct).abs();

        (availability_score - balance_penalty * 0.3) as i64
    }

    /// Assign the pod to the selected node and send create request
    #[allow(clippy::too_many_arguments)]
    async fn bind_pod_to_node(
        &self,
        pod_id: uuid::Uuid,
        name: &str,
        image: &str,
        resources: &crate::models::Resources,
        node_name: &str,
        node_endpoint: &str,
        node_cache: &mut [NodeCacheEntry],
    ) {
        {
            let mut store = self.controller.store.write().await;
            store.assign_pod_to_node(&pod_id, node_name);
            store.allocate_resources_on_node(node_name, resources);
            store.update_pod_status(&pod_id, crate::models::PodStatus::Creating);
        }

        let request = crate::models::CreatePodOnNodeRequest {
            pod_id,
            name: name.to_string(),
            image: image.to_string(),
            resources: *resources,
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
                    self.mark_pod_failed(&pod_id, node_name, resources).await;
                    Self::release_node_reservation(node_cache, node_name, resources);
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to communicate with node {} for pod {}: {}",
                    node_name,
                    name,
                    e
                );
                self.mark_pod_failed(&pod_id, node_name, resources).await;
                Self::release_node_reservation(node_cache, node_name, resources);
            }
        }
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
                capacity: node.capacity,
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
    capacity: crate::models::Resources,
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
