use std::sync::Arc;

use chrono::Utc;

use clawhive_domain::{AuditEvent, AuditEventId};
use clawhive_store::{Store, StoreError, StoreExt};

const KEY_PREFIX: &str = "audit:";

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("{0}")]
    Store(#[from] StoreError),
}

pub struct AuditService {
    store: Arc<dyn Store>,
}

impl AuditService {
    #[must_use]
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    pub async fn emit_event(&self, event: AuditEvent) -> Result<AuditEvent, AuditError> {
        let mut event = event;
        let key = format!("{KEY_PREFIX}{}", event.id.0);
        event.timestamp = Utc::now();
        self.store.set(&key, &event).await?;
        Ok(event)
    }

    pub async fn get_event(&self, id: &AuditEventId) -> Result<Option<AuditEvent>, AuditError> {
        let key = format!("{KEY_PREFIX}{}", id.0);
        Ok(self.store.get::<AuditEvent>(&key).await?)
    }

    pub async fn list_all(&self) -> Result<Vec<AuditEvent>, AuditError> {
        let results: Vec<(String, AuditEvent)> = self.store.scan_prefix(KEY_PREFIX).await?;
        Ok(results.into_iter().map(|(_, e)| e).collect())
    }

    pub async fn list_by_agent(&self, agent_id: &str) -> Result<Vec<AuditEvent>, AuditError> {
        let all = self.list_all().await?;
        Ok(all.into_iter().filter(|e| e.agent_id.as_deref() == Some(agent_id)).collect())
    }

    pub async fn list_by_mission(&self, mission_id: &str) -> Result<Vec<AuditEvent>, AuditError> {
        let all = self.list_all().await?;
        Ok(all.into_iter().filter(|e| e.mission_id.as_deref() == Some(mission_id)).collect())
    }

    pub async fn list_by_task(&self, task_id: &str) -> Result<Vec<AuditEvent>, AuditError> {
        let all = self.list_all().await?;
        Ok(all.into_iter().filter(|e| e.task_id.as_deref() == Some(task_id)).collect())
    }

    pub async fn list_by_event_type(&self, event_type: &str) -> Result<Vec<AuditEvent>, AuditError> {
        let all = self.list_all().await?;
        Ok(all.into_iter().filter(|e| e.event_type == event_type).collect())
    }

    pub async fn count(&self) -> Result<usize, AuditError> {
        let keys = self.store.list_keys(KEY_PREFIX).await?;
        Ok(keys.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(id: &str) -> AuditEvent {
        AuditEvent {
            id: AuditEventId(uuid::Uuid::parse_str(id).unwrap()),
            tenant_id: "tenant-1".into(),
            mission_id: Some("mission-1".into()),
            task_id: Some("task-1".into()),
            agent_id: Some("agent-1".into()),
            parent_agent_id: None,
            lineage_id: Some("lineage-1".into()),
            worker_id: None,
            trace_id: Some("trace-1".into()),
            event_type: "task.completed".into(),
            lifecycle_mode: Some("persistent".into()),
            risk_level: Some("low".into()),
            status: "success".into(),
            cost_usd: 0.0,
            payload: serde_json::json!({"detail": "ok"}),
            timestamp: Utc::now(),
        }
    }

    fn make_svc() -> AuditService {
        let store = Arc::new(clawhive_store::InMemoryStore::new()) as Arc<dyn Store>;
        AuditService::new(store)
    }

    #[tokio::test]
    async fn test_emit_and_get() {
        let svc = make_svc();
        let event = make_event("00000000-0000-0000-0000-000000000001");
        let emitted = svc.emit_event(event).await.unwrap();
        assert_eq!(emitted.event_type, "task.completed");

        let retrieved = svc.get_event(&emitted.id).await.unwrap().unwrap();
        assert_eq!(retrieved.event_type, "task.completed");
        assert_eq!(retrieved.agent_id.as_deref(), Some("agent-1"));
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let svc = make_svc();
        let id = AuditEventId(uuid::Uuid::nil());
        let retrieved = svc.get_event(&id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_list_all() {
        let svc = make_svc();
        let e1 = make_event("00000000-0000-0000-0000-000000000001");
        let e2 = make_event("00000000-0000-0000-0000-000000000002");
        svc.emit_event(e1).await.unwrap();
        svc.emit_event(e2).await.unwrap();

        let all = svc.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_list_by_agent() {
        let svc = make_svc();
        let mut e1 = make_event("00000000-0000-0000-0000-000000000001");
        let mut e2 = make_event("00000000-0000-0000-0000-000000000002");
        e1.agent_id = Some("agent-a".into());
        e2.agent_id = Some("agent-b".into());
        svc.emit_event(e1).await.unwrap();
        svc.emit_event(e2).await.unwrap();

        let by_a = svc.list_by_agent("agent-a").await.unwrap();
        assert_eq!(by_a.len(), 1);
        assert_eq!(by_a[0].agent_id.as_deref(), Some("agent-a"));
    }

    #[tokio::test]
    async fn test_list_by_event_type() {
        let svc = make_svc();
        let mut e1 = make_event("00000000-0000-0000-0000-000000000001");
        let mut e2 = make_event("00000000-0000-0000-0000-000000000002");
        e1.event_type = "spawn.approved".into();
        e2.event_type = "task.completed".into();
        svc.emit_event(e1).await.unwrap();
        svc.emit_event(e2).await.unwrap();

        let completed = svc.list_by_event_type("task.completed").await.unwrap();
        assert_eq!(completed.len(), 1);
    }

    #[tokio::test]
    async fn test_count() {
        let svc = make_svc();
        svc.emit_event(make_event("00000000-0000-0000-0000-000000000001")).await.unwrap();
        svc.emit_event(make_event("00000000-0000-0000-0000-000000000002")).await.unwrap();
        assert_eq!(svc.count().await.unwrap(), 2);
    }
}
