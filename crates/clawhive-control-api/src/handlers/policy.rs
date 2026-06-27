use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use clawhive_domain::{Agent, PolicyBundleId, PolicySubject};
use clawhive_policy::PolicyService;
use clawhive_store::StoreExt;

use crate::error::ApiError;
use crate::state::AppState;
use crate::store::AGENT_PREFIX;

#[derive(Serialize)]
pub struct PolicyResponse {
    pub result: String,
}

#[derive(Deserialize)]
pub struct CompilePolicyRequest {
    pub source: String,
}

#[derive(Deserialize)]
pub struct EvaluatePolicyRequest {
    pub bundle_id: String,
    pub subject: PolicySubject,
    pub action: String,
    pub resource: String,
    pub context: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct EvaluatePolicyResponse {
    pub allowed: bool,
    pub matched_rule: Option<String>,
    pub evaluation_time_ms: u64,
    pub reason: String,
}

/// POST /v1/policies/compile
pub async fn compile_policy(
    State(_state): State<AppState>,
    Json(req): Json<CompilePolicyRequest>,
) -> Json<PolicyResponse> {
    Json(PolicyResponse {
        result: format!("compiled: {} rules", req.source.len()),
    })
}

/// POST /v1/policies/simulate
pub async fn simulate_policy(
    State(_state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<PolicyResponse> {
    Json(PolicyResponse {
        result: format!("simulated: {req}"),
    })
}

/// POST /v1/policies/evaluate
pub async fn evaluate_policy(
    State(state): State<AppState>,
    Json(req): Json<EvaluatePolicyRequest>,
) -> Result<Json<EvaluatePolicyResponse>, ApiError> {
    let bundle = state
        .kv_store
        .scan_prefix::<Agent>(AGENT_PREFIX)
        .await?
        .into_iter()
        .find_map(|(_, a)| {
            if a.policy_bundle.id == PolicyBundleId(Uuid::parse_str(&req.bundle_id).unwrap()) {
                Some(a.policy_bundle)
            } else {
                None
            }
        })
        .ok_or_else(|| ApiError::NotFound(format!("policy bundle {}", req.bundle_id)))?;

    let result = PolicyService::evaluate(
        &bundle,
        &req.subject,
        &req.action,
        &req.resource,
        req.context.as_ref(),
    )
    .map_err(|e| ApiError::Validation(e.to_string()))?;

    let _ = state.telemetry.record("policy.evaluated", if result.allowed { "allowed" } else { "denied" }, |e| {
        e.with_additional("action".into(), req.action.clone())
            .with_additional("resource".into(), req.resource.clone())
            .with_additional("subject".into(), format!("{:?}", req.subject))
    });

    Ok(Json(EvaluatePolicyResponse {
        allowed: result.allowed,
        matched_rule: result.matched_rule.map(|r| r.id.0.to_string()),
        evaluation_time_ms: result.evaluation_time_ms,
        reason: result.reason,
    }))
}
