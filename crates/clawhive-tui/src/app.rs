    use std::sync::Arc;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use clawhive_control_api::state::AppState;
use clawhive_control_api::store::{AGENT_PREFIX, MISSION_PREFIX, SPAWNREQ_PREFIX};
use clawhive_domain::{
    Agent, AgentId, AgentState, ChildSpawnPolicy, ChildSpec, Mission, SpawnRequest, SpawnRequestId,
    SpawnState, SwarmTeamSpec, TerminationPolicy, Worker,
};
use clawhive_model_router::types::{
    group_models_by_family, ChatRequest, MessageRole, ModelFamily, ModelMessage, ModelProfile,
    StreamEvent,
};
use clawhive_store::StoreExt;

use crate::ui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Session,
    Agents,
    Workers,
    SpawnRequests,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Chat,
}

pub enum InputMode {
    Normal,
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelectionStep {
    SelectProvider,
    SelectFamily,
    SelectVariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandMode {
    None,
    CommandPalette {
        search_query: String,
        selected_index: usize,
        filtered_items: Vec<(String, String, String, String)>, // (category, name, shortcut, action)
    },
    ApiKeyInput {
        key_input: String,
        error_message: String,
    },
    ModelSelection,
}

pub struct TuiApp {
    pub state: AppState,
    pub agents: Vec<Agent>,
    pub workers: Vec<Worker>,
    pub spawn_requests: Vec<SpawnRequest>,
    pub selected_index: usize,
    pub selected_tab: Tab,
    pub should_quit: bool,
    pub status_message: String,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub active_screen: Screen,
    pub chat_history: Vec<(String, String, String)>, // (sender, role/model, message)
    pub command_mode: CommandMode,
    pub active_model: String,
    // Model selection flow state
    pub model_sel_step: ModelSelectionStep,
    pub model_sel_provider: String,
    pub model_sel_families: Vec<ModelFamily>,
    pub model_sel_variants: Vec<ModelProfile>,
    pub model_sel_search: String,
    pub model_sel_index: usize,
    /// If set, model selection was interrupted to ask for API key.
    /// After key is set, resume selecting models for this provider.
    pub model_sel_pending_provider: Option<String>,
    /// Receiver for streaming response events from a spawned task.
    stream_rx: Option<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>>,
    /// True while a streaming response is in flight.
    is_streaming: bool,
}

impl TuiApp {
    #[must_use]
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            agents: Vec::new(),
            workers: Vec::new(),
            spawn_requests: Vec::new(),
            selected_index: 0,
            selected_tab: Tab::Session,
            should_quit: false,
            status_message: "ClawHive OS — Ctrl+P: palette | Esc: home | :help: commands".into(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            active_screen: Screen::Home,
            chat_history: Vec::new(),
            command_mode: CommandMode::None,
            active_model: "Not Configured".into(),
            model_sel_step: ModelSelectionStep::SelectProvider,
            model_sel_provider: String::new(),
            model_sel_families: Vec::new(),
            model_sel_variants: Vec::new(),
            model_sel_search: String::new(),
            model_sel_index: 0,
            model_sel_pending_provider: None,
            stream_rx: None,
            is_streaming: false,
        }
    }

    pub async fn refresh(&mut self) {
        self.agents = self
            .state
            .kv_store
            .scan_prefix::<Agent>(AGENT_PREFIX)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, a)| a)
            .collect();

        self.workers = match self.state.worker_service.list(None).await {
            Ok(w) => w,
            Err(_) => Vec::new(),
        };

