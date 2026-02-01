#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerStatus {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Dead,
    Unknown,
}

impl From<&str> for ContainerStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "created" => ContainerStatus::Created,
            "running" => ContainerStatus::Running,
            "paused" => ContainerStatus::Paused,
            "restarting" => ContainerStatus::Restarting,
            "exited" => ContainerStatus::Exited,
            "dead" => ContainerStatus::Dead,
            _ => ContainerStatus::Unknown,
        }
    }
}

pub struct ContainerRuntime {
    docker: bollard::Docker,
}

impl ContainerRuntime {
    pub async fn new() -> crate::error::RuntimeResult<Self> {
        let docker = bollard::Docker::connect_with_local_defaults()?;

        docker.ping().await?;
        tracing::info!("Connected to Docker daemon");

        Ok(Self { docker })
    }

    pub async fn run_container(
        &self,
        name: &str,
        image: &str,
        cpu_millis: Option<u32>,
        memory_mb: Option<u32>,
    ) -> crate::error::RuntimeResult<String> {
        self.ensure_image(image).await?;

        let host_config = bollard::models::HostConfig {
            cpu_period: Some(100000),
            cpu_quota: cpu_millis.map(|m| (m as i64) * 100),
            memory: memory_mb.map(|m| (m as i64) * 1024 * 1024),
            ..Default::default()
        };

        let config = bollard::models::ContainerCreateBody {
            image: Some(image.to_string()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(name.to_string()),
            platform: String::new(),
        };

        tracing::debug!("Creating container {} with image {}", name, image);

        let response = self.docker.create_container(Some(options), config).await?;
        let container_id = response.id;

        self.docker.start_container(&container_id, None).await?;

        tracing::info!(
            "Container {} started with ID: {}",
            name,
            &container_id[..12.min(container_id.len())]
        );

        Ok(container_id)
    }

    pub async fn stop_container(&self, name_or_id: &str) -> crate::error::RuntimeResult<()> {
        tracing::info!("Stopping container: {}", name_or_id);

        let options = bollard::query_parameters::StopContainerOptions {
            t: Some(10),
            signal: None,
        };

        match self.docker.stop_container(name_or_id, Some(options)).await {
            Ok(_) => {
                tracing::info!("Container {} stopped", name_or_id);
                Ok(())
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                tracing::warn!("Container {} not found", name_or_id);
                Err(crate::error::RuntimeError::ContainerNotFound(
                    name_or_id.to_string(),
                ))
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 304, ..
            }) => {
                tracing::debug!("Container {} already stopped", name_or_id);
                Ok(())
            }
            Err(e) => Err(crate::error::RuntimeError::Docker(e)),
        }
    }

    pub async fn remove_container(&self, name_or_id: &str) -> crate::error::RuntimeResult<()> {
        tracing::info!("Removing container: {}", name_or_id);

        let options = bollard::query_parameters::RemoveContainerOptions {
            force: true,
            ..Default::default()
        };

        match self
            .docker
            .remove_container(name_or_id, Some(options))
            .await
        {
            Ok(_) => {
                tracing::info!("Container {} removed", name_or_id);
                Ok(())
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                tracing::warn!("Container {} was already removed", name_or_id);
                Ok(())
            }
            Err(e) => Err(crate::error::RuntimeError::Docker(e)),
        }
    }

    pub async fn get_container_state(
        &self,
        name_or_id: &str,
    ) -> crate::error::RuntimeResult<ContainerStatus> {
        match self.docker.inspect_container(name_or_id, None).await {
            Ok(info) => {
                let status = info
                    .state
                    .and_then(|s| s.status)
                    .map(|s| ContainerStatus::from(s.as_ref()))
                    .unwrap_or(ContainerStatus::Unknown);

                Ok(status)
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Err(crate::error::RuntimeError::ContainerNotFound(
                name_or_id.to_string(),
            )),
            Err(e) => Err(crate::error::RuntimeError::Docker(e)),
        }
    }

    async fn ensure_image(&self, image: &str) -> crate::error::RuntimeResult<()> {
        match self.docker.inspect_image(image).await {
            Ok(_) => {
                tracing::debug!("Image {} already exists", image);
                return Ok(());
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                tracing::info!("Image {} not found locally, pulling...", image);
            }
            Err(e) => return Err(crate::error::RuntimeError::Docker(e)),
        }

        let options = bollard::query_parameters::CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);

        while let Some(result) = futures_util::StreamExt::next(&mut stream).await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        tracing::debug!("Pull {}: {}", image, status);
                    }
                }
                Err(e) => return Err(crate::error::RuntimeError::Docker(e)),
            }
        }

        tracing::info!("Image {} pulled successfully", image);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_status_parsing() {
        assert_eq!(ContainerStatus::from("running"), ContainerStatus::Running);
        assert_eq!(ContainerStatus::from("Running"), ContainerStatus::Running);
        assert_eq!(ContainerStatus::from("exited"), ContainerStatus::Exited);
        assert_eq!(ContainerStatus::from("created"), ContainerStatus::Created);
        assert_eq!(ContainerStatus::from("foobar"), ContainerStatus::Unknown);
    }
}
