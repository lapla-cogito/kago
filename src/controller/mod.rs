mod scheduler;

pub struct Controller {
    store: crate::store::SharedStore,
    reconcile_interval: std::time::Duration,
    node_timeout: std::time::Duration,
    http_client: reqwest::Client,
}

impl Controller {
    pub fn new(store: crate::store::SharedStore) -> Self {
        Self {
            store,
            reconcile_interval: std::time::Duration::from_secs(5),
            node_timeout: std::time::Duration::from_secs(30),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }

    pub async fn run(&self) {
        tracing::info!(
            "Starting controller with reconcile interval: {:?}",
            self.reconcile_interval
        );

        let mut ticker = tokio::time::interval(self.reconcile_interval);

        loop {
            ticker.tick().await;
            self.reconcile_all().await;
        }
    }

    pub async fn reconcile_all(&self) {
        tracing::debug!("Starting reconciliation cycle");

        self.check_node_health().await;

        let deployments = {
            let store = self.store.read().await;
            store.list_deployments()
        };

        for deployment in deployments {
            if let Err(e) = self.reconcile_deployment(&deployment).await {
                tracing::error!("Failed to reconcile deployment {}: {}", deployment.name, e);
            }
        }

        scheduler::Scheduler::new(self)
            .schedule_pending_pods()
            .await;
        self.cleanup_terminated_pods().await;

        tracing::debug!("Reconciliation cycle complete");
    }

    async fn check_node_health(&self) {
        let nodes = {
            let store = self.store.read().await;
            store.list_nodes()
        };

        let now = chrono::Utc::now();

        for node in nodes {
            let elapsed = now.signed_duration_since(node.last_heartbeat);
            if elapsed > chrono::Duration::from_std(self.node_timeout).unwrap_or_default() {
                tracing::warn!(
                    "Node '{}' has not sent heartbeat for {:?}, marking as NotReady",
                    node.name,
                    elapsed
                );
                let mut store = self.store.write().await;
                store.update_node_status(&node.name, crate::models::NodeStatus::NotReady);
            }
        }
    }

    async fn reconcile_deployment(
        &self,
        deployment: &crate::models::Deployment,
    ) -> Result<(), String> {
        tracing::debug!(
            "Reconciling deployment: {} (replicas: {})",
            deployment.name,
            deployment.replicas
        );

        let (current_count, deployment_exists) = {
            let store = self.store.read().await;
            let exists = store.get_deployment(&deployment.name).is_some();
            let count = store.count_active_pods_for_deployment(&deployment.name);
            (count, exists)
        };

        if !deployment_exists {
            tracing::debug!(
                "Deployment {} no longer exists, skipping reconciliation",
                deployment.name
            );
            return Ok(());
        }

        let desired_count = deployment.replicas;

        tracing::debug!(
            "Deployment {}: current={}, desired={}",
            deployment.name,
            current_count,
            desired_count
        );

        if current_count < desired_count {
            let to_create = desired_count - current_count;
            tracing::info!(
                "Scaling up deployment {}: creating {} pods",
                deployment.name,
                to_create
            );

            for i in 0..to_create {
                let pod = self
                    .create_pod_for_deployment(deployment, current_count + i)
                    .await;
                let mut store = self.store.write().await;
                store.add_pod(pod);
            }
        } else if current_count > desired_count {
            let to_terminate = current_count - desired_count;
            tracing::info!(
                "Scaling down deployment {}: terminating {} pods",
                deployment.name,
                to_terminate
            );

            let pod_ids = {
                let store = self.store.read().await;
                store.get_pods_to_terminate(&deployment.name, to_terminate)
            };

            for pod_id in pod_ids {
                self.terminate_pod(pod_id).await;
            }
        }

        Ok(())
    }

    async fn create_pod_for_deployment(
        &self,
        deployment: &crate::models::Deployment,
        index: u32,
    ) -> crate::models::Pod {
        let mut final_index = index;
        let existing_names: std::collections::HashSet<String> = {
            let store = self.store.read().await;
            store
                .list_pods_for_deployment(&deployment.name)
                .into_iter()
                .filter(|p| {
                    !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated | crate::models::PodStatus::Failed
                    )
                })
                .map(|p| p.name)
                .collect()
        };

        while existing_names.contains(&format!("{}-{}", deployment.name, final_index)) {
            final_index += 1;
        }

        crate::models::Pod::from_deployment(deployment, final_index)
    }

