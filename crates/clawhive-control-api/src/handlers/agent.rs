use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use clawhive_domain::{Agent, AgentState};
use clawhive_lifecycle::LifecycleService;
use clawhive_store::StoreExt;

use crate::error::ApiError;
use crate::state::AppState;
use crate::store::AGENT_PREFIX;

#[derive(Serialize)]
pub struct AgentResponse {
    pub id: String,
    pub name: String,
    pub role: String,
    pub state: String,
    pub lifecycle_mode: String,
    pub parent_agent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct AgentsQuery {
    pub state: Option<String>,
    pub mission_id: Option<String>,
}

/// GET /v1/agents
pub async fn list_agents(
    State(state): State<AppState>,
    Query(query): Query<AgentsQuery>,
) -> Result<Json<Vec<AgentResponse>>, ApiError> {
    let agents = state
        .kv_store
        .scan_prefix::<Agent>(AGENT_PREFIX)
        .await?
        .into_iter()
        .map(|(_, a)| a)
        .filter(|a| {
            if let Some(ref s) = query.state {
                let q = s.to_lowercase();
                let state_str = format!("{:?}", a.state).to_lowercase();
                if state_str != q {
                    return false;
                }
            }
            if let Some(ref m) = query.mission_id
                && a.mission_id.0.to_string() != *m
            {
                return false;
            }
            true
        })
        .map(|a| AgentResponse {
            id: a.id.0.to_string(),
            name: a.name,
            role: a.role,
            state: format!("{:?}", a.state),
            lifecycle_mode: format!("{:?}", a.lifecycle_mode),
            parent_agent_id: a.parent_agent_id.map(|id| id.0.to_string()),
        })
        .collect();

    Ok(Json(agents))
}

/// GET /v1/agents/{id}
pub async fn get_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentResponse>, ApiError> {
    let key = format!("{AGENT_PREFIX}{id}");
    let agent = state
        .kv_store
        .get::<Agent>(&key)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;

    Ok(Json(AgentResponse {
        id: agent.id.0.to_string(),
        name: agent.name,
        role: agent.role,
        state: format!("{:?}", agent.state),
        lifecycle_mode: format!("{:?}", agent.lifecycle_mode),
        parent_agent_id: agent.parent_agent_id.map(|id| id.0.to_string()),
    }))
}

/// POST /v1/agents/{id}/pause
pub async fn pause_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentResponse>, ApiError> {
    let key = format!("{AGENT_PREFIX}{id}");
    let mut agent = state
        .kv_store
        .get::<Agent>(&key)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;

    if !matches!(agent.state, AgentState::Active | AgentState::Hibernating) {
        return Err(ApiError::Validation(format!(
            "cannot pause agent in state {:?}",
            agent.state
        )));
    }

    agent.state = AgentState::Paused;
    agent.updated_at = chrono::Utc::now();
    state.kv_store.set(&key, &agent).await?;

    let _ = state.telemetry.record("agent.paused", "success", |e| {
        e.with_agent_id(agent.id.0.to_string())
            .with_mission_id(agent.mission_id.0.to_string())
    });

    Ok(Json(AgentResponse {
        id: agent.id.0.to_string(),
        name: agent.name,
        role: agent.role,
        state: format!("{:?}", agent.state),
        lifecycle_mode: format!("{:?}", agent.lifecycle_mode),
        parent_agent_id: agent.parent_agent_id.map(|id| id.0.to_string()),
    }))
}

/// POST /v1/agents/{id}/terminate
pub async fn terminate_agent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentResponse>, ApiError> {
    let key = format!("{AGENT_PREFIX}{id}");

    let _ = state
        .kv_store
        .get::<Agent>(&key)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;

    let all_agents: Vec<Agent> = state
        .kv_store
        .scan_prefix::<Agent>(AGENT_PREFIX)
        .await?
        .into_iter()
        .map(|(_, a)| a)
        .collect();

    let children_ids: Vec<Uuid> = all_agents
        .iter()
        .filter(|a| a.parent_agent_id.as_ref().map(|p| p.0) == Some(id))
        .map(|a| a.id.0)
        .collect();

    for child_id in &children_ids {
        let child_key = format!("{AGENT_PREFIX}{child_id}");
        if let Some(mut child) = state.kv_store.get::<Agent>(&child_key).await? {
            LifecycleService::terminate_descendant(&mut child);
            state.kv_store.set(&child_key, &child).await?;
        }
    }

    let mut agent = state
        .kv_store
        .get::<Agent>(&key)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id}")))?;
    LifecycleService::terminate(&mut agent);
    state.kv_store.set(&key, &agent).await?;

    let _ = state.telemetry.record("agent.terminated", "success", |e| {
        e.with_agent_id(agent.id.0.to_string())
            .with_mission_id(agent.mission_id.0.to_string())
    });

    Ok(Json(AgentResponse {
        id: agent.id.0.to_string(),
        name: agent.name,
        role: agent.role,
        state: format!("{:?}", agent.state),
        lifecycle_mode: format!("{:?}", agent.lifecycle_mode),
        parent_agent_id: agent.parent_agent_id.map(|id| id.0.to_string()),
    }))
}
