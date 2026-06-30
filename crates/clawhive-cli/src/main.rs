use std::net::SocketAddr;
use std::sync::Arc;

use clawhive_store::StoreExt;

use clap::{Parser, Subcommand};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Registry;

mod setup_wizard;
mod telemetry_layer;

/// Load environment variables from `~/.clawhive/.env` if the file exists.
/// This makes API keys saved by the setup wizard available to the runtime
/// without requiring the user to manually export them.
fn load_clawhive_env() {
    let env_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".clawhive")
        .join(".env");

    if env_path.exists() {
        match dotenvy::from_path(&env_path) {
            Ok(_) => tracing::debug!("loaded env from {}", env_path.display()),
            Err(e) => tracing::warn!("failed to load {}: {e}", env_path.display()),
        }
    }
}

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
        /// Also start TUI in the same process to share the database
        #[arg(long)]
        tui: bool,
    },
    /// Start the TUI
    Tui {
        /// Path to sled database (in-memory if not set)
        #[arg(long)]
        db: Option<String>,
    },
    /// Run an agent objective directly from CLI with tools execution
    RunAgent {
        /// Path to sled database (in-memory if not set)
        #[arg(long)]
        db: Option<String>,
        /// The objective description for the agent
        #[arg(long)]
        objective: String,
        /// Optional model override
        #[arg(long)]
        model: Option<String>,
    },
    /// Print version
    Version,
    /// Initial setup wizard: create config file and workspace
    Setup {
        /// Force overwrite existing config
        #[arg(long)]
        force: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let is_tui = match &cli.command {
        Commands::Tui { .. } => true,
        Commands::Serve { tui, .. } => *tui,
        Commands::RunAgent { .. } => false,
        Commands::Version => false,
        Commands::Setup { .. } => false,
    };

    // Load local environment variables from ~/.clawhive/.env so that API keys
    // written by the setup wizard are available to all subcommands.
    load_clawhive_env();

    // Ensure logs directory exists
    let _ = std::fs::create_dir_all("logs");

    // Rolling file appender — daily rotation
    let file_appender = tracing_appender::rolling::daily("logs", "clawhive.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Layer 1: file output (non-ANSI)
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false);

    // Layer 2: stderr output (human-readable, ANSI)
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true);

    // Layer 3: structured JSON telemetry log for Vector consumption.
    // Only captures events with target "clawhive_telemetry".
    let telemetry_appender = tracing_appender::rolling::daily("logs", "clawhive-telemetry.json");
    let (telemetry_non_blocking, _telemetry_guard) =
        tracing_appender::non_blocking(telemetry_appender);
    let telemetry_layer = telemetry_layer::TelemetryLayer::new(telemetry_non_blocking);

    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    if is_tui {
        Registry::default()
            .with(env_filter)
            .with(file_layer)
            .with(telemetry_layer)
            .init();
    } else {
        Registry::default()
            .with(env_filter)
            .with(file_layer)
            .with(stderr_layer)
            .with(telemetry_layer)
            .init();
    }

    // Auto-detect first-run: if no config exists and not setup/version, redirect to setup
    let needs_setup = match &cli.command {
        Commands::Setup { .. } | Commands::Version => false,
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let candidates = [
                std::path::PathBuf::from("clawhive.toml"),
                std::path::PathBuf::from(&home).join(".config").join("clawhive").join("config.toml"),
                std::path::PathBuf::from(&home).join(".clawhive").join("config.toml"),
            ];
            !candidates.iter().any(|p| p.exists())
        }
    };

    if needs_setup {
        eprintln!("Belum ada konfigurasi ditemukan. Menjalankan setup wizard...\n");
        if let Err(e) = run_setup_wizard(false).await {
            eprintln!("Setup gagal: {e}");
            std::process::exit(1);
        }
    }

    match cli.command {
        Commands::Serve { bind, db, tui } => {
            let addr: SocketAddr = bind.parse().expect("invalid bind address");

            let kv_store: Arc<dyn clawhive_store::Store> = match db {
                Some(path) => {
                    tracing::info!("using sled database at {path}");
                    match clawhive_store::SledStore::new(&path) {
                        Ok(store) => Arc::new(store),
                        Err(e) => {
                            eprintln!("Error: Gagal membuka database sled di '{path}'.");
                            eprintln!("Detail: {e}");
                            eprintln!("Pastikan tidak ada proses ClawHive server atau TUI lain yang sedang berjalan menggunakan database ini.");
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    tracing::info!("using in-memory store");
                    Arc::new(clawhive_store::InMemoryStore::new())
                }
            };

            let mut registry = clawhive_model_router::provider::ModelRegistry::new();

            // 1. Try config file (clawhive.toml) for alias/custom providers
            if let Some(cfg) = clawhive_model_router::config::discover_config() {
                let builtin = clawhive_model_router::providers::provider_configs();

                // Pre-load KV store entries (sync adapter for closure)
                let mut kv_map: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for slot in &builtin {
                    let store_key = format!("config:{}_api_key", slot.name);
                    if let Ok(Some(val)) = kv_store.get::<String>(&store_key).await {
                        let trimmed = val.trim().to_string();
                        if !trimmed.is_empty() {
                            kv_map.insert(store_key, trimmed);
                        }
                    }
                }

                let kv_get = |key: &str| kv_map.get(key).cloned();
                let (resolved, errors) =
                    clawhive_model_router::config::resolve_providers(Some(&cfg), builtin, kv_get);
                for e in &errors {
                    tracing::warn!("config error: {e:?}");
                }
                for r in &resolved {
                    tracing::info!("registering provider: {} (from config)", r.name);
                }
                registry.register_resolved_providers(resolved);
            } else {
                // 2. Fallback: env var → KV store for every known provider
                for config in clawhive_model_router::providers::provider_configs() {
                    // Native providers (e.g. Bedrock) are registered via their factory.
                    if let Some(factory) = config.factory {
                        let name = config.name.to_string();
                        if !registry.list_providers().contains(&name) {
                            tracing::info!("registering native provider: {}", name);
                            registry.register(factory());
                        }
                        continue;
                    }

                    let key = match std::env::var(config.api_key_env) {
                        Ok(k) if !k.is_empty() => Some(k),
                        _ => {
                            let store_key = format!("config:{}_api_key", config.name);
                            match kv_store.get::<String>(&store_key).await {
                                Ok(Some(k)) if !k.trim().is_empty() => Some(k.trim().to_string()),
                                _ => None,
                            }
                        }
                    };
                    if let Some(key) = key {
                        tracing::info!("registering provider: {}", config.name);
                        registry.register(Box::new(
                            clawhive_model_router::openai_compat::OpenAiCompatibleProvider::with_config(
                                config.name,
                                config.base_url,
                                key,
                                config.models.clone(),
                            ),
                        ));
                    }
                }
            }

            let model_router = Arc::new(clawhive_model_router::router::ModelRouter::new(registry));

            // Register built-in tools
            let mut tool_registry = clawhive_tool::registry::ToolRegistry::new();
            tool_registry.register(Box::new(clawhive_tool::builtin::ShellTool));
            tool_registry.register(Box::new(clawhive_tool::builtin::ReadFileTool));
            tool_registry.register(Box::new(clawhive_tool::builtin::WriteFileTool));
            tool_registry.register(Box::new(clawhive_tool::builtin::HttpTool));
            let tool_registry = Arc::new(tool_registry);

            let state = clawhive_control_api::AppState::new_with_services(
                Arc::clone(&kv_store),
                model_router,
                tool_registry,
            );
            let app = clawhive_control_api::build_router(state);

            tracing::info!("ClawHive API server starting on {}", addr);
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

            if tui {
                // Jalankan API server di background task
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!("Server error: {e}");
                    }
                });
                // Jalankan TUI di thread utama
                if let Err(e) = clawhive_tui::run_with_store(kv_store).await {
                    tracing::error!("TUI error: {e}");
                }
            } else {
                axum::serve(listener, app).await.unwrap();
            }
        }
        Commands::Tui { db } => {
            let result = match db {
                Some(path) => {
                    match clawhive_store::SledStore::new(&path) {
                        Ok(store) => {
                            clawhive_tui::run_with_store(Arc::new(store)).await
                        }
                        Err(e) => {
                            eprintln!("Error: Gagal membuka database sled di '{path}'.");
                            eprintln!("Detail: {e}");
                            eprintln!("Pastikan tidak ada proses ClawHive server atau TUI lain yang sedang berjalan menggunakan database ini.");
                            std::process::exit(1);
                        }
                    }
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
        Commands::RunAgent { db, objective, model } => {
            let kv_store: Arc<dyn clawhive_store::Store> = match db {
                Some(path) => {
                    tracing::info!("using sled database at {path}");
                    match clawhive_store::SledStore::new(&path) {
                        Ok(store) => Arc::new(store),
                        Err(e) => {
                            eprintln!("Error: Gagal membuka database sled di '{path}'.");
                            eprintln!("Detail: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    tracing::info!("using in-memory store");
                    Arc::new(clawhive_store::InMemoryStore::new())
                }
            };

            // Setup router & registry
            let mut registry = clawhive_model_router::provider::ModelRegistry::new();
            if let Some(cfg) = clawhive_model_router::config::discover_config() {
                let builtin = clawhive_model_router::providers::provider_configs();
                let mut kv_map = std::collections::HashMap::new();
                for slot in &builtin {
                    let store_key = format!("config:{}_api_key", slot.name);
                    if let Ok(Some(val)) = kv_store.get::<String>(&store_key).await {
                        let trimmed = val.trim().to_string();
                        if !trimmed.is_empty() {
                            kv_map.insert(store_key, trimmed);
                        }
                    }
                }
                let kv_get = |key: &str| kv_map.get(key).cloned();
                let (resolved, _) = clawhive_model_router::config::resolve_providers(Some(&cfg), builtin, kv_get);
                registry.register_resolved_providers(resolved);
            } else {
                for config in clawhive_model_router::providers::provider_configs() {
                    // Native providers (e.g. Bedrock) are registered via their factory.
                    if let Some(factory) = config.factory {
                        let name = config.name.to_string();
                        if !registry.list_providers().contains(&name) {
                            registry.register(factory());
                        }
                        continue;
                    }

                    let key = match std::env::var(config.api_key_env) {
                        Ok(k) if !k.is_empty() => Some(k),
                        _ => {
                            let store_key = format!("config:{}_api_key", config.name);
                            match kv_store.get::<String>(&store_key).await {
                                Ok(Some(k)) if !k.trim().is_empty() => Some(k.trim().to_string()),
                                _ => None,
                            }
                        }
                    };
                    if let Some(key) = key {
                        registry.register(Box::new(
                            clawhive_model_router::openai_compat::OpenAiCompatibleProvider::with_config(
                                config.name,
                                config.base_url,
                                key,
                                config.models.clone(),
                            ),
                        ));
                    }
                }
            }

            let model_router = Arc::new(clawhive_model_router::router::ModelRouter::new(registry));

            // Setup tools
            let mut tool_registry = clawhive_tool::registry::ToolRegistry::new();
            tool_registry.register(Box::new(clawhive_tool::builtin::ShellTool));
            tool_registry.register(Box::new(clawhive_tool::builtin::ReadFileTool));
            tool_registry.register(Box::new(clawhive_tool::builtin::WriteFileTool));
            tool_registry.register(Box::new(clawhive_tool::builtin::HttpTool));
            let tool_registry = Arc::new(tool_registry);

            // Services
            let worker_service = Arc::new(clawhive_worker::WorkerService::new(Arc::clone(&kv_store)));
            let budget_service = Arc::new(clawhive_budget::BudgetService);
            let agent_store = clawhive_agent::store::AgentStore::new(Arc::clone(&kv_store));

            // Ensure minimal worker exists
            let worker_name = "cli-worker".to_string();
            let worker = worker_service.register(
                worker_name,
                clawhive_domain::WorkerType::Local,
                vec![],
                "1.0.0".to_string(),
            ).await;
            let worker_id = worker.id.clone();

            // Dapatkan model aktif (override atau ambil dari registry)
            let active_model = match model {
                Some(m) => m,
                None => {
                    let profiles = model_router.registry().list_profiles();
                    if profiles.is_empty() {
                        eprintln!("Error: Tidak ada provider model LLM yang terkonfigurasi.");
                        eprintln!("Pasang API key terlebih dahulu menggunakan TUI (Ctrl+P -> Set API Key) atau via env var.");
                        std::process::exit(1);
                    }
                    profiles[0].id.clone()
                }
            };

            // Dapatkan atau buat default Agent
            let agent_id = match agent_store.list(clawhive_agent::store::AgentQuery::default()).await {
                Ok(ref list) if !list.is_empty() => list[0].id.clone(),
                _ => {
                    // Create default agent
                    let now = chrono::Utc::now();
                    let new_agent = clawhive_domain::Agent {
                        id: clawhive_domain::AgentId(uuid::Uuid::now_v7()),
                        identity_id: clawhive_domain::IdentityId(uuid::Uuid::now_v7()),
                        mission_id: clawhive_domain::MissionId(uuid::Uuid::now_v7()),
                        parent_agent_id: None,
                        lineage_id: clawhive_domain::LineageId(uuid::Uuid::now_v7()),
                        name: "cli-agent".into(),
                        role: "Specialist".into(),
                        genome: clawhive_domain::AgentGenome {
                            id: "cli-genome".into(),
                            version: "1.0.0".into(),
                            role: "Specialist".into(),
                            lifecycle_modes: vec![clawhive_domain::LifecycleMode::Ephemeral],
                            model_policy: clawhive_domain::ModelPolicy {
                                preferred_profile: active_model.clone(),
                                fallback_profiles: vec![],
                                max_context_tokens: 128_000,
                            },
                            autonomy: clawhive_domain::AutonomyConfig {
                                can_spawn: false,
                                max_spawn_depth: 0,
                                max_children: 0,
                            },
                            delegable_permissions: vec![],
                            non_delegable_permissions: vec![],
                            memory: clawhive_domain::MemoryConfig {
                                default_read_scopes: vec![],
                                default_write_scope: None,
                            },
                            runtime: clawhive_domain::RuntimeConfig {
                                preferred_class: "local".into(),
                                network: clawhive_domain::NetworkPolicy::AllowByDefault,
                            },
                            verification_required: false,
                        },
                        state: clawhive_domain::AgentState::Ready,
                        lifecycle_mode: clawhive_domain::LifecycleMode::Ephemeral,
                        persistent_pattern: None,
                        budget: clawhive_domain::Budget {
                            allocated_usd: 100.0,
                            spent_usd: 0.0,
                            soft_limit_usd: None,
                            hard_limit_usd: Some(100.0),
                            recurring_monthly_usd: None,
                        },
                        delegable_permissions: vec![],
                        non_delegable_permissions: vec![],
                        current_runtime: None,
                        checkpoints: vec![],
                        subscriptions: vec![],
                        schedules: vec![],
                        policy_bundle: clawhive_domain::PolicyBundle {
                            id: clawhive_domain::PolicyBundleId(uuid::Uuid::now_v7()),
                            name: "default".into(),
                            version: "1.0.0".into(),
                            rules: vec![],
                            is_active: true,
                            signed_by: None,
                            signature: None,
                            activated_at: None,
                            created_at: now,
                        },
                        turn_count: 0,
                        total_cost_usd: 0.0,
                        created_at: now,
                        updated_at: now,
                        terminated_at: None,
                    };
                    let id = new_agent.id.clone();
                    if let Err(e) = agent_store.save(&new_agent).await {
                        eprintln!("Error: Gagal menyimpan agent default: {e}");
                        std::process::exit(1);
                    }
                    id
                }
            };

            // Inisialisasi runtime
            let runtime = clawhive_agent::runtime::AgentRuntime::new(
                agent_store,
                model_router,
                tool_registry,
                budget_service,
                worker_service,
                Some(worker_id),
            );

            println!("=== ClawHive Agent CLI Executor ===");
            println!("Objective: \"{}\"", objective);
            println!("Model: {}", active_model);
            println!("Memulai eksekusi agent...");

            let mut context = std::collections::HashMap::new();
            context.insert("mission_statement".to_string(), "CLI SWARM EXECUTION".to_string());

            match runtime.execute_agent(&agent_id, objective, context, None).await {
                Ok((session, events)) => {
                    println!("\n--- Eksekusi Selesai ---");
                    for event in events {
                        match event {
                            clawhive_agent::events::AgentEvent::Thought { content, .. } => {
                                println!("\n[Thought]\n{}", content.trim());
                            }
                            clawhive_agent::events::AgentEvent::ModelCall { tokens, cost, .. } => {
                                println!("[Model Call] Tokens used: {} | Cost: ${:.5}", tokens, cost);
                            }
                            clawhive_agent::events::AgentEvent::ToolCall { tool, args, result } => {
                                println!("[Tool Call] Running tool '{}' with args: {}", tool, args);
                                println!("[Tool Response] Output:\n{}", result);
                            }
                            clawhive_agent::events::AgentEvent::ObjectiveComplete { summary, evidence } => {
                                println!("\n[Objective Complete]");
                                println!("Ringkasan: {}", summary);
                                println!("Bukti (Evidence): {:?}", evidence);
                            }
                            clawhive_agent::events::AgentEvent::Error { message } => {
                                println!("[Error Event] {}", message);
                            }
                            _ => {}
                        }
                    }
                    println!("\n--- Ringkasan Sesi ---");
                    println!("Status Sesi: {:?}", session.state);
                    println!("Total Turn: {}", session.turn_count);
                    println!("Total Tokens: {}", session.total_tokens);
                    println!("Estimasi Biaya: ${:.5}", session.total_cost_usd);
                }
                Err(e) => {
                    eprintln!("\nError Eksekusi Agent: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Setup { force } => {
            if let Err(e) = run_setup_wizard(force).await {
                eprintln!("Setup failed: {e}");
                std::process::exit(1);
            }
        }
    }
}

async fn run_setup_wizard(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let default_path = std::path::PathBuf::from(&home).join(".clawhive").join("config.toml");
    let local_path = std::path::PathBuf::from("clawhive.toml");

    let target_path = if local_path.exists() {
        local_path
    } else if default_path.exists() && !force {
        default_path
    } else {
        default_path
    };

    let mut wizard = setup_wizard::SetupWizard::new(target_path);
    wizard.run()?;
    Ok(())
}
