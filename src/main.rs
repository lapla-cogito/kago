mod api;
mod cli;
mod controller;
mod error;
mod models;
mod runtime;
mod store;

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_SERVER_URL: &str = "http://localhost:8080";

#[derive(clap::Parser)]
#[command(name = "kago")]
#[command(about = "A container orchestrator written in Rust", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand)]
enum Commands {
    Serve {
        #[arg(short, long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    Apply {
        #[arg(short, long)]
        file: std::path::PathBuf,
        #[arg(short, long, default_value = DEFAULT_SERVER_URL)]
        server: String,
    },
    Get {
        resource: String,
        #[arg(short, long, default_value = DEFAULT_SERVER_URL)]
        server: String,
    },
    Delete {
        resource: String,
        #[arg(short, long, default_value = DEFAULT_SERVER_URL)]
        server: String,
    },
}

fn main() {
    let cli = <Cli as clap::Parser>::parse();

    match cli.command {
        Some(Commands::Serve { port }) => {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(run_server(port));
        }
        Some(Commands::Apply { file, server }) => {
            if let Err(e) = run_apply(&file, &server) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Get { resource, server }) => {
            if let Err(e) = run_get(&resource, &server) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Delete { resource, server }) => {
            if let Err(e) = run_delete(&resource, &server) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        None => {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(run_server(DEFAULT_PORT));
        }
    }
}

async fn run_server(port: u16) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kago=info,tower_http=debug".into()),
        )
        .init();

    tracing::info!("Starting Kago");

    let runtime = match crate::runtime::ContainerRuntime::new().await {
        Ok(runtime) => std::sync::Arc::new(runtime),
        Err(e) => {
            tracing::error!("Failed to initialize container runtime: {}", e);
            tracing::error!("Make sure Docker or nerdctl is installed and running.");
            std::process::exit(1);
        }
    };

    let store = crate::store::new_shared_store();
    let controller = std::sync::Arc::new(crate::controller::Controller::new(
        std::sync::Arc::clone(&store),
        runtime,
    ));
    let app = crate::api::create_router(store, std::sync::Arc::clone(&controller));

    let controller_handle = tokio::spawn(async move {
        controller.run().await;
    });

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("API server listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");

    tracing::info!("Shutting down...");
    controller_handle.abort();
    tracing::info!("Kago stopped");
}

fn run_apply(file: &std::path::Path, server: &str) -> crate::error::CliResult<()> {
    let manifests = crate::cli::parse_manifests_from_file(file)?;

    if manifests.is_empty() {
        return Err(crate::error::CliError::InvalidManifest(
            "No manifests found in file".to_string(),
        ));
    }

    let client = crate::cli::CliClient::new(server);

    let mut errors = Vec::new();

    for manifest in manifests {
        match client.apply_deployment(&manifest) {
            Ok(message) => println!("{}", message),
            Err(e) => {
                eprintln!("Error applying {}: {}", manifest.spec.name, e);
                errors.push(format!("{}: {}", manifest.spec.name, e));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(crate::error::CliError::HttpError(errors.join("; ")))
    }
}

fn run_get(resource: &str, server: &str) -> crate::error::CliResult<()> {
    let client = crate::cli::CliClient::new(server);

    let output = match resource.to_lowercase().as_str() {
        "deployments" | "deployment" | "deploy" => client.get_deployments()?,

        "pods" | "pod" => client.get_pods()?,

        _ => {
            return Err(crate::error::CliError::HttpError(format!(
                "Unknown resource type: {} (available: deployments, pods)",
                resource
            )));
        }
    };

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&output) {
        if let Ok(pretty) = serde_json::to_string_pretty(&json) {
            println!("{}", pretty);
        } else {
            println!("{}", output);
        }
    } else {
        println!("{}", output);
    }

    Ok(())
}

fn run_delete(resource: &str, server: &str) -> crate::error::CliResult<()> {
    let (resource_type, name) = if resource.contains('/') {
        let parts: Vec<&str> = resource.splitn(2, '/').collect();

        (parts[0], parts[1])
    } else {
        ("deployment", resource)
    };

    let client = crate::cli::CliClient::new(server);

    let message = match resource_type.to_lowercase().as_str() {
        "deployment" | "deployments" | "deploy" => client.delete_deployment(name)?,

        _ => {
            return Err(crate::error::CliError::HttpError(format!(
                "Unknown resource type: {} (available: deployment)",
                resource_type
            )));
        }
    };

    println!("{}", message);

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received");
}
