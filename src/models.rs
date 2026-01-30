#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, Default, PartialEq)]
pub struct Resources {
    pub cpu_millis: u32,
    pub memory_mb: u32,
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
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deployment {
    pub name: String,
    pub image: String,
    pub replicas: u32,
    pub resources: Resources,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateDeploymentRequest {
    pub name: String,
    pub image: String,
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    #[serde(default)]
    pub resources: Resources,
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
}

impl DeploymentResponse {
    pub fn from_deployment(deployment: &Deployment, ready_replicas: u32) -> Self {
        Self {
            name: deployment.name.clone(),
            image: deployment.image.clone(),
            replicas: deployment.replicas,
            resources: deployment.resources,
            ready_replicas,
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
}

impl From<&Pod> for PodResponse {
    fn from(pod: &Pod) -> Self {
        Self {
            id: pod.id,
            name: pod.name.clone(),
            image: pod.image.clone(),
            status: pod.status,
            deployment_name: pod.deployment_name.clone(),
        }
    }
}
