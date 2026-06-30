use super::*;
use chrono::Utc;

#[test]
fn test_subject_matching() {
    assert!(InMemoryEventBus::matches(
        "claw10.agent.*",
        "claw10.agent.spawned"
    ));
    assert!(InMemoryEventBus::matches(
        "claw10.>",
        "claw10.agent.spawned"
    ));
    assert!(InMemoryEventBus::matches(
        "claw10.agent.spawned",
        "claw10.agent.spawned"
    ));
    assert!(!InMemoryEventBus::matches(
        "claw10.task.*",
        "claw10.agent.spawned"
    ));
    assert!(!InMemoryEventBus::matches(
        "claw10.agent.hibernated",
        "claw10.agent.spawned"
    ));
}

#[tokio::test]
async fn test_publish_and_receive() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let bus = Arc::new(InMemoryEventBus::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    bus.subscribe(
        "claw10.agent.*",
        Arc::new(move |_event| {
            let c = Arc::clone(&counter_clone);
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
            })
        }),
    )
    .await
    .unwrap();

    bus.publish(Claw10Event::AgentSpawned {
        agent_id: Uuid::now_v7(),
        parent_agent_id: None,
        mission_id: Uuid::now_v7(),
        role: "specialist".into(),
        lifecycle_mode: "ephemeral".into(),
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    // Tunggu handler selesai
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_unsubscribe() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let bus = Arc::new(InMemoryEventBus::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = Arc::clone(&counter);

    let sub_id = bus
        .subscribe(
            "claw10.>",
            Arc::new(move |_event| {
                let c = Arc::clone(&counter_clone);
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                })
            }),
        )
        .await
        .unwrap();

    bus.unsubscribe(&sub_id).await.unwrap();

    bus.publish(Claw10Event::AgentSpawned {
        agent_id: Uuid::now_v7(),
        parent_agent_id: None,
        mission_id: Uuid::now_v7(),
        role: "specialist".into(),
        lifecycle_mode: "ephemeral".into(),
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // Handler sudah di-unsubscribe, counter harus tetap 0
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_published_history() {
    let bus = InMemoryEventBus::new();

    bus.publish(Claw10Event::AgentHibernated {
        agent_id: Uuid::now_v7(),
        checkpoint_id: Uuid::now_v7(),
        timestamp: Utc::now(),
    })
    .await
    .unwrap();

    let events = bus.published_events().await;
    assert_eq!(events.len(), 1);
}
