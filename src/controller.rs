pub struct Controller {
    store: crate::store::SharedStore,
    runtime: std::sync::Arc<crate::runtime::ContainerRuntime>,
    reconcile_interval: std::time::Duration,
}

impl Controller {
    pub fn new(
        store: crate::store::SharedStore,
        runtime: std::sync::Arc<crate::runtime::ContainerRuntime>,
    ) -> Self {
        Self {
            store,
            runtime,
            reconcile_interval: std::time::Duration::from_secs(5),
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

        let deployments: Vec<crate::models::Deployment> = {
            let store = self.store.read().await;
            store.list_deployments().into_iter().cloned().collect()
        };

        for deployment in deployments {
            if let Err(e) = self.reconcile_deployment(&deployment).await {
                tracing::error!("Failed to reconcile deployment {}: {}", deployment.name, e);
            }
        }

        self.process_pending_pods().await;
        self.sync_pod_statuses().await;
        self.cleanup_terminated_pods().await;

        tracing::debug!("Reconciliation cycle complete");
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
        let store = self.store.read().await;

        loop {
            let name = format!("{}-{}", deployment.name, final_index);
            let exists = store
                .list_pods_for_deployment(&deployment.name)
                .iter()
                .any(|p| {
                    p.name == name
                        && !matches!(
                            p.status,
                            crate::models::PodStatus::Terminated | crate::models::PodStatus::Failed
                        )
                });

            if !exists {
                break;
            }
            final_index += 1;
        }
        drop(store);

        crate::models::Pod::from_deployment(deployment, final_index)
    }

    async fn process_pending_pods(&self) {
        let pending_pods: Vec<(uuid::Uuid, String, String, u32, u32)> = {
            let store = self.store.read().await;
            store
                .get_pending_pods()
                .iter()
                .map(|p| {
                    (
                        p.id,
                        p.name.clone(),
                        p.image.clone(),
                        p.resources.cpu_millis,
                        p.resources.memory_mb,
                    )
                })
                .collect()
        };

        for (pod_id, name, image, cpu_millis, memory_mb) in pending_pods {
            tracing::info!("Starting container for pod: {}", name);

            {
                let mut store = self.store.write().await;
                store.update_pod_status(&pod_id, crate::models::PodStatus::Creating);
            }

            let cpu = if cpu_millis > 0 {
                Some(cpu_millis)
            } else {
                None
            };
            let mem = if memory_mb > 0 { Some(memory_mb) } else { None };

            match self.runtime.run_container(&name, &image, cpu, mem).await {
                Ok(container_id) => {
                    let mut store = self.store.write().await;
                    store.update_pod_container_id(&pod_id, container_id);
                    store.update_pod_status(&pod_id, crate::models::PodStatus::Running);
                    tracing::info!("Pod {} is now running", name);
                }
                Err(e) => {
                    tracing::error!("Failed to start container for pod {}: {}", name, e);
                    let mut store = self.store.write().await;
                    store.update_pod_status(&pod_id, crate::models::PodStatus::Failed);
                }
            }
        }
    }

    async fn terminate_pod(&self, pod_id: uuid::Uuid) {
        let (name, container_id) = {
            let store = self.store.read().await;
            match store.get_pod(&pod_id) {
                Some(pod) => (pod.name.clone(), pod.container_id.clone()),
                None => return,
            }
        };

        tracing::info!("Terminating pod: {}", name);

        {
            let mut store = self.store.write().await;
            store.update_pod_status(&pod_id, crate::models::PodStatus::Terminating);
        }

        if let Some(container_id) = container_id {
            if let Err(e) = self.runtime.stop_container(&container_id).await {
                match e {
                    crate::runtime::RuntimeError::ContainerNotFound(_) => {}
                    _ => {
                        tracing::warn!("Failed to stop container {}: {}", name, e);
                    }
                }
            }

            if let Err(e) = self.runtime.remove_container(&container_id).await {
                tracing::warn!("Failed to remove container {}: {}", name, e);
            }
        }

        let _ = self.runtime.remove_container(&name).await;

        {
            let mut store = self.store.write().await;
            store.update_pod_status(&pod_id, crate::models::PodStatus::Terminated);
        }

        tracing::info!("Pod {} terminated", name);
    }

    async fn sync_pod_statuses(&self) {
        let running_pods: Vec<(uuid::Uuid, String)> = {
            let store = self.store.read().await;
            store
                .list_pods()
                .iter()
                .filter(|p| {
                    matches!(
                        p.status,
                        crate::models::PodStatus::Running | crate::models::PodStatus::Creating
                    )
                })
                .map(|p| (p.id, p.name.clone()))
                .collect()
        };

        for (pod_id, name) in running_pods {
            match self.runtime.get_container_state(&name).await {
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
                        crate::runtime::ContainerStatus::Paused => {
                            crate::models::PodStatus::Running
                        }
                        crate::runtime::ContainerStatus::Restarting => {
                            crate::models::PodStatus::Running
                        }
                        crate::runtime::ContainerStatus::Unknown => continue,
                    };

                    let mut store = self.store.write().await;

                    if let Some(pod) = store.get_pod(&pod_id)
                        && pod.status != new_status
                        && pod.status != crate::models::PodStatus::Terminating
                    {
                        tracing::info!(
                            "Pod {} status changed: {:?} -> {:?}",
                            name,
                            pod.status,
                            new_status
                        );

                        store.update_pod_status(&pod_id, new_status);
                    }
                }

                Err(crate::runtime::RuntimeError::ContainerNotFound(_)) => {
                    let mut store = self.store.write().await;
                    if let Some(pod) = store.get_pod(&pod_id)
                        && !matches!(
                            pod.status,
                            crate::models::PodStatus::Terminating
                                | crate::models::PodStatus::Terminated
                        )
                    {
                        tracing::warn!("Container for pod {} not found, marking as failed", name);
                        store.update_pod_status(&pod_id, crate::models::PodStatus::Failed);
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to get container state for {}: {}", name, e);
                }
            }
        }
    }

    async fn cleanup_terminated_pods(&self) {
        let terminated_pods: Vec<uuid::Uuid> = {
            let store = self.store.read().await;
            store
                .list_pods()
                .iter()
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
                .iter()
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
}
