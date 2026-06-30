use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use claw10_domain::{AgentId, Schedule, ScheduleAction};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct AddScheduleRequest {
    pub cron: String,
    pub timezone: String,
    pub action: ScheduleAction,
}

#[derive(Serialize)]
pub struct ScheduleResponse {
    pub index: usize,
    pub cron: String,
    pub timezone: String,
    pub action: String,
}

#[derive(Serialize)]
pub struct DueScheduleResponse {
    pub agent_id: String,
    pub schedule: ScheduleResponse,
}

/// POST /v1/agents/{agent_id}/schedules
pub async fn add_schedule(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    Json(body): Json<AddScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), ApiError> {
    let action_str = format!("{:?}", body.action);
    let schedule = Schedule {
        cron: body.cron,
        timezone: body.timezone,
        action: body.action,
    };

    let schedules = state
        .scheduler_service
        .list_schedules(&AgentId(agent_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let index = schedules.len();

    state
        .scheduler_service
        .add_schedule(&AgentId(agent_id), schedule)
        .await
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let _ = state.telemetry.record("schedule.added", "success", |e| {
        e.with_agent_id(agent_id.to_string())
            .with_additional("action".into(), action_str)
    });

    let schedules = state
        .scheduler_service
        .list_schedules(&AgentId(agent_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let sched = &schedules[index];

    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            index,
            cron: sched.cron.clone(),
            timezone: sched.timezone.clone(),
            action: format!("{:?}", sched.action),
        }),
    ))
}

/// GET /v1/agents/{agent_id}/schedules
pub async fn list_schedules(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Vec<ScheduleResponse>>, ApiError> {
    let schedules = state
        .scheduler_service
        .list_schedules(&AgentId(agent_id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(
        schedules
            .into_iter()
            .enumerate()
            .map(|(i, s)| ScheduleResponse {
                index: i,
                cron: s.cron,
                timezone: s.timezone,
                action: format!("{:?}", s.action),
            })
            .collect(),
    ))
}

/// DELETE /v1/agents/{agent_id}/schedules/{index}
pub async fn remove_schedule(
    State(state): State<AppState>,
    Path((agent_id, index)): Path<(Uuid, usize)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .scheduler_service
        .remove_schedule(&AgentId(agent_id), index)
        .await
        .map_err(|e| ApiError::NotFound(e.to_string()))?;

    let _ = state.telemetry.record("schedule.removed", "success", |e| {
        e.with_agent_id(agent_id.to_string())
            .with_additional("schedule_index".into(), index.to_string())
    });

    Ok(Json(serde_json::json!({ "status": "removed" })))
}

/// GET /v1/schedules/due
pub async fn get_due_schedules(
    State(state): State<AppState>,
) -> Result<Json<Vec<DueScheduleResponse>>, ApiError> {
    let due = state
        .scheduler_service
        .get_due_schedules(&chrono::Utc::now())
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(
        due.into_iter()
            .map(|ds| DueScheduleResponse {
                agent_id: ds.agent_id.0.to_string(),
                schedule: ScheduleResponse {
                    index: 0,
                    cron: ds.schedule.cron,
                    timezone: ds.schedule.timezone,
                    action: format!("{:?}", ds.schedule.action),
                },
            })
            .collect(),
    ))
}