    async fn terminate_pod(&self, pod_id: uuid::Uuid) {
        let (name, node_name, resources) = {
            let store = self.store.read().await;
            match store.get_pod(&pod_id) {
                Some(pod) => (pod.name.clone(), pod.node_name.clone(), pod.resources),
                None => return,
            }
        };

        tracing::info!("Terminating pod: {}", name);

        {
            let mut store = self.store.write().await;
            store.update_pod_status(&pod_id, crate::models::PodStatus::Terminating);
        }

        if let Some(ref node_name) = node_name {
            let node_endpoint = {
                let store = self.store.read().await;
                store.get_node(node_name).map(|n| n.endpoint())
            };

            if let Some(endpoint) = node_endpoint {
                let url = format!("{}/pods/{}", endpoint, name);

                match self.http_client.delete(&url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            tracing::info!("Pod {} deleted from node {}", name, node_name);
                        } else {
                            tracing::warn!(
                                "Failed to delete pod {} from node {}: {}",
                                name,
                                node_name,
                                response.text().await.unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to communicate with node {} to delete pod {}: {}",
                            node_name,
                            name,
                            e
                        );
                    }
                }

                let mut store = self.store.write().await;
                store.deallocate_resources_on_node(node_name, &resources);
            }
        }

        {
            let mut store = self.store.write().await;
            store.update_pod_status(&pod_id, crate::models::PodStatus::Terminated);
        }

        tracing::info!("Pod {} terminated", name);
    }

    async fn cleanup_terminated_pods(&self) {
        let terminated_pods: Vec<uuid::Uuid> = {
            let store = self.store.read().await;
            store
                .list_pods()
                .into_iter()
                .filter(|p| matches!(p.status, crate::models::PodStatus::Terminated))
                .map(|p| p.id)
                .collect()
        };

        if !terminated_pods.is_empty() {
            let mut store = self.store.write().await;
            for pod_id in terminated_pods {
                store.delete_pod(&pod_id);
            }
        }
    }

    pub async fn terminate_deployment(&self, deployment_name: &str) {
        tracing::info!("Terminating all pods for deployment: {}", deployment_name);

        let pod_ids: Vec<uuid::Uuid> = {
            let store = self.store.read().await;
            store
                .list_pods_for_deployment(deployment_name)
                .into_iter()
                .filter(|p| {
                    !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated
                            | crate::models::PodStatus::Terminating
                    )
                })
                .map(|p| p.id)
                .collect()
        };

        for pod_id in pod_ids {
            self.terminate_pod(pod_id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_controller_creation() {
        let store = crate::store::new_shared_store();

        {
            let mut s = store.write().await;
            let deployment = crate::models::Deployment {
                name: "test".to_string(),
                image: "nginx:latest".to_string(),
                replicas: 2,
                resources: crate::models::Resources {
                    cpu_millis: 100,
                    memory_mb: 128,
                },
            };
            s.upsert_deployment(deployment);
        }

        {
            let s = store.read().await;
            assert!(s.get_deployment("test").is_some());
        }
    }

    #[tokio::test]
    async fn test_node_registration() {
        let store = crate::store::new_shared_store();

        {
            let s = store.read().await;
            assert!(s.list_nodes().is_empty());
        }

        {
            let mut s = store.write().await;
            let node = crate::models::Node::new(
                "worker-1".to_string(),
                "localhost".to_string(),
                8081,
                crate::models::Resources {
                    cpu_millis: 4000,
                    memory_mb: 8192,
                },
            );
            s.register_node(node);
        }

        {
            let s = store.read().await;
            assert_eq!(s.list_nodes().len(), 1);
            assert!(s.get_node("worker-1").is_some());
        }
    }
}
