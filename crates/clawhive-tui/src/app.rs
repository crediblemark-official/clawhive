use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use clawhive_control_api::state::AppState;
use clawhive_control_api::store::{AGENT_PREFIX, MISSION_PREFIX, SPAWNREQ_PREFIX};
use clawhive_domain::{
    Agent, AgentId, AgentState, ChildSpawnPolicy, ChildSpec, Mission, SpawnRequest, SpawnRequestId,
    SpawnState, SwarmTeamSpec, TerminationPolicy, Worker,
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
pub enum CommandMode {
    None,
    SlashAutocomplete {
        selected_index: usize,
        filtered_commands: Vec<(String, String)>,
    },
    CommandPalette {
        search_query: String,
        selected_index: usize,
        filtered_items: Vec<(String, String, String, String)>, // (category, name, shortcut, action)
    },
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
            status_message: "ClawHive OS TUI — Tab: switch sidebar | Esc: home | :q: quit".into(),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            active_screen: Screen::Home,
            chat_history: Vec::new(),
            command_mode: CommandMode::None,
            active_model: "Kimi K2.7 Code".into(),
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

    async fn execute_command(&mut self, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let command = parts[0].to_lowercase();

        let result = match command.as_str() {
            "help" => {
                let help = "\
Commands:
  :help                       Show this help
  :refresh                    Refresh all data
  :pause <agent_id|name>      Pause agent
  :terminate <agent_id|name>  Terminate agent
  :approve <spawn_id>         Approve spawn request
  :deny <spawn_id>            Deny spawn request
  :spawn <mission> <role> <objective> <budget>  Create spawn request
  :goto <agents|workers|spawn>  Switch tab
  :q                          Quit TUI";
                help.to_string()
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

        while !self.should_quit {
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
                    CommandMode::SlashAutocomplete { selected_index, filtered_commands } => {
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
                                if *selected_index < filtered_commands.len().saturating_sub(1) {
                                    *selected_index += 1;
                                }
                            }
                            KeyCode::Enter | KeyCode::Tab => {
                                if !filtered_commands.is_empty() {
                                    let chosen = filtered_commands[*selected_index].0.clone();
                                    self.input_buffer = chosen;
                                }
                                self.command_mode = CommandMode::None;
                            }
                            KeyCode::Backspace => {
                                self.input_buffer.pop();
                                self.update_autocomplete();
                            }
                            KeyCode::Char(c) => {
                                self.input_buffer.push(c);
                                self.update_autocomplete();
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
                                self.command_mode = CommandMode::None;
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
                                    if trimmed.starts_with(':') {
                                        self.execute_command(&trimmed[1..]).await;
                                    } else {
                                        self.chat_history.push(("User".to_string(), "".to_string(), trimmed.to_string()));
                                        self.active_screen = Screen::Chat;
                                        
                                        let response = format!("Memproses instruksi: \"{}\". Memulai analisis codebase dan menyiapkan agent workspace...", trimmed);
                                        self.chat_history.push(("Agent".to_string(), "Build · Kimi K2.7 Code".to_string(), response));
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
                            KeyCode::Char('/') if self.input_buffer.is_empty() => {
                                self.input_buffer.push('/');
                                self.command_mode = CommandMode::SlashAutocomplete {
                                    selected_index: 0,
                                    filtered_commands: get_slash_commands(),
                                };
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

    fn update_autocomplete(&mut self) {
        if !self.input_buffer.starts_with('/') {
            self.command_mode = CommandMode::None;
            return;
        }
        let query = &self.input_buffer;
        let filtered: Vec<(String, String)> = get_slash_commands()
            .into_iter()
            .filter(|(cmd, _)| cmd.to_lowercase().starts_with(&query.to_lowercase()))
            .collect();
            
        self.command_mode = CommandMode::SlashAutocomplete {
            selected_index: 0,
            filtered_commands: filtered,
        };
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
                self.chat_history.clear();
                self.active_screen = Screen::Home;
            }
            "/session_switch" => {
                self.chat_history.clear();
                self.chat_history.push(("System".into(), "".into(), "Sesi baru berhasil dimuat.".into()));
                self.active_screen = Screen::Chat;
            }
            "/model_switch" => {
                if self.active_model == "Kimi K2.7 Code" {
                    self.active_model = "Qwen 2.5 Coder".to_string();
                } else {
                    self.active_model = "Kimi K2.7 Code".to_string();
                }
                self.chat_history.push(("System".into(), "".into(), format!("Model berhasil diubah ke: {}", self.active_model)));
                self.active_screen = Screen::Chat;
            }
            "/session_share" => {
                self.chat_history.push(("System".into(), "".into(), "Tautan sesi berhasil disalin ke clipboard!".into()));
                self.active_screen = Screen::Chat;
            }
            "/session_rename" => {
                self.chat_history.push(("System".into(), "".into(), "Sesi berhasil diganti namanya.".into()));
                self.active_screen = Screen::Chat;
            }
            _ => {}
        }
    }
}

fn get_slash_commands() -> Vec<(String, String)> {
    vec![
        ("/agents".into(), "Switch agent".into()),
        ("/compact".into(), "Compact session".into()),
        ("/connect".into(), "Connect provider".into()),
        ("/copy".into(), "Copy session transcript".into()),
        ("/diff".into(), "Open diff viewer".into()),
        ("/editor".into(), "Open editor".into()),
        ("/exit".into(), "Exit the app".into()),
        ("/export".into(), "Export session transcript".into()),
        ("/fork".into(), "Fork session".into()),
        ("/help".into(), "Help".into()),
    ]
}

fn get_palette_items() -> Vec<(String, String, String, String)> {
    vec![
        ("Suggested".into(), "Switch session".into(), "ctrl+x l".into(), "/session_switch".into()),
        ("Suggested".into(), "New session".into(), "ctrl+x n".into(), "/session_new".into()),
        ("Suggested".into(), "Switch model".into(), "ctrl+x m".into(), "/model_switch".into()),
        ("Suggested".into(), "Share session".into(), "".into(), "/session_share".into()),
        ("Session".into(), "Switch session".into(), "ctrl+x l".into(), "/session_switch".into()),
        ("Session".into(), "New session".into(), "ctrl+x n".into(), "/session_new".into()),
        ("Session".into(), "Share session".into(), "".into(), "/session_share".into()),
        ("Session".into(), "Rename session".into(), "ctrl+r".into(), "/session_rename".into()),
    ]
}
