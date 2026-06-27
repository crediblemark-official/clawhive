#![allow(clippy::pedantic)]

use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use clawhive_domain::{Channel, ChannelType, IdentityId, Session, SessionState};
use clawhive_store::{Store, StoreError, StoreExt};

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("channel not found: {0}")]
    ChannelNotFound(String),
    #[error("channel {0} is inactive")]
    ChannelInactive(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("session {0} is expired")]
    SessionExpired(String),
    #[error("unsupported channel type for dispatch: {0:?}")]
    UnsupportedDispatch(ChannelType),
    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for GatewayError {
    fn from(e: StoreError) -> Self {
        Self::Other(e.to_string())
    }
}

/// A message to be dispatched through a channel.
#[derive(Debug, Clone)]
pub struct Message {
    pub recipient: String,
    pub subject: Option<String>,
    pub body: String,
    pub metadata: Option<serde_json::Value>,
}

/// Result of a message dispatch.
#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub channel_id: String,
    pub success: bool,
    pub response: Option<String>,
    pub dispatched_at: DateTime<Utc>,
}

const CHANNEL_PREFIX: &str = "gateway:channel:";
const SESSION_PREFIX: &str = "gateway:session:";

pub struct GatewayService {
    store: Arc<dyn Store>,
}

impl GatewayService {
    #[must_use]
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    // ── Channel Management ──────────────────────────────────────

    /// Register a new channel.
    pub async fn register_channel(
        &self,
        channel_type: ChannelType,
        config: serde_json::Value,
    ) -> Channel {
        let channel = Channel {
            id: Uuid::now_v7().to_string(),
            channel_type,
            config,
            is_active: true,
        };
        let key = format!("{CHANNEL_PREFIX}{}", channel.id);
        self.store
            .set(&key, &channel)
            .await
            .expect("GatewayService::register_channel: store set failed");
        channel
    }

    /// Get a channel by ID.
    pub async fn get_channel(&self, channel_id: &str) -> Result<Option<Channel>, GatewayError> {
        let key = format!("{CHANNEL_PREFIX}{channel_id}");
        Ok(self.store.get::<Channel>(&key).await?)
    }

