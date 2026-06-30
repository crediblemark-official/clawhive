#![cfg(feature = "nats")]

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::bus::EventBus;
use crate::nats::NatsEventBus;
use crate::events::Claw10Event;

/// Helper untuk membuat event agen dummy.
fn make_dummy_event() -> Claw10Event {
    Claw10Event::AgentSpawned {
        agent_id: uuid::Uuid::now_v7(),
        parent_agent_id: None,
        mission_id: uuid::Uuid::now_v7(),
        role: "Specialist".to_string(),
        lifecycle_mode: "ephemeral".to_string(),
        timestamp: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn test_nats_event_bus_integration() {
    // Hubungkan ke NATS server lokal. Jika tidak ada server berjalan, skip test.
    let bus = match NatsEventBus::new("127.0.0.1:4222") {
        Ok(b) => b,
        Err(_) => {
            println!("NATS server lokal tidak terdeteksi pada 127.0.0.1:4222. Skipping test.");
            return;
        }
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<Claw10Event>();

    // Subscribe ke subject "claw10.agent.>"
    let handler = Arc::new(move |event: Claw10Event| {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(event);
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
    });

    let sub_id = bus.subscribe("claw10.agent.>", handler).await
        .expect("gagal subscribe");

    // Publish event
    let event = make_dummy_event();
    bus.publish(event.clone()).await.expect("gagal publish");

    // Tunggu event diterima
    let received = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    match received {
        Ok(Some(ev)) => {
            assert_eq!(ev.subject(), event.subject());
        }
        _ => panic!("Event tidak diterima dalam batas waktu 500ms"),
    }

    // Unsubscribe
    bus.unsubscribe(&sub_id).await.expect("gagal unsubscribe");
}
