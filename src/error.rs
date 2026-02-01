#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Docker error: {0}")]
    Docker(#[from] bollard::errors::Error),
    #[error("Container not found: {0}")]
    ContainerNotFound(String),
}

pub type RuntimeResult<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),
    #[error("HTTP error: {0}")]
    HttpError(String),
    #[error("YAML parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Jsonnet error: {0}")]
    JsonnetError(String),
}

pub type CliResult<T> = std::result::Result<T, CliError>;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),
}

pub type AgentResult<T> = std::result::Result<T, AgentError>;
