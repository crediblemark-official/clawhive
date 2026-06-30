use std::sync::Arc;

use chrono::Utc;

use claw10_domain::{WorkerCapability, WorkerHeartbeat, WorkerState, WorkerType};
use claw10_store::InMemoryStore;
use claw10_worker::WorkerService;

fn make_hb() -> WorkerHeartbeat {
    WorkerHeartbeat {
        cpu_percent: 10.0,
        memory_percent: 20.0,
        active_runtimes: 0,
        queue_depth: 0,
        tool_availability: vec![],
        sandbox_healthy: true,
        timestamp: Utc::now(),
    }
}

fn make_svc() -> WorkerService {
    let store = Arc::new(InMemoryStore::new()) as Arc<dyn claw10_store::Store>;
    WorkerService::new(store)
}

#[tokio::test]
async fn test_register_worker() {
    let svc = make_svc();
    let worker = svc
        .register(
            "worker-1".into(),
            WorkerType::Local,
            vec![WorkerCapability {
                name: "shell".into(),
                version: Some("1.0".into()),
            }],
            "0.1.0".into(),
        )
        .await;

    assert_eq!(worker.name, "worker-1");
    assert_eq!(worker.state, WorkerState::Online);
    assert!(!worker.is_draining);
    assert!(worker.heartbeat.is_none());
}

#[tokio::test]
async fn test_heartbeat_updates_worker() {
    let svc = make_svc();
    let worker = svc
        .register("worker-1".into(), WorkerType::Local, vec![], "0.1.0".into())
        .await;

    let hb = WorkerHeartbeat {
        cpu_percent: 45.0,
        memory_percent: 60.0,
        active_runtimes: 3,
        queue_depth: 5,
        tool_availability: vec!["shell".into()],
        sandbox_healthy: true,
        timestamp: Utc::now(),
    };
    svc.heartbeat(&worker.id, hb).await.unwrap();

    let retrieved = svc.get(&worker.id).await.unwrap().unwrap();
    assert_eq!(retrieved.state, WorkerState::Online);
    assert_eq!(retrieved.heartbeat.as_ref().unwrap().cpu_percent, 45.0);
    assert_eq!(retrieved.heartbeat.as_ref().unwrap().active_runtimes, 3);
}

#[tokio::test]
async fn test_drain_worker() {
    let svc = make_svc();
    let worker = svc
        .register("worker-1".into(), WorkerType::Remote, vec![], "0.1.0".into())
        .await;

    svc.drain(&worker.id).await.unwrap();
    let retrieved = svc.get(&worker.id).await.unwrap().unwrap();
    assert!(retrieved.is_draining);
    assert_eq!(retrieved.state, WorkerState::Draining);
}

#[tokio::test]
async fn test_drain_twice_fails() {
    let svc = make_svc();
    let worker = svc
        .register("w".into(), WorkerType::Local, vec![], "1".into())
        .await;

    svc.drain(&worker.id).await.unwrap();
    let result = svc.drain(&worker.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_quarantine_worker() {
    let svc = make_svc();
    let worker = svc
        .register("w".into(), WorkerType::Cloud, vec![], "1".into())
        .await;

    svc.quarantine(&worker.id).await.unwrap();
    let retrieved = svc.get(&worker.id).await.unwrap().unwrap();
    assert_eq!(retrieved.state, WorkerState::Quarantined);
}

#[tokio::test]
async fn test_heartbeat_fails_when_quarantined() {
    let svc = make_svc();
    let worker = svc
        .register("w".into(), WorkerType::Local, vec![], "1".into())
        .await;
    svc.quarantine(&worker.id).await.unwrap();

    let result = svc.heartbeat(&worker.id, make_hb()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_workers_with_filter() {
    let svc = make_svc();
    let _w1 = svc
        .register("w1".into(), WorkerType::Local, vec![], "1".into())
        .await;
    let w2 = svc
        .register("w2".into(), WorkerType::Remote, vec![], "1".into())
        .await;

    svc.drain(&w2.id).await.unwrap();

    let online = svc.list(Some(&WorkerState::Online)).await.unwrap();
    assert_eq!(online.len(), 1);

    let draining = svc.list(Some(&WorkerState::Draining)).await.unwrap();
    assert_eq!(draining.len(), 1);

    let all = svc.list(None).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_detect_stale() {
    let svc = make_svc();
    let w1 = svc
        .register("w1".into(), WorkerType::Local, vec![], "1".into())
        .await;
    let _w2 = svc
        .register("w2".into(), WorkerType::Local, vec![], "1".into())
        .await;

    svc.heartbeat(&w1.id, make_hb()).await.unwrap();

    let stale = svc.detect_stale(0).await.unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].name, "w2");
}

#[tokio::test]
async fn test_mark_offline() {
    let svc = make_svc();
    let worker = svc
        .register("w".into(), WorkerType::Edge, vec![], "1".into())
        .await;

    svc.mark_offline(&worker.id).await.unwrap();
    let retrieved = svc.get(&worker.id).await.unwrap().unwrap();
    assert_eq!(retrieved.state, WorkerState::Offline);
}

#[tokio::test]
async fn test_count_by_state() {
    let svc = make_svc();
    let w1 = svc
        .register("w1".into(), WorkerType::Local, vec![], "1".into())
        .await;
    let _w2 = svc
        .register("w2".into(), WorkerType::Local, vec![], "1".into())
        .await;

    svc.drain(&w1.id).await.unwrap();

    let counts = svc.count_by_state().await.unwrap();
    assert_eq!(*counts.get("Draining").unwrap_or(&0), 1);
    assert_eq!(*counts.get("Online").unwrap_or(&0), 1);
}
