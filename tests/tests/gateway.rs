use std::sync::Arc;

use uuid::Uuid;

use clawhive_domain::{ChannelType, IdentityId};
use clawhive_gateway::{GatewayService, Message};
use clawhive_store::InMemoryStore;

fn make_identity() -> IdentityId {
    IdentityId(Uuid::now_v7())
}

fn make_svc() -> GatewayService {
    let store = Arc::new(InMemoryStore::new()) as Arc<dyn clawhive_store::Store>;
    GatewayService::new(store)
}

#[tokio::test]
async fn test_register_channel() {
    let svc = make_svc();
    let channel = svc
        .register_channel(
            ChannelType::Webhook,
            serde_json::json!({"url": "https://example.com/hook"}),
        )
        .await;

    assert_eq!(channel.channel_type, ChannelType::Webhook);
    assert!(channel.is_active);
}

#[tokio::test]
async fn test_get_channel() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Slack, serde_json::json!({"token": "xoxb-123"}))
        .await;

    let retrieved = svc.get_channel(&channel.id).await.unwrap().unwrap();
    assert_eq!(retrieved.channel_type, ChannelType::Slack);
}

#[tokio::test]
async fn test_list_channels_filter() {
    let svc = make_svc();
    svc.register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;
    svc.register_channel(ChannelType::Telegram, serde_json::json!({}))
        .await;

    let webhooks = svc.list_channels(Some(&ChannelType::Webhook)).await.unwrap();
    assert_eq!(webhooks.len(), 1);

    let all = svc.list_channels(None).await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_activate_deactivate_channel() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;

    svc.deactivate_channel(&channel.id).await.unwrap();
    assert!(!svc.get_channel(&channel.id).await.unwrap().unwrap().is_active);

    svc.activate_channel(&channel.id).await.unwrap();
    assert!(svc.get_channel(&channel.id).await.unwrap().unwrap().is_active);
}

#[tokio::test]
async fn test_create_session() {
    let svc = make_svc();
    let identity = make_identity();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;

    let session = svc
        .create_session(identity.clone(), channel.id.clone(), 3600)
        .await
        .unwrap();

    assert_eq!(session.identity_id, identity);
    assert_eq!(session.state, clawhive_domain::SessionState::Active);
    assert!(session.expires_at > session.created_at);
}

#[tokio::test]
async fn test_create_session_fails_for_nonexistent_channel() {
    let svc = make_svc();
    let identity = make_identity();

    let result = svc.create_session(identity, "nonexistent".into(), 3600).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_terminate_session() {
    let svc = make_svc();
    let identity = make_identity();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;

    let session = svc
        .create_session(identity, channel.id.clone(), 3600)
        .await
        .unwrap();

    svc.terminate_session(&session.id).await.unwrap();
    assert_eq!(
        svc.get_session(&session.id).await.unwrap().unwrap().state,
        clawhive_domain::SessionState::Terminated
    );
}

#[tokio::test]
async fn test_dispatch_internal_bus() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::InternalBus, serde_json::json!({}))
        .await;

    let msg = Message {
        recipient: "user-1".into(),
        subject: Some("alert".into()),
        body: "something happened".into(),
        metadata: None,
    };

    let result = svc.dispatch(&channel.id, &msg).await.unwrap();
    assert!(result.success);
    assert_eq!(result.response.as_deref(), Some("internal bus echo"));
}

#[tokio::test]
async fn test_dispatch_webhook_missing_url_fails() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;

    let msg = Message {
        recipient: "user-1".into(),
        subject: Some("alert".into()),
        body: "something happened".into(),
        metadata: None,
    };

    let result = svc.dispatch(&channel.id, &msg).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_dispatch_inactive_channel_fails() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;
    svc.deactivate_channel(&channel.id).await.unwrap();

    let msg = Message {
        recipient: "user".into(),
        subject: None,
        body: "test".into(),
        metadata: None,
    };

    let result = svc.dispatch(&channel.id, &msg).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_dispatch_unsupported_channel_fails() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Terminal, serde_json::json!({}))
        .await;

    let msg = Message {
        recipient: "user".into(),
        subject: None,
        body: "test".into(),
        metadata: None,
    };

    let result = svc.dispatch(&channel.id, &msg).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_expire_stale_sessions() {
    let svc = make_svc();
    let identity = make_identity();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;

    // Create a session with a TTL of 0 so it expires immediately
    let _session = svc
        .create_session(identity.clone(), channel.id.clone(), 0)
        .await
        .unwrap();

    // Small sleep to ensure it's stale
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let expired_count = svc.expire_stale_sessions().await.unwrap();
    assert_eq!(expired_count, 1);
}
