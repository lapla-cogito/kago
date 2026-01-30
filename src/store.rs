#[derive(Debug, Default)]
pub struct Store {
    deployments: std::collections::HashMap<String, crate::models::Deployment>,
    pods: std::collections::HashMap<uuid::Uuid, crate::models::Pod>,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert_deployment(&mut self, deployment: crate::models::Deployment) {
        self.deployments.insert(deployment.name.clone(), deployment);
    }

    pub fn get_deployment(&self, name: &str) -> Option<&crate::models::Deployment> {
        self.deployments.get(name)
    }

    pub fn list_deployments(&self) -> Vec<&crate::models::Deployment> {
        self.deployments.values().collect()
    }

    pub fn delete_deployment(&mut self, name: &str) -> Option<crate::models::Deployment> {
        self.deployments.remove(name)
    }

    pub fn add_pod(&mut self, pod: crate::models::Pod) {
        self.pods.insert(pod.id, pod);
    }

    pub fn get_pod(&self, id: &uuid::Uuid) -> Option<&crate::models::Pod> {
        self.pods.get(id)
    }

    pub fn list_pods(&self) -> Vec<&crate::models::Pod> {
        self.pods.values().collect()
    }

    pub fn list_pods_for_deployment(&self, deployment_name: &str) -> Vec<&crate::models::Pod> {
        self.pods
            .values()
            .filter(|p| p.deployment_name.as_deref() == Some(deployment_name))
            .collect()
    }

    pub fn delete_pod(&mut self, id: &uuid::Uuid) -> Option<crate::models::Pod> {
        self.pods.remove(id)
    }

    pub fn update_pod_status(&mut self, id: &uuid::Uuid, status: crate::models::PodStatus) -> bool {
        if let Some(pod) = self.pods.get_mut(id) {
            pod.status = status;
            true
        } else {
            false
        }
    }

    pub fn update_pod_container_id(&mut self, id: &uuid::Uuid, container_id: String) -> bool {
        if let Some(pod) = self.pods.get_mut(id) {
            pod.container_id = Some(container_id);
            true
        } else {
            false
        }
    }

    pub fn count_running_pods_for_deployment(&self, deployment_name: &str) -> u32 {
        self.pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && p.status == crate::models::PodStatus::Running
            })
            .count() as u32
    }

    pub fn count_active_pods_for_deployment(&self, deployment_name: &str) -> u32 {
        self.pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated | crate::models::PodStatus::Failed
                    )
            })
            .count() as u32
    }

    pub fn get_pods_to_terminate(&self, deployment_name: &str, count: u32) -> Vec<uuid::Uuid> {
        let mut pods: Vec<_> = self
            .pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated
                            | crate::models::PodStatus::Terminating
                            | crate::models::PodStatus::Failed
                    )
            })
            .collect();

        pods.sort_by(|a, b| b.name.cmp(&a.name));

        pods.into_iter()
            .take(count as usize)
            .map(|p| p.id)
            .collect()
    }

    pub fn get_pending_pods(&self) -> Vec<&crate::models::Pod> {
        self.pods
            .values()
            .filter(|p| p.status == crate::models::PodStatus::Pending)
            .collect()
    }
}

pub type SharedStore = std::sync::Arc<tokio::sync::RwLock<Store>>;

pub fn new_shared_store() -> SharedStore {
    std::sync::Arc::new(tokio::sync::RwLock::new(Store::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deployment_crud() {
        let mut store = Store::new();

        let deployment = crate::models::Deployment {
            name: "web".to_string(),
            image: "nginx:latest".to_string(),
            replicas: 3,
            resources: crate::models::Resources {
                cpu_millis: 100,
                memory_mb: 128,
            },
        };

        store.upsert_deployment(deployment);
        assert!(store.get_deployment("web").is_some());
        assert_eq!(store.list_deployments().len(), 1);

        store.delete_deployment("web");
        assert!(store.get_deployment("web").is_none());
    }

    #[test]
    fn test_pod_crud() {
        let mut store = Store::new();

        let pod = crate::models::Pod {
            id: uuid::Uuid::new_v4(),
            name: "web-0".to_string(),
            image: "nginx:latest".to_string(),
            resources: crate::models::Resources {
                cpu_millis: 100,
                memory_mb: 128,
            },
            deployment_name: None,
            status: crate::models::PodStatus::Pending,
            container_id: None,
        };
        let pod_id = pod.id;

        store.add_pod(pod);
        assert!(store.get_pod(&pod_id).is_some());

        store.update_pod_status(&pod_id, crate::models::PodStatus::Running);
        assert_eq!(
            store.get_pod(&pod_id).unwrap().status,
            crate::models::PodStatus::Running
        );

        store.delete_pod(&pod_id);
        assert!(store.get_pod(&pod_id).is_none());
    }

    #[test]
    fn test_pods_for_deployment() {
        let mut store = Store::new();

        let deployment = crate::models::Deployment {
            name: "web".to_string(),
            image: "nginx:latest".to_string(),
            replicas: 2,
            resources: crate::models::Resources {
                cpu_millis: 100,
                memory_mb: 128,
            },
        };

        let pod1 = crate::models::Pod::from_deployment(&deployment, 0);
        let pod2 = crate::models::Pod::from_deployment(&deployment, 1);

        store.add_pod(pod1);
        store.add_pod(pod2);

        let pods = store.list_pods_for_deployment("web");
        assert_eq!(pods.len(), 2);

        let count = store.count_active_pods_for_deployment("web");
        assert_eq!(count, 2);
    }
}