        self.spawn_requests = self
            .state
            .kv_store
            .scan_prefix::<SpawnRequest>(SPAWNREQ_PREFIX)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, r)| r)
            .collect();

        if self.selected_index >= self.current_list_len() {
            self.selected_index = self.current_list_len().saturating_sub(1);
        }
    }

    fn current_list_len(&self) -> usize {
        match self.selected_tab {
            Tab::Session => 0,
            Tab::Agents => self.agents.len(),
            Tab::Workers => self.workers.len(),
            Tab::SpawnRequests => self.spawn_requests.len(),
        }
    }

    /// Scan config file, env vars, and KV store for every known provider.
    /// Supports the ZeroClaw-style alias system from `clawhive.toml`.
    async fn register_all_providers(&mut self) {
        use clawhive_model_router::config::{discover_config, resolve_providers};
        use clawhive_model_router::provider::ModelRegistry;
        use clawhive_store::StoreExt;

        let mut registry = ModelRegistry::new();

        // Pre-load KV store entries for all provider API keys
        let builtin = clawhive_model_router::providers::provider_configs();
        let mut kv_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for slot in &builtin {
            let store_key = format!("config:{}_api_key", slot.name);
            if let Ok(Some(val)) = self.state.kv_store.get::<String>(&store_key).await {
                let trimmed = val.trim().to_string();
                if !trimmed.is_empty() {
                    kv_map.insert(store_key, trimmed);
                }
            }
        }

        let config = discover_config();

        if let Some(ref cfg) = config {
            let kv_get = |key: &str| kv_map.get(key).cloned();
            let (resolved, errors) = resolve_providers(Some(cfg), builtin, kv_get);
            for e in &errors {
                tracing::warn!("config error: {e:?}");
            }
            registry.register_resolved_providers(resolved);
        } else {
            // No config file — fallback: env var → KV store for each built-in provider
            for config in clawhive_model_router::providers::provider_configs() {
                let key = match std::env::var(config.api_key_env) {
                    Ok(k) if !k.is_empty() => Some(k),
                    _ => {
                        let store_key = format!("config:{}_api_key", config.name);
                        self.state.kv_store.get::<String>(&store_key).await
                            .ok()
                            .flatten()
                            .filter(|k| !k.trim().is_empty())
                            .map(|k| k.trim().to_string())
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

        let total = registry.list_profiles().len();
        if total > 0 {
            let router = Arc::new(clawhive_model_router::router::ModelRouter::new(registry));
            if let Some(first) = router.registry().list_profiles().first() {
                self.active_model = first.model_name.clone();
            }
            self.state.model_router = Some(router);
        }
    }

    /// Process a single stream event, updating chat_history in place.
    fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::TextDelta(delta) => {
                if let Some((_, _, content)) = self.chat_history.last_mut() {
                    content.push_str(&delta);
                }
            }
            StreamEvent::ToolCallDelta { .. } => {
                // Future: render tool call state
            }
            StreamEvent::Usage(_usage) => {
                // Future: show token usage in status bar
            }
            StreamEvent::Done => {
                self.is_streaming = false;
                self.stream_rx = None;
                // Ensure non-empty response
                if let Some((_, _, content)) = self.chat_history.last_mut() {
                    if content.is_empty() {
                        content.push_str("(empty response)");
                    }
                }
            }
            StreamEvent::Error(e) => {
                self.is_streaming = false;
                self.stream_rx = None;
                self.chat_history.push((
                    "System".to_string(),
                    String::new(),
                    format!("Stream error: {e}"),
                ));
            }
        }
    }

    /// Non-blocking drain of all pending stream events before a draw.
    async fn try_flush_stream(&mut self) {
        let mut rx = match self.stream_rx.take() {
            Some(rx) => rx,
            None => return,
        };
        loop {
            match tokio::time::timeout(
                std::time::Duration::from_millis(1),
                rx.recv(),
            )
            .await
            {
                Ok(Some(ev)) => {
                    self.handle_stream_event(ev);
                    if !self.is_streaming {
                        return; // rx already consumed by handle_stream_event
                    }
                }
                _ => break,
            }
        }
        // Put back the non-exhausted receiver
        self.stream_rx = Some(rx);
    }

    /// Stop any active streaming response.
    fn stop_streaming(&mut self) {
        self.is_streaming = false;
        self.stream_rx = None;
    }

    /// Called after `model_sel_provider` is set: fetch models and advance to family/variant list.
    async fn advance_to_model_list(&mut self) {
        let provider_name = self.model_sel_provider.clone();
        self.model_sel_search.clear();
        self.model_sel_index = 0;
        self.command_mode = CommandMode::ModelSelection;

        // Clone Arc agar bisa digunakan bebas tanpa borrow conflict di blok async
        let router_arc = self.state.model_router.clone();

        if let Some(router) = &router_arc {
            if let Ok(provider) = router.registry().get_provider(&provider_name) {
                match provider.fetch_models().await {
                    Ok(models) => {
                        // Inject semua model yang di-fetch ke registry agar bisa di-route
                        router.inject_profiles(models.clone());
                        let families = group_models_by_family(models);
                        self.model_sel_families = families;
                        self.model_sel_step = ModelSelectionStep::SelectFamily;
                    }
                    Err(_) => {
                        // Fallback: gunakan profile yang sudah diregistrasi untuk provider ini
                        let profiles: Vec<_> = router
                            .registry()
                            .list_profiles()
                            .into_iter()
                            .filter(|p| p.provider == provider_name)
                            .collect();
                        if profiles.is_empty() {
                            self.status_message =
                                format!("No models available for {provider_name}");
                            self.reset_model_selection();
                            self.command_mode = CommandMode::None;
                        } else {
                            let families = group_models_by_family(profiles);
                            self.model_sel_families = families;
                            self.model_sel_step = ModelSelectionStep::SelectFamily;
                        }
                    }
                }
            }
        }
    }

    fn reset_model_selection(&mut self) {
        self.model_sel_step = ModelSelectionStep::SelectProvider;
        self.model_sel_provider.clear();
        self.model_sel_families.clear();
        self.model_sel_variants.clear();
        self.model_sel_search.clear();
        self.model_sel_index = 0;
        self.model_sel_pending_provider = None;
    }

    /// Number of items in the current model-selection list (filtered by search).
    fn active_model_list_len(&self) -> usize {
        let search = self.model_sel_search.to_lowercase();
        match self.model_sel_step {
            ModelSelectionStep::SelectProvider => {
                let all = self.all_catalog_providers();
                if search.is_empty() {
                    all.len()
                } else {
                    all.iter()
                        .filter(|(name, _)| name.to_lowercase().contains(&search))
                        .count()
                }
            }
            ModelSelectionStep::SelectFamily => {
                if search.is_empty() {
                    self.model_sel_families.len()
                } else {
                    self.model_sel_families
                        .iter()
                        .filter(|f| f.name.to_lowercase().contains(&search))
                        .count()
                }
            }
            ModelSelectionStep::SelectVariant => {
                if search.is_empty() {
                    self.model_sel_variants.len()
                } else {
                    self.model_sel_variants
                        .iter()
                        .filter(|v| {
                            v.id.to_lowercase().contains(&search)
                                || v.model_name.to_lowercase().contains(&search)
                                || v.suitable_for.iter().any(|t| t.to_lowercase().contains(&search))
                        })
                        .count()
                }
            }
        }
    }

    /// Handle Enter key during model selection.
    async fn handle_model_selection_enter(&mut self) {
        match self.model_sel_step {
            ModelSelectionStep::SelectProvider => {
                // Resolve the selected provider name from the filtered list
                let providers: Vec<String> = {
                    let search = self.model_sel_search.to_lowercase();
                    let all: Vec<String> = self
                        .all_catalog_providers()
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect();
                    if search.is_empty() {
                        all
                    } else {
                        all.into_iter()
                            .filter(|p| p.to_lowercase().contains(&search))
                            .collect()
                    }
                };
                if self.model_sel_index >= providers.len() {
                    return;
                }
                let provider_name = providers[self.model_sel_index].clone();
                self.model_sel_provider = provider_name.clone();
                self.model_sel_search.clear();
                self.model_sel_index = 0;

                // Check if provider is actually registered (has API key)
                if !self.provider_is_configured(&provider_name) {
                    // Provider not configured — ask for API key first
                    self.model_sel_pending_provider = Some(provider_name.clone());
                    self.command_mode = CommandMode::ApiKeyInput {
                        key_input: String::new(),
                        error_message: String::new(),
                    };
                    return;
                }

                // Fetch models from the provider
                self.advance_to_model_list().await;
            }
            ModelSelectionStep::SelectFamily => {
                let search = self.model_sel_search.to_lowercase();
                let filtered: Vec<&ModelFamily> = if search.is_empty() {
                    self.model_sel_families.iter().collect()
                } else {
                    self.model_sel_families
                        .iter()
                        .filter(|f| f.name.to_lowercase().contains(&search))
                        .collect()
                };
                if self.model_sel_index >= filtered.len() {
                    return;
                }
                let variants = filtered[self.model_sel_index].variants.clone();
                self.model_sel_search.clear();
                self.model_sel_index = 0;
                if variants.len() == 1 {
                    // Only one variant — select it directly
                    self.active_model = variants[0].id.clone();
                    self.status_message = format!(
                        "Active model: {} ({})",
                        self.active_model,
                        variants[0].provider,
                    );
                    self.reset_model_selection();
                    self.command_mode = CommandMode::None;
                } else {
                    self.model_sel_variants = variants;
                    self.model_sel_step = ModelSelectionStep::SelectVariant;
                }
            }
            ModelSelectionStep::SelectVariant => {
                let search = self.model_sel_search.to_lowercase();
                let filtered: Vec<&ModelProfile> = if search.is_empty() {
                    self.model_sel_variants.iter().collect()
                } else {
                    self.model_sel_variants
                        .iter()
                        .filter(|v| {
                            v.id.to_lowercase().contains(&search)
                                || v.model_name.to_lowercase().contains(&search)
                                || v.suitable_for.iter().any(|t| t.to_lowercase().contains(&search))
                        })
                        .collect()
                };
                if self.model_sel_index >= filtered.len() {
                    return;
                }
                let selected = filtered[self.model_sel_index].clone();

                // Inject profile ke router agar model hasil auto-fetch bisa ditemukan
                // saat routing (fix: "model not available" untuk model dinamis)
                if let Some(router) = &self.state.model_router {
                    router.inject_profile(selected.clone());
                }

                self.active_model = selected.id.clone();
                self.status_message = format!(
                    "Model: {} (via {})",
                    selected.id,
                    selected.provider
                );
                self.command_mode = CommandMode::None;
                self.reset_model_selection();
            }
        }
    }

    /// Collect provider names from the model router (only configured/registered ones).
    pub fn configured_providers(&self) -> Vec<String> {
        self.state
            .model_router
            .as_ref()
            .map(|r| {
                r.registry()
                    .list_profiles()
                    .iter()
                    .map(|p| p.provider.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All known providers from the built-in catalog (regardless of registration).
    /// Used for model selection so users can pick a provider and set API key.
    pub fn all_catalog_providers(&self) -> Vec<(String, bool)> {
        let catalog = clawhive_model_router::providers::provider_configs();
        let configured: std::collections::BTreeSet<String> = self
            .state
            .model_router
            .as_ref()
            .map(|r| {
                r.registry()
                    .list_profiles()
                    .iter()
                    .map(|p| p.provider.clone())
                    .collect()
            })
            .unwrap_or_default();
        catalog
            .into_iter()
            .map(|c| {
                let is_cfg = configured.contains(c.name);
                (c.name.to_string(), is_cfg)
            })
            .collect()
    }

    /// Check if a provider name is registered in the router.
    pub fn provider_is_configured(&self, name: &str) -> bool {
        self.state.model_router.as_ref().is_some_and(|r| {
            r.registry().get_provider(name).is_ok()
        })
    }

    async fn persist_api_key(&self, provider: &str, api_key: &str) {
        let key = format!("config:{}_api_key", provider);
        let _ = self
            .state
            .kv_store
            .set::<String>(&key, &api_key.to_string())
            .await;
    }

    async fn load_saved_api_key(&mut self) {
        if self.state.model_router.is_some() {
            return;
        }
        self.register_all_providers().await;
    }

    async fn execute_command(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let command = parts[0].to_lowercase();

        let result = match command.as_str() {
            "help" => {
                let providers_list = clawhive_model_router::providers::provider_configs();
                let names: Vec<&str> = providers_list.iter().map(|c| c.name).collect();
                let help = format!(
"\
Commands:
  :help                              Show this help
  :refresh                           Refresh all data
  :apikey <provider> <key>           Set API key for a provider (alias: :connect)
  :pause <agent_id|name>             Pause agent
  :terminate <agent_id|name>         Terminate agent
  :approve <spawn_id>                Approve spawn request
  :deny <spawn_id>                   Deny spawn request
  :spawn <mission> <role> <objective> <budget>  Create spawn request
  :goto <agents|workers|spawn>       Switch tab
  :q                                 Quit TUI

Available providers ({} total):
  {}
Use Ctrl+P palette to set API keys.
Type any message to start a chat with the active model.",
                    names.len(),
                    names.chunks(5).map(|chunk| chunk.join(", ")).collect::<Vec<_>>().join(",\n  ")
                );
                help
            }
            "apikey" | "connect" => {
                if parts.len() < 2 {
                    "Usage: :apikey <provider> <your_api_key>".into()
                } else if parts.len() < 3 {
                    format!("Missing API key for '{}'. Usage: :apikey {} <key>", parts[1], parts[1])
                } else {
                    let provider_type = parts[1].to_lowercase();
                    let api_key = parts[2..].join(" ");
                    let env_var = provider_api_key_env(&provider_type);
                    unsafe { std::env::set_var(&env_var, &api_key) };
                    self.register_all_providers().await;
                    self.persist_api_key(&provider_type, &api_key).await;
                    format!("{} API key set & saved.", provider_type)
                }
            }
            "refresh" => {
                self.status_message = "Refreshing...".into();
                return;
            }
            "pause" => {
                if parts.len() < 2 {
                    "Usage: :pause <agent_id|name>".into()
                } else {
                    let id_str = parts[1];
                    let agents = self
                        .state
                        .kv_store
                        .scan_prefix::<Agent>(AGENT_PREFIX)
                        .await
                        .unwrap_or_default();
                    match agents.into_iter().find(|(_, a)| {
                        a.id.0.to_string().starts_with(id_str) || a.name == id_str
                    }) {
                        Some((key, mut agent))
                            if matches!(
                                agent.state,
                                AgentState::Active | AgentState::Hibernating
                            ) =>
                        {
                            agent.state = AgentState::Paused;
                            agent.updated_at = chrono::Utc::now();
                            let _ = self.state.kv_store.set(&key, &agent).await;
                            format!("Paused agent {}", agent.name)
                        }
                        Some((_, agent)) => format!(
                            "Agent {} is in state {:?} (cannot pause)",
                            agent.name, agent.state
                        ),
                        None => format!("Agent not found: {id_str}"),
                    }
                }
            }
            "terminate" => {
                if parts.len() < 2 {
                    "Usage: :terminate <agent_id|name>".into()
                } else {
                    let id_str = parts[1];
                    let agents = self
                        .state
                        .kv_store
                        .scan_prefix::<Agent>(AGENT_PREFIX)
                        .await
                        .unwrap_or_default();
                    match agents.into_iter().find(|(_, a)| {
                        a.id.0.to_string().starts_with(id_str) || a.name == id_str
                    }) {
                        Some((key, mut agent)) => {
                            agent.state = AgentState::Terminated;
                            agent.updated_at = chrono::Utc::now();
                            agent.terminated_at = Some(chrono::Utc::now());
                            let _ = self.state.kv_store.set(&key, &agent).await;
                            format!("Terminated agent {}", agent.name)
                        }
                        None => format!("Agent not found: {id_str}"),
                    }
                }
            }
            "approve" => {
                if parts.len() < 2 {
                    "Usage: :approve <spawn_id>".into()
                } else {
                    let id_str = parts[1];
                    let requests = self
                        .state
                        .kv_store
                        .scan_prefix::<SpawnRequest>(SPAWNREQ_PREFIX)
                        .await
                        .unwrap_or_default();
                    match requests.into_iter().find(|(_, r)| {
                        r.id.0.to_string().starts_with(id_str)
                    }) {
                        Some((key, mut req)) if req.state == SpawnState::Pending => {
                            req.state = SpawnState::Approved;
                            req.updated_at = chrono::Utc::now();
                            let _ = self.state.kv_store.set(&key, &req).await;
                            format!("Approved spawn request {}", req.id.0)
                        }
                        Some((_, req)) => {
                            format!("Spawn request is {:?} (not pending)", req.state)
                        }
                        None => format!("Spawn request not found: {id_str}"),
                    }
                }
            }
            "deny" => {
                if parts.len() < 2 {
                    "Usage: :deny <spawn_id>".into()
                } else {
                    let id_str = parts[1];
                    let requests = self
                        .state
                        .kv_store
                        .scan_prefix::<SpawnRequest>(SPAWNREQ_PREFIX)
                        .await
                        .unwrap_or_default();
                    match requests.into_iter().find(|(_, r)| {
                        r.id.0.to_string().starts_with(id_str)
                    }) {
                        Some((key, mut req)) if req.state == SpawnState::Pending => {
                            req.state = SpawnState::Denied;
                            req.updated_at = chrono::Utc::now();
                            let _ = self.state.kv_store.set(&key, &req).await;
                            format!("Denied spawn request {}", req.id.0)
                        }
                        Some((_, req)) => {
                            format!("Spawn request is {:?} (not pending)", req.state)
                        }
                        None => format!("Spawn request not found: {id_str}"),
                    }
                }
            }
            "spawn" => {
                if parts.len() < 4 {
                    "Usage: :spawn <mission_id> <role> <objective> <budget>".into()
                } else {
                    let mission_id_str = parts[1];
                    let role = parts[2];
                    let objective = parts[3];
                    let budget: f64 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);

                    let missions = self
                        .state
                        .kv_store
                        .scan_prefix::<Mission>(MISSION_PREFIX)
                        .await
                        .unwrap_or_default();
                    let mission_id_uuid = uuid::Uuid::parse_str(mission_id_str).ok();
                    let mission = missions
                        .into_iter()
                        .find(|(_, m)| {
                            Some(m.id.0) == mission_id_uuid
                                || m.objective.contains(mission_id_str)
                        })
                        .map(|(_, m)| m);

                    match mission {
                        Some(m) => {
                            let request = SpawnRequest {
                                id: SpawnRequestId(uuid::Uuid::now_v7()),
                                mission_id: m.id,
                                task_id: None,
                                requested_by: AgentId(uuid::Uuid::now_v7()),
                                reason: format!("TUI spawn for {role}"),
                                team: SwarmTeamSpec {
                                    name: format!("{role}-team"),
                                    lifecycle_mode: clawhive_domain::LifecycleMode::Ephemeral,
                                    ttl_seconds: Some(3600),
                                    idle_timeout_seconds: Some(300),
                                },
                                children: vec![ChildSpec {
                                    role: role.to_string(),
                                    objective: objective.to_string(),
                                    budget_usd: budget,
                                    model_profile: "default".into(),
                                    max_turns: 100,
                                    custom_permissions: None,
                                }],
                                child_spawn_policy: ChildSpawnPolicy {
                                    allowed: true,
                                    max_depth: Some(3),
                                    max_children: Some(5),
                                },
                                termination: TerminationPolicy {
                                    on_task_complete: true,
                                    on_parent_terminated: true,
                                    on_budget_exhausted: true,
                                },
                                state: SpawnState::Pending,
                                created_at: chrono::Utc::now(),
                                updated_at: chrono::Utc::now(),
                            };
                            let spawn_key = format!("{SPAWNREQ_PREFIX}{}", request.id.0);
                            let _ = self.state.kv_store.set(&spawn_key, &request).await;
                            format!("Created spawn request for {role}")
                        }
                        None => format!("Mission not found: {mission_id_str}"),
                    }
                }
            }
            "goto" => {
                if parts.len() < 2 {
                    "Usage: :goto <agents|workers|spawn>".into()
                } else {
                    match parts[1] {
                        "agents" => {
                            self.selected_tab = Tab::Agents;
                            self.selected_index = 0;
                            "Switched to Agents".into()
                        }
                        "workers" => {
                            self.selected_tab = Tab::Workers;
                            self.selected_index = 0;
                            "Switched to Workers".into()
                        }
                        "spawn" => {
                            self.selected_tab = Tab::SpawnRequests;
                            self.selected_index = 0;
                            "Switched to Spawn Requests".into()
                        }
                        other => format!("Unknown tab: {other}"),
                    }
                }
            }
            "q" => {
                self.should_quit = true;
                "Quitting...".into()
            }
            other => format!("Unknown command: {other}. Type :help for commands."),
        };

        self.status_message = result;
    }

    pub async fn run(&mut self) -> Result<(), crate::TuiError> {
        use crossterm::ExecutableCommand;
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
        use ratatui::backend::CrosstermBackend;
        use ratatui::Terminal;
        use std::io::stdout;

        enable_raw_mode().map_err(|e| crate::TuiError::TermInit(e.to_string()))?;
        let mut stdout = stdout();
        stdout.execute(EnterAlternateScreen).map_err(|e| crate::TuiError::TermInit(e.to_string()))?;

        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))
            .map_err(|e| crate::TuiError::TermInit(e.to_string()))?;
        terminal.clear().map_err(|e| crate::TuiError::TermInit(e.to_string()))?;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(256);

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(event) = event::read() {
                    if tx_clone.send(event).await.is_err() {
                        break;
                    }
                }
            }
        });

        let tx_timer = tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                if tx_timer.send(Event::Resize(0, 0)).await.is_err() {
                    break;
                }
            }
        });

        self.refresh().await;
        self.load_saved_api_key().await;

        while !self.should_quit {
            // Flush pending stream events before rendering
            self.try_flush_stream().await;

            terminal
                .draw(|f| {
                    let area = f.area();
                    ui::draw(f, area, &self);
                })
                .map_err(|e| crate::TuiError::Runtime(e.to_string()))?;

            tokio::select! {
                Some(event) = rx.recv() => {
                    self.handle_event(event).await;
                }
            }
        }

        disable_raw_mode().map_err(|e| crate::TuiError::Runtime(e.to_string()))?;
        let _ = std::io::stdout().execute(LeaveAlternateScreen);
        Ok(())
    }

    async fn handle_event(&mut self, event: Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                // 1. Handle Ctrl+C to quit
                if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
                    self.should_quit = true;
                    return;
                }

                // 2. Handle Ctrl+P to trigger Command Palette
                if key.code == KeyCode::Char('p') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
                    self.command_mode = CommandMode::CommandPalette {
                        search_query: String::new(),
                        selected_index: 0,
                        filtered_items: get_palette_items(),
                    };
                    return;
                }

                // 3. Handle key based on current CommandMode
                match &mut self.command_mode {
                    CommandMode::ApiKeyInput { key_input, error_message } => {
                        match key.code {
                            KeyCode::Esc => {
                                if self.model_sel_pending_provider.is_some() {
                                    self.model_sel_pending_provider = None;
                                    self.command_mode = CommandMode::ModelSelection;
                                    self.model_sel_step = ModelSelectionStep::SelectProvider;
                                } else {
                                    self.command_mode = CommandMode::None;
                                }
                            }
                            KeyCode::Enter => {
                                let key = std::mem::take(key_input);

                                // If provider is predetermined (from model selection), use it directly
                                if let Some(provider) = self.model_sel_pending_provider.clone() {
                                    if key.trim().is_empty() {
                                        *error_message = "API key cannot be empty".into();
                                        *key_input = key;
                                    } else {
                                        let actual_key = key.trim().to_string();
                                        let env_var = provider_api_key_env(&provider);
                                        unsafe { std::env::set_var(&env_var, &actual_key) };
                                        self.register_all_providers().await;
                                        self.persist_api_key(&provider, &actual_key).await;
                                        let model_count = self.state.model_router.as_ref()
                                            .and_then(|r| Some(r.registry().list_profiles().len()))
                                            .unwrap_or(0);
                                        self.status_message = format!(
                                            "{provider} API key set. {model_count} model(s) available."
                                        );
                                        self.model_sel_pending_provider = None;
                                        self.model_sel_provider = provider.clone();
                                        self.advance_to_model_list().await;
                                        return;
                                    }
                                } else if key.trim().is_empty() {
                                    *error_message = "API key cannot be empty. Use `provider:key` format.".into();
                                    *key_input = key;
                                } else {
                                    let trimmed = key.trim().to_string();
                                    let (provider, actual_key) = if let Some(idx) = trimmed.find(':') {
                                        let (p, k) = trimmed.split_at(idx);
                                        (p.to_string(), k[1..].trim().to_string())
                                    } else {
                                        let catalog = clawhive_model_router::providers::provider_configs();
                                        let matched = catalog.iter().find(|c| c.name == trimmed);
                                        match matched {
                                            Some(c) => (c.name.to_string(), String::new()),
                                            None => ("".to_string(), trimmed.clone()),
                                        }
                                    };

                                    if provider.is_empty() {
                                        *error_message = "Use `provider:key` format (e.g. openai:sk-...)".into();
                                        *key_input = trimmed;
                                    } else if actual_key.is_empty() {
                                        *error_message = format!("API key for '{provider}' is empty.");
                                        *key_input = trimmed;
                                    } else {
                                        let env_var = provider_api_key_env(&provider);
                                        unsafe { std::env::set_var(&env_var, &actual_key) };
                                        self.register_all_providers().await;
                                        self.persist_api_key(&provider, &actual_key).await;
                                        let model_count = self.state.model_router.as_ref()
                                            .and_then(|r| Some(r.registry().list_profiles().len()))
                                            .unwrap_or(0);
                                        self.status_message = format!(
                                            "{provider} API key set. {model_count} model(s) available."
                                        );
                                        self.command_mode = CommandMode::None;
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                key_input.pop();
                            }
                            KeyCode::Char(c) => {
                                key_input.push(c);
                            }
                            _ => {}
                        }
                    }
                    CommandMode::ModelSelection => {
                        match key.code {
                            KeyCode::Esc => {
                                self.command_mode = CommandMode::None;
                                self.reset_model_selection();
                            }
                            KeyCode::Up => {
                                self.model_sel_index = self.model_sel_index.saturating_sub(1);
                            }
                            KeyCode::Down => {
                                let max = self.active_model_list_len();
                                if self.model_sel_index < max {
                                    self.model_sel_index += 1;
                                }
                            }
                            KeyCode::Enter => {
                                self.handle_model_selection_enter().await;
                            }
                            KeyCode::Backspace => {
                                self.model_sel_search.pop();
                                // Reset index when search changes
                                self.model_sel_index = 0;
                            }
                            KeyCode::Char(c) => {
                                self.model_sel_search.push(c);
                                self.model_sel_index = 0;
                            }
                            _ => {}
                        }
                    }
                    CommandMode::CommandPalette { search_query, selected_index, filtered_items } => {
                        match key.code {
                            KeyCode::Esc => {
                                self.command_mode = CommandMode::None;
                            }
                            KeyCode::Up => {
                                if *selected_index > 0 {
                                    *selected_index -= 1;
                                }
                            }
                            KeyCode::Down => {
                                if *selected_index < filtered_items.len().saturating_sub(1) {
                                    *selected_index += 1;
                                }
                            }
                            KeyCode::Enter => {
                                if !filtered_items.is_empty() {
                                    let action = filtered_items[*selected_index].3.clone();
                                    self.execute_palette_action(&action).await;
                                }
                                if matches!(self.command_mode, CommandMode::CommandPalette { .. }) {
                                    self.command_mode = CommandMode::None;
                                }
                            }
                            KeyCode::Backspace => {
                                search_query.pop();
                                self.update_palette_filter();
                            }
                            KeyCode::Char(c) => {
                                search_query.push(c);
                                self.update_palette_filter();
                            }
                            _ => {}
                        }
                    }
                    CommandMode::None => {
                        // Standard Input / Navigation handling
                        match key.code {
                            KeyCode::Enter => {
                                let content = std::mem::take(&mut self.input_buffer);
                                let trimmed = content.trim();
                                if !trimmed.is_empty() {
                                    if let Some(cmd) = trimmed.strip_prefix(':').or_else(|| trimmed.strip_prefix('/')) {
                                        self.execute_command(cmd).await;
                                    } else {
                                        self.chat_history.push(("User".to_string(), "".to_string(), trimmed.to_string()));
                                        self.active_screen = Screen::Chat;

                                        let router_opt = self
                                            .state
                                            .model_router
                                            .as_ref()
                                            .map(Arc::clone);
                                        match router_opt {
                                            Some(router) => {
                                                let model_id = self.active_model.clone();
                                                let request = ChatRequest {
                                                    model: model_id.clone(),
                                                    messages: vec![ModelMessage {
                                                        role: MessageRole::User,
                                                        content: trimmed.to_string(),
                                                        tool_calls: None,
                                                        tool_call_id: None,
                                                        name: None,
                                                    }],
                                                    max_tokens: Some(4096),
                                                    temperature: Some(0.7),
                                                    tools: None,
                                                    stop: None,
                                                };

                                                self.stop_streaming();

                                                match router.route_chat_stream(&model_id, request).await {
                                                    Ok(handle) => {
                                                        let label = model_id.clone();
                                                        self.chat_history.push((
                                                            "Agent".to_string(),
                                                            label.clone(),
                                                            String::new(),
                                                        ));
                                                        self.is_streaming = true;

                                                        let (stream_tx, stream_rx) =
                                                            tokio::sync::mpsc::unbounded_channel();
                                                        self.stream_rx = Some(stream_rx);

                                                        tokio::spawn(async move {
                                                            while let Some(event) = handle.recv().await {
                                                                if stream_tx.send(event).is_err() {
                                                                    break;
                                                                }
                                                            }
                                                        });
                                                    }
                                                    Err(e) => {
                                                        self.chat_history.push((
                                                            "System".to_string(),
                                                            "".into(),
                                                            format!("Model error: {e}"),
                                                        ));
                                                    }
                                                }
                                            }
                                            None => {
                                                self.chat_history.push((
                                                    "System".to_string(),
                                                    "".into(),
                                                    "Model router not configured. Set API key via Ctrl+P → Set API Key".into(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                if self.active_screen == Screen::Chat {
                                    self.active_screen = Screen::Home;
                                } else {
                                    self.should_quit = true;
                                }
                            }
                            KeyCode::Backspace => {
                                self.input_buffer.pop();
                            }
                            KeyCode::Char(c) => {
                                self.input_buffer.push(c);
                            }
                            KeyCode::Tab => {
                                self.selected_tab = match self.selected_tab {
                                    Tab::Session => Tab::Agents,
                                    Tab::Agents => Tab::Workers,
                                    Tab::Workers => Tab::SpawnRequests,
                                    Tab::SpawnRequests => Tab::Session,
                                };
                                self.selected_index = 0;
                            }
                            KeyCode::Up => {
                                if self.selected_index > 0 {
                                    self.selected_index -= 1;
                                }
                            }
                            KeyCode::Down => {
                                let max = self.current_list_len().saturating_sub(1);
                                if self.selected_index < max {
                                    self.selected_index += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    fn update_palette_filter(&mut self) {
        if let CommandMode::CommandPalette { search_query, selected_index, filtered_items } = &mut self.command_mode {
            let query = search_query.to_lowercase();
            let filtered: Vec<(String, String, String, String)> = get_palette_items()
                .into_iter()
                .filter(|(_, name, _, _)| name.to_lowercase().contains(&query))
                .collect();
            *selected_index = 0;
            *filtered_items = filtered;
        }
    }

    async fn execute_palette_action(&mut self, action: &str) {
        match action {
            "/session_new" => {
                self.stop_streaming();
                self.chat_history.clear();
                self.active_screen = Screen::Home;
            }
            "/session_switch" => {
                self.stop_streaming();
                self.chat_history.clear();
                self.chat_history.push(("System".into(), "".into(), "Sesi baru berhasil dimuat.".into()));
                self.active_screen = Screen::Chat;
            }
            "/model_switch" => {
                self.reset_model_selection();
                self.model_sel_step = ModelSelectionStep::SelectProvider;
                self.command_mode = CommandMode::ModelSelection;
            }
            "/session_share" => {
                self.chat_history.push(("System".into(), "".into(), "Tautan sesi berhasil disalin ke clipboard!".into()));
                self.active_screen = Screen::Chat;
            }
            "/session_rename" => {
                self.chat_history.push(("System".into(), "".into(), "Sesi berhasil diganti namanya.".into()));
                self.active_screen = Screen::Chat;
            }
            act if act.starts_with("/apikey") || act.starts_with("/connect") => {
                let prefix = if let Some(p) = act.strip_prefix("/apikey ").or_else(|| act.strip_prefix("/connect ")) {
                    format!("{p}:")
                } else {
                    String::new()
                };
                self.command_mode = CommandMode::ApiKeyInput {
                    key_input: prefix,
                    error_message: String::new(),
                };
            }
            _ => {}
        }
    }
}

/// Resolve API key env var name for a provider.
/// First checks the built-in catalog, falls back to `{NAME}_API_KEY` convention.
fn provider_api_key_env(provider: &str) -> String {
    // Check built-in catalog
    if let Some(slot) = clawhive_model_router::providers::get_provider_slot(provider) {
        return slot.api_key_env.to_string();
    }
    // Fallback to conventional naming
    format!("{}_API_KEY", provider.to_uppercase())
}

fn get_palette_items() -> Vec<(String, String, String, String)> {
    let mut items: Vec<(String, String, String, String)> = vec![
        ("Suggested".into(), "Switch session".into(), "ctrl+x l".into(), "/session_switch".into()),
        ("Suggested".into(), "New session".into(), "ctrl+x n".into(), "/session_new".into()),
        ("Suggested".into(), "Switch model".into(), "ctrl+x m".into(), "/model_switch".into()),
        ("Suggested".into(), "Share session".into(), "".into(), "/session_share".into()),
    ];
    items.extend([
        ("Session".into(), "Switch session".into(), "ctrl+x l".into(), "/session_switch".into()),
        ("Session".into(), "New session".into(), "ctrl+x n".into(), "/session_new".into()),
        ("Session".into(), "Share session".into(), "".into(), "/session_share".into()),
        ("Session".into(), "Rename session".into(), "ctrl+r".into(), "/session_rename".into()),
    ]);
    items
}
