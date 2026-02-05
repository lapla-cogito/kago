#[derive(Debug, Default)]
pub struct Store {
    deployments: std::collections::HashMap<String, crate::models::Deployment>,
    pods: std::collections::HashMap<uuid::Uuid, crate::models::Pod>,
    nodes: std::collections::HashMap<String, crate::models::Node>,
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

    pub fn list_deployments(&self) -> Vec<crate::models::Deployment> {
        self.deployments.values().cloned().collect()
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

    pub fn get_pod_mut(&mut self, id: &uuid::Uuid) -> Option<&mut crate::models::Pod> {
        self.pods.get_mut(id)
    }

    pub fn list_pods(&self) -> Vec<crate::models::Pod> {
        self.pods.values().cloned().collect()
    }

    pub fn list_pods_for_deployment(&self, deployment_name: &str) -> Vec<crate::models::Pod> {
        self.pods
            .values()
            .filter(|p| p.deployment_name.as_deref() == Some(deployment_name))
            .cloned()
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

    pub fn assign_pod_to_node(&mut self, pod_id: &uuid::Uuid, node_name: &str) -> bool {
        if let Some(pod) = self.pods.get_mut(pod_id) {
            pod.node_name = Some(node_name.to_string());
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

    pub fn get_old_revision_pods(
        &self,
        deployment_name: &str,
        current_revision: u64,
    ) -> Vec<crate::models::Pod> {
        self.pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && p.revision < current_revision
                    && !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated
                            | crate::models::PodStatus::Terminating
                            | crate::models::PodStatus::Failed
                    )
            })
            .cloned()
            .collect()
    }

    pub fn count_running_pods_for_revision(&self, deployment_name: &str, revision: u64) -> u32 {
        self.pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && p.revision == revision
                    && p.status == crate::models::PodStatus::Running
            })
            .count() as u32
    }

    /// Count all active (non-terminated/failed) pods with the current revision
    pub fn count_active_pods_for_revision(&self, deployment_name: &str, revision: u64) -> u32 {
        self.pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && p.revision == revision
                    && !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated | crate::models::PodStatus::Failed
                    )
            })
            .count() as u32
    }

    pub fn get_old_pods_to_terminate(
        &self,
        deployment_name: &str,
        current_revision: u64,
        count: u32,
    ) -> Vec<uuid::Uuid> {
        let mut pods: Vec<_> = self
            .pods
            .values()
            .filter(|p| {
                p.deployment_name.as_deref() == Some(deployment_name)
                    && p.revision < current_revision
                    && !matches!(
                        p.status,
                        crate::models::PodStatus::Terminated
                            | crate::models::PodStatus::Terminating
                            | crate::models::PodStatus::Failed
                    )
            })
            .collect();

        // Sort by name descending to terminate newer pods first
        pods.sort_by(|a, b| b.name.cmp(&a.name));

        pods.into_iter()
            .take(count as usize)
            .map(|p| p.id)
            .collect()
    }

    pub fn get_unassigned_pods(&self) -> Vec<crate::models::Pod> {
        self.pods
            .values()
            .filter(|p| {
                p.node_name.is_none()
                    && matches!(
                        p.status,
                        crate::models::PodStatus::Pending | crate::models::PodStatus::Creating
                    )
            })
            .cloned()
            .collect()
    }

    pub fn register_node(&mut self, node: crate::models::Node) {
        self.nodes.insert(node.name.clone(), node);
    }

    pub fn get_node(&self, name: &str) -> Option<&crate::models::Node> {
        self.nodes.get(name)
    }

    pub fn list_nodes(&self) -> Vec<crate::models::Node> {
        self.nodes.values().cloned().collect()
    }

    pub fn delete_node(&mut self, name: &str) -> Option<crate::models::Node> {
        self.nodes.remove(name)
    }

    pub fn update_node_heartbeat(&mut self, name: &str) -> bool {
        if let Some(node) = self.nodes.get_mut(name) {
            node.last_heartbeat = chrono::Utc::now();
            node.status = crate::models::NodeStatus::Ready;
            true
        } else {
            false
        }
    }

    pub fn update_node_status(&mut self, name: &str, status: crate::models::NodeStatus) -> bool {
        if let Some(node) = self.nodes.get_mut(name) {
            node.status = status;
            true
        } else {
            false
        }
    }

    pub fn update_node_resources(&mut self, name: &str, used: crate::models::Resources) -> bool {
        if let Some(node) = self.nodes.get_mut(name) {
            node.used = used;
            true
        } else {
            false
        }
    }

    pub fn get_ready_nodes(&self) -> Vec<crate::models::Node> {
        let mut nodes: Vec<_> = self
            .nodes
            .values()
            .filter(|n| n.status == crate::models::NodeStatus::Ready)
            .cloned()
            .collect();
        nodes.sort_by(|a, b| a.name.cmp(&b.name));
        nodes
    }

    pub fn allocate_resources_on_node(
        &mut self,
        node_name: &str,
        resources: &crate::models::Resources,
    ) -> bool {
        if let Some(node) = self.nodes.get_mut(node_name) {
            if !node.can_fit(resources) {
                return false;
            }
            node.used.cpu_millis += resources.cpu_millis;
            node.used.memory_mb += resources.memory_mb;
            true
        } else {
            false
        }
    }

    pub fn deallocate_resources_on_node(
        &mut self,
        node_name: &str,
        resources: &crate::models::Resources,
    ) -> bool {
        if let Some(node) = self.nodes.get_mut(node_name) {
            node.used.cpu_millis = node.used.cpu_millis.saturating_sub(resources.cpu_millis);
            node.used.memory_mb = node.used.memory_mb.saturating_sub(resources.memory_mb);
            true
        } else {
            false
        }
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
            rolling_update: crate::models::RollingUpdateConfig::default(),
            revision: 1,
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
            node_name: None,
            revision: 1,
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
            rolling_update: crate::models::RollingUpdateConfig::default(),
            revision: 1,
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

    #[test]
    fn test_node_crud() {
        let mut store = Store::new();

        let node = crate::models::Node::new(
            "worker-1".to_string(),
            "localhost".to_string(),
            8081,
            crate::models::Resources {
                cpu_millis: 4000,
                memory_mb: 8192,
            },
        );

        store.register_node(node);
        assert!(store.get_node("worker-1").is_some());
        assert_eq!(store.list_nodes().len(), 1);

        store.update_node_heartbeat("worker-1");
        assert_eq!(
            store.get_node("worker-1").unwrap().status,
            crate::models::NodeStatus::Ready
        );

        store.delete_node("worker-1");
        assert!(store.get_node("worker-1").is_none());
    }

    #[test]
    fn test_node_resource_allocation() {
        let mut store = Store::new();

        let node = crate::models::Node::new(
            "worker-1".to_string(),
            "localhost".to_string(),
            8081,
            crate::models::Resources {
                cpu_millis: 4000,
                memory_mb: 8192,
            },
        );
        store.register_node(node);

        let resources = crate::models::Resources {
            cpu_millis: 1000,
            memory_mb: 2048,
        };

        let node = store.get_node("worker-1").unwrap();
        assert!(node.can_fit(&resources));

        store.allocate_resources_on_node("worker-1", &resources);
        let node = store.get_node("worker-1").unwrap();
        assert_eq!(node.used.cpu_millis, 1000);
        assert_eq!(node.used.memory_mb, 2048);

        let large_resources = crate::models::Resources {
            cpu_millis: 4000,
            memory_mb: 8192,
        };

        // After allocation, should not fit large resources
        assert!(!node.can_fit(&large_resources));

        store.deallocate_resources_on_node("worker-1", &resources);
        let node = store.get_node("worker-1").unwrap();
        assert_eq!(node.used.cpu_millis, 0);
        assert_eq!(node.used.memory_mb, 0);
    }

    #[test]
    fn test_rolling_update_pod_tracking() {
        let mut store = Store::new();

        let deployment_v1 = crate::models::Deployment {
            name: "web".to_string(),
            image: "nginx:1.0".to_string(),
            replicas: 3,
            resources: crate::models::Resources {
                cpu_millis: 100,
                memory_mb: 128,
            },
            rolling_update: crate::models::RollingUpdateConfig::default(),
            revision: 1,
        };

        let pod1 = crate::models::Pod::from_deployment(&deployment_v1, 0);
        let pod2 = crate::models::Pod::from_deployment(&deployment_v1, 1);
        let pod3 = crate::models::Pod::from_deployment(&deployment_v1, 2);

        store.add_pod(pod1.clone());
        store.add_pod(pod2.clone());
        store.add_pod(pod3.clone());
        store.update_pod_status(&pod1.id, crate::models::PodStatus::Running);
        store.update_pod_status(&pod2.id, crate::models::PodStatus::Running);
        store.update_pod_status(&pod3.id, crate::models::PodStatus::Running);
        let old_pods = store.get_old_revision_pods("web", 1);
        assert_eq!(old_pods.len(), 0);

        let deployment_v2 = crate::models::Deployment {
            name: "web".to_string(),
            image: "nginx:2.0".to_string(),
            replicas: 3,
            resources: crate::models::Resources {
                cpu_millis: 100,
                memory_mb: 128,
            },
            rolling_update: crate::models::RollingUpdateConfig::default(),
            revision: 2,
        };
        let old_pods = store.get_old_revision_pods("web", 2);
        assert_eq!(old_pods.len(), 3);
        let new_pod1 = crate::models::Pod::from_deployment(&deployment_v2, 3);
        store.add_pod(new_pod1.clone());
        store.update_pod_status(&new_pod1.id, crate::models::PodStatus::Running);
        assert_eq!(store.count_running_pods_for_revision("web", 1), 3);
        assert_eq!(store.count_running_pods_for_revision("web", 2), 1);
        assert_eq!(store.count_active_pods_for_revision("web", 2), 1);

        let to_terminate = store.get_old_pods_to_terminate("web", 2, 1);
        assert_eq!(to_terminate.len(), 1);
        store.update_pod_status(&to_terminate[0], crate::models::PodStatus::Terminated);

        let old_pods = store.get_old_revision_pods("web", 2);
        assert_eq!(old_pods.len(), 2);
    }
}
