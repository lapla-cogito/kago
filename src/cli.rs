#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Kind {
    Deployment,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ResourceSpec {
    #[serde(default)]
    pub cpu: Option<CpuValue>,
    #[serde(default)]
    pub memory: Option<MemoryValue>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CpuValue {
    Millis(u32),
    String(String),
}

impl CpuValue {
    pub fn to_millis(&self) -> u32 {
        match self {
            CpuValue::Millis(m) => *m,
            CpuValue::String(s) => {
                let s = s.trim();
                if let Some(stripped) = s.strip_suffix('m') {
                    stripped.parse().unwrap_or(0)
                } else if let Ok(cores) = s.parse::<f64>() {
                    (cores * 1000.0) as u32
                } else {
                    0
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum MemoryValue {
    Megabytes(u32),
    String(String),
}

impl MemoryValue {
    pub fn to_megabytes(&self) -> u32 {
        match self {
            MemoryValue::Megabytes(m) => *m,
            MemoryValue::String(s) => {
                let s = s.trim();
                if let Some(stripped) = s.strip_suffix("Mi") {
                    stripped.parse().unwrap_or(0)
                } else if let Some(stripped) = s.strip_suffix("Gi") {
                    stripped.parse::<u32>().unwrap_or(0) * 1024
                } else if let Some(stripped) = s.strip_suffix('M') {
                    stripped.parse().unwrap_or(0)
                } else if let Some(stripped) = s.strip_suffix('G') {
                    stripped.parse::<u32>().unwrap_or(0) * 1024
                } else {
                    s.parse().unwrap_or(0)
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeploymentSpec {
    pub name: String,
    pub image: String,
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    #[serde(default)]
    pub resources: ResourceSpec,
}

fn default_replicas() -> u32 {
    1
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeploymentManifest {
    pub kind: Kind,
    pub spec: DeploymentSpec,
}

impl DeploymentManifest {
    #[cfg(test)]
    pub fn from_yaml(yaml: &str) -> crate::error::CliResult<Self> {
        let manifest: DeploymentManifest = serde_yaml::from_str(yaml)?;
        manifest.validate()?;

        Ok(manifest)
    }

    pub fn validate(&self) -> crate::error::CliResult<()> {
        if self.spec.name.is_empty() {
            return Err(crate::error::CliError::InvalidManifest(
                "name cannot be empty".to_string(),
            ));
        }
        if self.spec.image.is_empty() {
            return Err(crate::error::CliError::InvalidManifest(
                "image cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    pub fn to_create_request(&self) -> crate::models::CreateDeploymentRequest {
        crate::models::CreateDeploymentRequest {
            name: self.spec.name.clone(),
            image: self.spec.image.clone(),
            replicas: self.spec.replicas,
            resources: crate::models::Resources {
                cpu_millis: self
                    .spec
                    .resources
                    .cpu
                    .as_ref()
                    .map(|c| c.to_millis())
                    .unwrap_or(0),
                memory_mb: self
                    .spec
                    .resources
                    .memory
                    .as_ref()
                    .map(|m| m.to_megabytes())
                    .unwrap_or(0),
            },
        }
    }
}

pub fn parse_manifests(yaml: &str) -> crate::error::CliResult<Vec<DeploymentManifest>> {
    let mut manifests = Vec::new();

    for document in serde_yaml::Deserializer::from_str(yaml) {
        let value = <serde_yaml::Value as serde::Deserialize>::deserialize(document)?;
        if value.is_null() {
            continue;
        }
        if let serde_yaml::Value::Mapping(mapping) = &value
            && mapping.is_empty()
        {
            continue;
        }

        let manifest: DeploymentManifest = serde_yaml::from_value(value)?;
        manifest.validate()?;
        manifests.push(manifest);
    }

    Ok(manifests)
}

pub fn parse_manifests_from_file(
    path: &std::path::Path,
) -> crate::error::CliResult<Vec<DeploymentManifest>> {
    let content = std::fs::read_to_string(path)?;

    parse_manifests(&content)
}

pub struct CliClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl CliClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn apply_deployment(
        &self,
        manifest: &DeploymentManifest,
    ) -> crate::error::CliResult<String> {
        let url = format!("{}/deployments", self.base_url);
        let request = manifest.to_create_request();

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;

        if response.status().is_success() {
            return Ok(format!("deployment/{} created", manifest.spec.name));
        }

        if response.status() == reqwest::StatusCode::CONFLICT {
            let update_url = format!("{}/deployments/{}", self.base_url, manifest.spec.name);
            let update_response = self
                .client
                .put(&update_url)
                .json(&serde_json::json!({
                    "replicas": request.replicas,
                    "image": request.image,
                }))
                .send()
                .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;

            if update_response.status().is_success() {
                return Ok(format!("deployment/{} configured", manifest.spec.name));
            }

            let error_text = update_response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(crate::error::CliError::HttpError(error_text));
        }

        let error_text = response
            .text()
            .unwrap_or_else(|_| "Unknown error".to_string());
        Err(crate::error::CliError::HttpError(error_text))
    }

    pub fn delete_deployment(&self, name: &str) -> crate::error::CliResult<String> {
        let url = format!("{}/deployments/{}", self.base_url, name);

        let response = self
            .client
            .delete(&url)
            .send()
            .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;

        if response.status().is_success() {
            Ok(format!("deployment/{} deleted", name))
        } else if response.status() == reqwest::StatusCode::NOT_FOUND {
            Err(crate::error::CliError::HttpError(format!(
                "deployment '{}' not found",
                name
            )))
        } else {
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(crate::error::CliError::HttpError(error_text))
        }
    }

    pub fn get_deployments(&self) -> crate::error::CliResult<String> {
        let url = format!("{}/deployments", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;

        if response.status().is_success() {
            let text = response
                .text()
                .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;
            Ok(text)
        } else {
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(crate::error::CliError::HttpError(error_text))
        }
    }

    pub fn get_pods(&self) -> crate::error::CliResult<String> {
        let url = format!("{}/pods", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;

        if response.status().is_success() {
            let text = response
                .text()
                .map_err(|e| crate::error::CliError::HttpError(e.to_string()))?;
            Ok(text)
        } else {
            let error_text = response
                .text()
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(crate::error::CliError::HttpError(error_text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_parse_deployment_manifest() {
        let yaml = r#"
kind: Deployment
spec:
  name: web
  image: nginx:latest
  replicas: 3
  resources:
    cpu: 100m
    memory: 128Mi
"#;

        let manifest = DeploymentManifest::from_yaml(yaml).unwrap();
        assert_eq!(manifest.spec.name, "web");
        assert_eq!(manifest.spec.image, "nginx:latest");
        assert_eq!(manifest.spec.replicas, 3);

        let request = manifest.to_create_request();
        assert_eq!(request.resources.cpu_millis, 100);
        assert_eq!(request.resources.memory_mb, 128);
    }

    #[test]
    fn test_parse_minimal_manifest() {
        let yaml = r#"
kind: Deployment
spec:
  name: simple
  image: alpine:latest
"#;

        let manifest = DeploymentManifest::from_yaml(yaml).unwrap();
        assert_eq!(manifest.spec.name, "simple");
        assert_eq!(manifest.spec.replicas, 1);
    }

    #[test]
    fn test_cpu_value_parsing() {
        assert_eq!(CpuValue::String("100m".to_string()).to_millis(), 100);
        assert_eq!(CpuValue::String("1".to_string()).to_millis(), 1000);
        assert_eq!(CpuValue::String("0.5".to_string()).to_millis(), 500);
        assert_eq!(CpuValue::Millis(200).to_millis(), 200);
    }

    #[test]
    fn test_memory_value_parsing() {
        assert_eq!(MemoryValue::String("128Mi".to_string()).to_megabytes(), 128);
        assert_eq!(MemoryValue::String("1Gi".to_string()).to_megabytes(), 1024);
        assert_eq!(MemoryValue::String("256M".to_string()).to_megabytes(), 256);
        assert_eq!(MemoryValue::String("2G".to_string()).to_megabytes(), 2048);
        assert_eq!(MemoryValue::Megabytes(512).to_megabytes(), 512);
    }

    #[test]
    fn test_parse_multiple_manifests() {
        let yaml = r#"
kind: Deployment
spec:
  name: app1
  image: nginx:latest
---
kind: Deployment
spec:
  name: app2
  image: redis:latest
  replicas: 2
"#;

        let manifests = parse_manifests(yaml).unwrap();
        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].spec.name, "app1");
        assert_eq!(manifests[1].spec.name, "app2");
    }

    #[test]
    fn test_invalid_manifest_empty_name() {
        let yaml = r#"
kind: Deployment
spec:
  name: ""
  image: nginx:latest
"#;

        let result = DeploymentManifest::from_yaml(yaml);
        assert!(result.is_err());
    }
}