    /// List all channels, optionally filtered by type.
    pub async fn list_channels(
        &self,
        type_filter: Option<&ChannelType>,
    ) -> Result<Vec<Channel>, GatewayError> {
        let all: Vec<(String, Channel)> = self.store.scan_prefix(CHANNEL_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, c)| c)
            .filter(|c| match type_filter {
                Some(t) => &c.channel_type == t,
                None => true,
            })
            .collect())
    }

    /// Activate a channel.
    ///
    /// # Errors
    /// Returns `GatewayError::ChannelNotFound` if the channel does not exist.
    pub async fn activate_channel(&self, channel_id: &str) -> Result<(), GatewayError> {
        let key = format!("{CHANNEL_PREFIX}{channel_id}");
        let mut channel = self
            .store
            .get::<Channel>(&key)
            .await?
            .ok_or_else(|| GatewayError::ChannelNotFound(channel_id.into()))?;
        channel.is_active = true;
        self.store.set(&key, &channel).await?;
        Ok(())
    }

    /// Deactivate a channel.
    ///
    /// # Errors
    /// Returns `GatewayError::ChannelNotFound` if the channel does not exist.
    pub async fn deactivate_channel(&self, channel_id: &str) -> Result<(), GatewayError> {
        let key = format!("{CHANNEL_PREFIX}{channel_id}");
        let mut channel = self
            .store
            .get::<Channel>(&key)
            .await?
            .ok_or_else(|| GatewayError::ChannelNotFound(channel_id.into()))?;
        channel.is_active = false;
        self.store.set(&key, &channel).await?;
        Ok(())
    }

    // ── Session Management ──────────────────────────────────────

    /// Create a new session.
    pub async fn create_session(
        &self,
        identity_id: IdentityId,
        channel_id: String,
        ttl_seconds: i64,
    ) -> Result<Session, GatewayError> {
        let channel_key = format!("{CHANNEL_PREFIX}{channel_id}");
        if !self.store.exists(&channel_key).await? {
            return Err(GatewayError::ChannelNotFound(channel_id));
        }

        let now = Utc::now();
        let session = Session {
            id: Uuid::now_v7().to_string(),
            identity_id,
            channel_id,
            state: SessionState::Active,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(ttl_seconds),
        };
        let key = format!("{SESSION_PREFIX}{}", session.id);
        self.store
            .set(&key, &session)
            .await
            .expect("GatewayService::create_session: store set failed");
        Ok(session)
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>, GatewayError> {
        let key = format!("{SESSION_PREFIX}{session_id}");
        Ok(self.store.get::<Session>(&key).await?)
    }

    /// Terminate a session.
    ///
    /// # Errors
    /// Returns `GatewayError::SessionNotFound` if the session does not exist.
    pub async fn terminate_session(&self, session_id: &str) -> Result<(), GatewayError> {
        let key = format!("{SESSION_PREFIX}{session_id}");
        let mut session = self
            .store
            .get::<Session>(&key)
            .await?
            .ok_or_else(|| GatewayError::SessionNotFound(session_id.into()))?;
        session.state = SessionState::Terminated;
        self.store.set(&key, &session).await?;
        Ok(())
    }

    /// Clean up expired sessions.
    pub async fn expire_stale_sessions(&self) -> Result<usize, GatewayError> {
        let now = Utc::now();
        let all: Vec<(String, Session)> = self.store.scan_prefix(SESSION_PREFIX).await?;
        let mut expired_count = 0;

        for (key, mut session) in all {
            if session.expires_at < now && session.state == SessionState::Active {
                session.state = SessionState::Expired;
                self.store.set(&key, &session).await?;
                expired_count += 1;
            }
        }

        Ok(expired_count)
    }

    /// List active sessions for an identity.
    pub async fn list_sessions(
        &self,
        identity_id: &IdentityId,
    ) -> Result<Vec<Session>, GatewayError> {
        let all: Vec<(String, Session)> = self.store.scan_prefix(SESSION_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, s)| s)
            .filter(|s| s.identity_id == *identity_id)
            .collect())
    }

    // ── Message Dispatch ────────────────────────────────────────

    /// Dispatch a message through a channel.
    ///
    /// This is a simulation — actual transport is not implemented.
    ///
    /// # Errors
    /// Returns `GatewayError::ChannelNotFound` if the channel does not exist.
    /// Returns `GatewayError::ChannelInactive` if the channel is inactive.
    /// Returns `GatewayError::UnsupportedDispatch` for non-dispatchable channel types.
    pub async fn dispatch(
        &self,
        channel_id: &str,
        _message: &Message,
    ) -> Result<DispatchResult, GatewayError> {
        let key = format!("{CHANNEL_PREFIX}{channel_id}");
        let channel = self
            .store
            .get::<Channel>(&key)
            .await?
            .ok_or_else(|| GatewayError::ChannelNotFound(channel_id.into()))?;

        if !channel.is_active {
            return Err(GatewayError::ChannelInactive(channel_id.into()));
        }

        match channel.channel_type {
            ChannelType::Webhook
            | ChannelType::Email
            | ChannelType::Slack
            | ChannelType::Telegram
            | ChannelType::WhatsApp
            | ChannelType::Discord
            | ChannelType::InternalBus => Ok(DispatchResult {
                channel_id: channel_id.into(),
                success: true,
                response: Some(format!("dispatched via {:?}", channel.channel_type)),
                dispatched_at: Utc::now(),
            }),
            ChannelType::Mobile | ChannelType::Rest | ChannelType::Terminal => {
                Err(GatewayError::UnsupportedDispatch(channel.channel_type))
            }
        }
    }
}
