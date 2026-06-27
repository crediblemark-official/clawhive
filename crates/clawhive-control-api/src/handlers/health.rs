use axum::Json;
use serde_json::{Value, json};

pub async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "clawhive-control-api",
        "version": "0.1.0"
    }))
}
