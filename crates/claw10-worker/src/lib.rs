#![allow(clippy::pedantic)]

use std::sync::Arc;

use chrono::Utc;

use claw10_domain::{
    Worker, WorkerCapability, WorkerHeartbeat, WorkerId, WorkerState, WorkerType,
};
use claw10_store::{Store, StoreError, StoreExt};

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("worker not found: {0}")]
    NotFound(String),
    #[error("worker {0} is already draining")]
    AlreadyDraining(String),
    #[error("worker {0} is quarantined")]
    Quarantined(String),
    #[error("worker {0} has active runtimes: {1}")]
    HasActiveRuntimes(String, u32),
    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for WorkerError {
    fn from(e: StoreError) -> Self {
        Self::Other(e.to_string())
    }
}

const KEY_PREFIX: &str = "worker:";

pub struct WorkerService {
    store: Arc<dyn Store>,
}

impl WorkerService {
    #[must_use]
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    /// Register a new worker.
    pub async fn register(
        &self,
        name: String,
        worker_type: WorkerType,
        capabilities: Vec<WorkerCapability>,
        version: String,
    ) -> Worker {
        let now = Utc::now();
        let worker = Worker {
            id: WorkerId(uuid::Uuid::now_v7()),
            name,
            worker_type,
            capabilities,
            state: WorkerState::Online,
            heartbeat: None,
            version,
            is_draining: false,
            created_at: now,
            updated_at: now,
        };
        let key = format!("{KEY_PREFIX}{}", worker.id.0);
        self.store
            .set(&key, &worker)
            .await
            .expect("WorkerService::register: store set failed");
        worker
    }

    /// Process a heartbeat from a worker.
    ///
    /// # Errors
    /// Returns `WorkerError::NotFound` if the worker does not exist.
    /// Returns `WorkerError::Quarantined` if the worker is quarantined.
    pub async fn heartbeat(
        &self,
        worker_id: &WorkerId,
        heartbeat: WorkerHeartbeat,
    ) -> Result<(), WorkerError> {
        let key = format!("{KEY_PREFIX}{}", worker_id.0);
        let mut worker = self
            .store
            .get::<Worker>(&key)
            .await?
            .ok_or_else(|| WorkerError::NotFound(worker_id.0.to_string()))?;

        if worker.state == WorkerState::Quarantined {
            return Err(WorkerError::Quarantined(worker_id.0.to_string()));
        }

        worker.heartbeat = Some(heartbeat);
        worker.state = WorkerState::Online;
        worker.updated_at = Utc::now();

        self.store.set(&key, &worker).await?;
        Ok(())
    }

    /// Mark a worker as draining.
    ///
    /// # Errors
    /// Returns `WorkerError::NotFound` if the worker does not exist.
    /// Returns `WorkerError::AlreadyDraining` if already draining.
    /// Returns `WorkerError::Quarantined` if quarantined.
    pub async fn drain(&self, worker_id: &WorkerId) -> Result<(), WorkerError> {
        let key = format!("{KEY_PREFIX}{}", worker_id.0);
        let mut worker = self
            .store
            .get::<Worker>(&key)
            .await?
            .ok_or_else(|| WorkerError::NotFound(worker_id.0.to_string()))?;

        if worker.is_draining {
            return Err(WorkerError::AlreadyDraining(worker_id.0.to_string()));
        }

        if worker.state == WorkerState::Quarantined {
            return Err(WorkerError::Quarantined(worker_id.0.to_string()));
        }

        worker.is_draining = true;
        worker.state = WorkerState::Draining;
        worker.updated_at = Utc::now();

        self.store.set(&key, &worker).await?;
        Ok(())
    }

    /// Mark a worker as offline.
    ///
    /// # Errors
    /// Returns `WorkerError::NotFound` if the worker does not exist.
    pub async fn mark_offline(&self, worker_id: &WorkerId) -> Result<(), WorkerError> {
        let key = format!("{KEY_PREFIX}{}", worker_id.0);
        let mut worker = self
            .store
            .get::<Worker>(&key)
            .await?
            .ok_or_else(|| WorkerError::NotFound(worker_id.0.to_string()))?;

        worker.state = WorkerState::Offline;
        worker.updated_at = Utc::now();

        self.store.set(&key, &worker).await?;
        Ok(())
    }

    /// Quarantine a worker.
    ///
    /// # Errors
    /// Returns `WorkerError::NotFound` if the worker does not exist.
    pub async fn quarantine(&self, worker_id: &WorkerId) -> Result<(), WorkerError> {
        let key = format!("{KEY_PREFIX}{}", worker_id.0);
        let mut worker = self
            .store
            .get::<Worker>(&key)
            .await?
            .ok_or_else(|| WorkerError::NotFound(worker_id.0.to_string()))?;

        worker.state = WorkerState::Quarantined;
        worker.updated_at = Utc::now();

        self.store.set(&key, &worker).await?;
        Ok(())
    }

    /// Get a worker by ID.
    pub async fn get(&self, worker_id: &WorkerId) -> Result<Option<Worker>, WorkerError> {
        let key = format!("{KEY_PREFIX}{}", worker_id.0);
        Ok(self.store.get::<Worker>(&key).await?)
    }

    /// List all workers, optionally filtered by state.
    pub async fn list(
        &self,
        state_filter: Option<&WorkerState>,
    ) -> Result<Vec<Worker>, WorkerError> {
        let all: Vec<(String, Worker)> = self.store.scan_prefix(KEY_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, w)| w)
            .filter(|w| match state_filter {
                Some(s) => &w.state == s,
                None => true,
            })
            .collect())
    }

    /// Detect workers that haven't sent a heartbeat within the stale threshold.
    pub async fn detect_stale(
        &self,
        max_seconds_since_heartbeat: i64,
    ) -> Result<Vec<Worker>, WorkerError> {
        let now = Utc::now();
        let all: Vec<(String, Worker)> = self.store.scan_prefix(KEY_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, w)| w)
            .filter(|w| {
                if w.state != WorkerState::Online {
                    return false;
                }
                match &w.heartbeat {
                    Some(hb) => (now - hb.timestamp).num_seconds() > max_seconds_since_heartbeat,
                    None => true,
                }
            })
            .collect())
    }

    /// Count workers by state.
    pub async fn count_by_state(
        &self,
    ) -> Result<std::collections::HashMap<String, usize>, WorkerError> {
        let all: Vec<(String, Worker)> = self.store.scan_prefix(KEY_PREFIX).await?;
        let mut counts = std::collections::HashMap::new();
        for (_, worker) in all {
            *counts.entry(format!("{:?}", worker.state)).or_insert(0) += 1;
        }
        Ok(counts)
    }
}
