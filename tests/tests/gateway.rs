use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use claw10_domain::{ChannelType, IdentityId};
use claw10_gateway::{GatewayService, Message};
use claw10_store::InMemoryStore;

fn make_identity() -> IdentityId {
    IdentityId(Uuid::now_v7())
}

fn make_svc() -> GatewayService {
    let store = Arc::new(InMemoryStore::new()) as Arc<dyn claw10_store::Store>;
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
    assert_eq!(session.state, claw10_domain::SessionState::Active);
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
        claw10_domain::SessionState::Terminated
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
        .register_channel(ChannelType::Mobile, serde_json::json!({}))
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
async fn test_dispatch_terminal_succeeds() {
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
    assert!(result.is_ok());
    assert_eq!(result.unwrap().response.unwrap(), "terminal echo");
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

// ── Incoming Webhook Tests ────────────────────────────────────────

#[tokio::test]
async fn test_receive_telegram_message() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Telegram, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "update_id": 12345,
        "message": {
            "message_id": 1,
            "from": {"id": 98765, "is_bot": false, "first_name": "User"},
            "chat": {"id": 98765, "type": "private"},
            "text": "hello from telegram"
        }
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    assert_eq!(result.message.sender_id, "98765");
    assert_eq!(result.message.text, "hello from telegram");
    assert_eq!(result.message.channel_id, channel.id);
}

#[tokio::test]
async fn test_receive_telegram_missing_fields_fails() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Telegram, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({"update_id": 1});
    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_receive_telegram_secret_token() {
    let svc = make_svc();
    let channel = svc
        .register_channel(
            ChannelType::Telegram,
            serde_json::json!({"secret_token": "supersecret"}),
        )
        .await;

    let payload = serde_json::json!({
        "update_id": 1,
        "message": {
            "message_id": 1,
            "from": {"id": 111, "is_bot": false, "first_name": "A"},
            "chat": {"id": 111, "type": "private"},
            "text": "hi"
        }
    });

    // Wrong token should fail
    let mut headers = HashMap::new();
    headers.insert("x-telegram-bot-api-secret-token".into(), "wrong".into());
    let result = svc.receive(&channel.id, &payload, &headers).await;
    assert!(result.is_err());

    // Correct token should succeed
    headers.insert("x-telegram-bot-api-secret-token".into(), "supersecret".into());
    let result = svc.receive(&channel.id, &payload, &headers).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_receive_whatsapp_message() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::WhatsApp, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "entry": [{
            "changes": [{
                "value": {
                    "messages": [{
                        "from": "628123456789",
                        "text": {"body": "halo dari WA"}
                    }]
                }
            }]
        }]
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    assert_eq!(result.message.sender_id, "628123456789");
    assert_eq!(result.message.text, "halo dari WA");
}

#[tokio::test]
async fn test_receive_slack_event() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Slack, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "token": "dummy",
        "challenge": "challenge-string-123",
        "type": "url_verification"
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    // Slack URL verification returns the challenge in response
    assert!(result.response.is_some());
    assert_eq!(
        result.response.unwrap().get("challenge").unwrap(),
        "challenge-string-123"
    );
}

#[tokio::test]
async fn test_receive_slack_message() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Slack, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "token": "dummy",
        "team_id": "T123",
        "event": {
            "type": "message",
            "user": "U456",
            "text": "hello dari slack",
            "channel": "C789"
        }
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    assert_eq!(result.message.sender_id, "U456");
    assert_eq!(result.message.text, "hello dari slack");
}

#[tokio::test]
async fn test_receive_discord_interaction() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Discord, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "type": 2,
        "member": {"user": {"id": "123456"}},
        "data": {"name": "ask", "options": [{"value": "hello dari discord"}]}
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    assert_eq!(result.message.sender_id, "123456");
    assert_eq!(result.message.text, "/ask");
}

#[tokio::test]
async fn test_receive_generic_webhook() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "sender": "user1",
        "text": "hello from webhook"
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    assert_eq!(result.message.sender_id, "user1");
    assert_eq!(result.message.text, "hello from webhook");
}

#[tokio::test]
async fn test_receive_rest_channel() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Rest, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({
        "sender": "api-user",
        "text": "hello from rest"
    });

    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await.unwrap();
    assert_eq!(result.message.sender_id, "api-user");
    assert_eq!(result.message.text, "hello from rest");
}

#[tokio::test]
async fn test_receive_unsupported_channel_fails() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Mobile, serde_json::json!({}))
        .await;

    let payload = serde_json::json!({"text": "test"});
    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_receive_inactive_channel_fails() {
    let svc = make_svc();
    let channel = svc
        .register_channel(ChannelType::Webhook, serde_json::json!({}))
        .await;
    svc.deactivate_channel(&channel.id).await.unwrap();

    let payload = serde_json::json!({"text": "test"});
    let headers = HashMap::new();
    let result = svc.receive(&channel.id, &payload, &headers).await;
    assert!(result.is_err());
}
