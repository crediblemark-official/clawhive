use std::sync::Arc;

use uuid::Uuid;

use clawhive_domain::{AgentId, EvidenceId, MemoryStatus, MemoryType, TaskId};
use clawhive_memory::{MemoryQuery, MemoryService, StoreMemoryInput};
use clawhive_store::InMemoryStore;

fn make_agent_id() -> AgentId {
    AgentId(Uuid::now_v7())
}

fn make_task_id() -> TaskId {
    TaskId(Uuid::now_v7())
}

fn make_input(
    tenant: &str,
    scope: &str,
    mtype: MemoryType,
    content: &str,
    confidence: f64,
) -> StoreMemoryInput {
    StoreMemoryInput {
        tenant_id: tenant.into(),
        scope: scope.into(),
        memory_type: mtype,
        content: content.into(),
        source_agent: make_agent_id(),
        source_task: make_task_id(),
        evidence_id: None,
        confidence,
        classification: "test".into(),
    }
}

fn make_svc() -> MemoryService {
    let store = Arc::new(InMemoryStore::new()) as Arc<dyn clawhive_store::Store>;
    MemoryService::new(store)
}

#[tokio::test]
async fn test_store_and_get_memory() {
    let svc = make_svc();
    let agent = make_agent_id();
    let task = make_task_id();

    let mem = svc
        .store(StoreMemoryInput {
            tenant_id: "tenant-1".into(),
            scope: "public".into(),
            memory_type: MemoryType::Episodic,
            content: "agent observed something".into(),
            source_agent: agent.clone(),
            source_task: task.clone(),
            evidence_id: None,
            confidence: 0.8,
            classification: "observation".into(),
        })
        .await;

    let retrieved = svc.get(&mem.id).await.unwrap().unwrap();
    assert_eq!(retrieved.content, "agent observed something");
    assert_eq!(retrieved.status, MemoryStatus::Active);
    assert_eq!(retrieved.source.agent_id, agent);
    assert_eq!(retrieved.source.task_id, task);
}

#[tokio::test]
async fn test_update_content() {
    let svc = make_svc();
    let mem = svc
        .store(make_input("t", "s", MemoryType::Working, "original", 0.5))
        .await;

    svc.update_content(&mem.id, "updated".into())
        .await
        .unwrap();
    let retrieved = svc.get(&mem.id).await.unwrap().unwrap();
    assert_eq!(retrieved.content, "updated");
}

#[tokio::test]
async fn test_transition_status_valid() {
    let svc = make_svc();
    let mem = svc
        .store(make_input("t", "s", MemoryType::Semantic, "data", 0.9))
        .await;

    // Admission pipeline activates high-confidence memories directly.
    assert_eq!(mem.status, MemoryStatus::Active);

    svc.transition_status(&mem.id, MemoryStatus::Rejected)
        .await
        .unwrap();
    assert_eq!(
        svc.get(&mem.id).await.unwrap().unwrap().status,
        MemoryStatus::Rejected
    );
}

#[tokio::test]
async fn test_transition_status_invalid() {
    let svc = make_svc();
    let mem = svc
        .store(make_input("t", "s", MemoryType::Procedural, "steps", 0.7))
        .await;

    let result = svc.transition_status(&mem.id, MemoryStatus::Active).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_verify_memory() {
    let svc = make_svc();
    let verifier = make_agent_id();
    let mem = svc
        .store(make_input(
            "t",
            "s",
            MemoryType::AgentContinuity,
            "memory",
            0.6,
        ))
        .await;

    svc.verify(&mem.id, verifier.clone()).await.unwrap();
    let retrieved = svc.get(&mem.id).await.unwrap().unwrap();
    assert_eq!(retrieved.verified_by.len(), 1);
    assert_eq!(retrieved.verified_by[0], verifier);
}

#[tokio::test]
async fn test_query_filter() {
    let svc = make_svc();
    let agent_a = make_agent_id();
    let agent_b = make_agent_id();
    let task = make_task_id();

    svc.store(StoreMemoryInput {
        tenant_id: "t1".into(),
        scope: "public".into(),
        memory_type: MemoryType::Episodic,
        content: "event a".into(),
        source_agent: agent_a.clone(),
        source_task: task.clone(),
        evidence_id: None,
        confidence: 0.8,
        classification: "event".into(),
    })
    .await;

    svc.store(StoreMemoryInput {
        tenant_id: "t1".into(),
        scope: "private".into(),
        memory_type: MemoryType::Semantic,
        content: "fact b".into(),
        source_agent: agent_b.clone(),
        source_task: task,
        evidence_id: None,
        confidence: 0.9,
        classification: "fact".into(),
    })
    .await;

    let results = svc
        .query(MemoryQuery {
            scope: Some("public".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "event a");

    let results = svc
        .query(MemoryQuery {
            source_agent: Some(agent_b),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "fact b");

    let results = svc
        .query(MemoryQuery {
            scope: Some("public".into()),
            source_agent: Some(agent_a),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_delete_memory() {
    let svc = make_svc();
    let mem = svc
        .store(make_input("t", "s", MemoryType::User, "user data", 1.0))
        .await;

    svc.delete(&mem.id).await.unwrap();
    assert!(svc.get(&mem.id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_count_by_status() {
    let svc = make_svc();
    let _m1 = svc
        .store(make_input("t", "s", MemoryType::Working, "a", 0.5))
        .await;
    let _m2 = svc
        .store(make_input("t", "s", MemoryType::Working, "b", 0.9))
        .await;

    // Low-confidence memory is rejected; high-confidence memory is activated.
    let counts = svc.count_by_status().await.unwrap();
    assert_eq!(*counts.get("Rejected").unwrap_or(&0), 1);
    assert_eq!(*counts.get("Active").unwrap_or(&0), 1);
}

#[tokio::test]
async fn test_store_with_evidence() {
    let svc = make_svc();
    let evidence_id = EvidenceId(Uuid::now_v7());

    let mem = svc
        .store(StoreMemoryInput {
            tenant_id: "t".into(),
            scope: "s".into(),
            memory_type: MemoryType::Procedural,
            content: "evidence-linked".into(),
            source_agent: make_agent_id(),
            source_task: make_task_id(),
            evidence_id: Some(evidence_id.clone()),
            confidence: 0.95,
            classification: "verified".into(),
        })
        .await;

    assert_eq!(mem.source.evidence_id, Some(evidence_id));
}
