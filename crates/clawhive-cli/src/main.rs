use std::net::SocketAddr;
use std::sync::Arc;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "clawhive",
    about = "ClawHive OS - Recursive Agent Swarm Operating System"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the API server
    Serve {
        /// Bind address
        #[arg(default_value = "0.0.0.0:3000")]
        bind: String,
        /// Path to sled database (in-memory if not set)
        #[arg(long)]
        db: Option<String>,
    },
    /// Start the TUI
    Tui {
        /// Path to sled database (in-memory if not set)
        #[arg(long)]
        db: Option<String>,
    },
    /// Print version
    Version,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { bind, db } => {
            let addr: SocketAddr = bind.parse().expect("invalid bind address");

            let kv_store: Arc<dyn clawhive_store::Store> = match db {
                Some(path) => {
                    tracing::info!("using sled database at {path}");
                    Arc::new(
                        clawhive_store::SledStore::new(path)
                            .expect("failed to open sled database"),
                    )
                }
                None => {
                    tracing::info!("using in-memory store");
                    Arc::new(clawhive_store::InMemoryStore::new())
                }
            };

            let state = clawhive_control_api::AppState::new_with_store(kv_store);
            let app = clawhive_control_api::build_router(state);

            tracing::info!("ClawHive API server starting on {}", addr);
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        }
        Commands::Tui { db } => {
            let result = match db {
                Some(path) => {
                    let store = Arc::new(
                        clawhive_store::SledStore::new(path)
                            .expect("failed to open sled database"),
                    );
                    clawhive_tui::run_with_store(store).await
                }
                None => clawhive_tui::run().await,
            };
            if let Err(e) = result {
                tracing::error!("TUI error: {e}");
            }
        }
        Commands::Version => {
            println!("ClawHive OS v{}", env!("CARGO_PKG_VERSION"));
        }
    }
}
