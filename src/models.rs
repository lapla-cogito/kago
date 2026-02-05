#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub struct Resources {
    pub cpu_millis: u32,
    pub memory_mb: u32,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RollingUpdateConfig {
    /// Maximum number of pods that can be created above the desired replica count
    #[serde(default = "default_max_surge")]
    pub max_surge: u32,
    /// Maximum number of pods that can be unavailable during the update
    #[serde(default)]
    pub max_unavailable: u32,
}

fn default_max_surge() -> u32 {
    1
}

impl Default for RollingUpdateConfig {
    fn default() -> Self {
        Self {
            max_surge: 1,
            max_unavailable: 0,
        }
    }
}

impl Resources {
    pub fn subtract(&self, other: &Resources) -> Resources {
        Resources {
            cpu_millis: self.cpu_millis.saturating_sub(other.cpu_millis),
            memory_mb: self.memory_mb.saturating_sub(other.memory_mb),
        }
    }

    pub fn fits(&self, request: &Resources) -> bool {
        self.cpu_millis >= request.cpu_millis && self.memory_mb >= request.memory_mb
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PodStatus {
    #[default]
    Pending,
    Creating,
    Running,
    Succeeded,
    Failed,
    Terminating,
    Terminated,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Pod {
    pub id: uuid::Uuid,
    pub name: String,
    pub image: String,
    pub resources: Resources,
    pub deployment_name: Option<String>,
    pub status: PodStatus,
    pub container_id: Option<String>,
    #[serde(default)]
    pub node_name: Option<String>,
    /// Revision number for rolling updates (matches deployment's revision when created)
    #[serde(default)]
    pub revision: u64,
}

impl Pod {
    pub fn from_deployment(deployment: &Deployment, index: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            name: format!("{}-{}", deployment.name, index),
            image: deployment.image.clone(),
            resources: deployment.resources,
            deployment_name: Some(deployment.name.clone()),
            status: PodStatus::Pending,
            container_id: None,
            node_name: None,
            revision: deployment.revision,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deployment {
    pub name: String,
    pub image: String,
    pub replicas: u32,
    pub resources: Resources,
    /// Rolling update configuration
    #[serde(default)]
    pub rolling_update: RollingUpdateConfig,
    /// Current revision number, incremented on image changes
    #[serde(default = "default_revision")]
    pub revision: u64,
}

fn default_revision() -> u64 {
    1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateDeploymentRequest {
    pub name: String,
    pub image: String,
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub rolling_update: RollingUpdateConfig,
}

fn default_replicas() -> u32 {
    1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateDeploymentRequest {
    pub replicas: Option<u32>,
    pub image: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeploymentResponse {
    pub name: String,
    pub image: String,
    pub replicas: u32,
    pub resources: Resources,
    pub ready_replicas: u32,
    pub rolling_update: RollingUpdateConfig,
    pub revision: u64,
    /// Number of pods with the current revision
    pub updated_replicas: u32,
}

impl DeploymentResponse {
    pub fn from_deployment(
        deployment: &Deployment,
        ready_replicas: u32,
        updated_replicas: u32,
    ) -> Self {
        Self {
            name: deployment.name.clone(),
            image: deployment.image.clone(),
            replicas: deployment.replicas,
            resources: deployment.resources,
            ready_replicas,
            rolling_update: deployment.rolling_update,
            revision: deployment.revision,
            updated_replicas,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PodResponse {
    pub id: uuid::Uuid,
    pub name: String,
    pub image: String,
    pub status: PodStatus,
    pub deployment_name: Option<String>,
    pub node_name: Option<String>,
    pub revision: u64,
}

impl From<&Pod> for PodResponse {
    fn from(pod: &Pod) -> Self {
        Self {
            id: pod.id,
            name: pod.name.clone(),
            image: pod.image.clone(),
            status: pod.status,
            deployment_name: pod.deployment_name.clone(),
            node_name: pod.node_name.clone(),
            revision: pod.revision,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    #[default]
    Unknown,
    Ready,
    NotReady,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub capacity: Resources,
    pub allocatable: Resources,
    pub used: Resources,
    pub status: NodeStatus,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
}

impl Node {
    pub fn new(name: String, address: String, port: u16, capacity: Resources) -> Self {
        Self {
            name,
            address,
            port,
            capacity,
            allocatable: capacity,
            used: Resources::default(),
            status: NodeStatus::Ready,
            last_heartbeat: chrono::Utc::now(),
        }
    }

    pub fn available_resources(&self) -> Resources {
        self.allocatable.subtract(&self.used)
    }

    pub fn can_fit(&self, request: &Resources) -> bool {
        self.available_resources().fits(request)
    }

    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.address, self.port)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegisterNodeRequest {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub capacity: Resources,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HeartbeatRequest {
    pub used: Resources,
    pub pod_statuses: Vec<PodStatusReport>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PodStatusReport {
    pub pod_id: uuid::Uuid,
    pub status: PodStatus,
    pub container_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeResponse {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub status: NodeStatus,
    pub capacity: Resources,
    pub allocatable: Resources,
    pub used: Resources,
    pub available: Resources,
}

impl From<&Node> for NodeResponse {
    fn from(node: &Node) -> Self {
        Self {
            name: node.name.clone(),
            address: node.address.clone(),
            port: node.port,
            status: node.status,
            capacity: node.capacity,
            allocatable: node.allocatable,
            used: node.used,
            available: node.available_resources(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatePodOnNodeRequest {
    pub pod_id: uuid::Uuid,
    pub name: String,
    pub image: String,
    pub resources: Resources,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentPodStatus {
    pub pod_id: uuid::Uuid,
    pub name: String,
    pub status: PodStatus,
    pub container_id: Option<String>,
}
