//! Domain event types yang dipublish dan disubscribe oleh seluruh sistem.
//!
//! Semua event harus serializable (JSON) agar bisa dikirim via NATS atau
//! disimpan dalam event store untuk replay.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Semua event domain-level dalam Claw10.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Claw10Event {
    // ── Agent Lifecycle ────────────────────────────────────────────
    /// Agent baru berhasil di-spawn dan disimpan.
    AgentSpawned {
        agent_id: Uuid,
        parent_agent_id: Option<Uuid>,
        mission_id: Uuid,
        role: String,
        lifecycle_mode: String,
        timestamp: DateTime<Utc>,
    },

    /// Agent aktif mulai hibernasi.
    AgentHibernated {
        agent_id: Uuid,
        checkpoint_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Agent hibernasi berhasil dibangunkan.
    AgentWoken {
        agent_id: Uuid,
        trigger: WakeTrigger,
        timestamp: DateTime<Utc>,
    },

    /// Agent diterminasi (ephemeral selesai atau kill).
    AgentTerminated {
        agent_id: Uuid,
        reason: TerminationReason,
        timestamp: DateTime<Utc>,
    },

    /// Agent dimigrasikan ke worker lain.
    AgentMigrated {
        agent_id: Uuid,
        from_worker: String,
        to_worker: String,
        checkpoint_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    // ── Spawn ──────────────────────────────────────────────────────
    /// Spawn request diterima dan divalidasi.
    SpawnRequestApproved {
        spawn_request_id: Uuid,
        parent_agent_id: Uuid,
        child_count: usize,
        timestamp: DateTime<Utc>,
    },

    /// Spawn request ditolak.
    SpawnRequestDenied {
        spawn_request_id: Uuid,
        parent_agent_id: Uuid,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    // ── Scheduler ─────────────────────────────────────────────────
    /// Schedule agent jatuh tempo dan perlu dibangunkan.
    ScheduleDue {
        agent_id: Uuid,
        cron: String,
        timestamp: DateTime<Utc>,
    },

    // ── Memory ────────────────────────────────────────────────────
    /// Memory candidate baru masuk admission pipeline.
    MemoryCandidateSubmitted {
        memory_id: Uuid,
        agent_id: Uuid,
        scope: String,
        timestamp: DateTime<Utc>,
    },

    /// Memory berhasil diaktifkan setelah admission.
    MemoryActivated {
        memory_id: Uuid,
        scope: String,
        confidence: f64,
        timestamp: DateTime<Utc>,
    },

    /// Memory ditolak dalam admission pipeline.
    MemoryRejected {
        memory_id: Uuid,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    // ── Task ──────────────────────────────────────────────────────
    /// Task selesai diverifikasi.
    TaskVerified {
        task_id: Uuid,
        verifier_agent_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Task gagal dan butuh escalation.
    TaskFailed {
        task_id: Uuid,
        agent_id: Uuid,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    // ── Worker ────────────────────────────────────────────────────
    /// Worker heartbeat diterima.
    WorkerHeartbeat {
        worker_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Worker dideteksi stale.
    WorkerStale {
        worker_id: Uuid,
        last_seen: DateTime<Utc>,
        timestamp: DateTime<Utc>,
    },
}

impl Claw10Event {
    /// Subject/topic NATS untuk event ini.
    /// Format: `claw10.<domain>.<action>`
    #[must_use]
    pub fn subject(&self) -> &'static str {
        match self {
            Claw10Event::AgentSpawned { .. } => "claw10.agent.spawned",
            Claw10Event::AgentHibernated { .. } => "claw10.agent.hibernated",
            Claw10Event::AgentWoken { .. } => "claw10.agent.woken",
            Claw10Event::AgentTerminated { .. } => "claw10.agent.terminated",
            Claw10Event::AgentMigrated { .. } => "claw10.agent.migrated",
            Claw10Event::SpawnRequestApproved { .. } => "claw10.spawn.approved",
            Claw10Event::SpawnRequestDenied { .. } => "claw10.spawn.denied",
            Claw10Event::ScheduleDue { .. } => "claw10.schedule.due",
            Claw10Event::MemoryCandidateSubmitted { .. } => "claw10.memory.submitted",
            Claw10Event::MemoryActivated { .. } => "claw10.memory.activated",
            Claw10Event::MemoryRejected { .. } => "claw10.memory.rejected",
            Claw10Event::TaskVerified { .. } => "claw10.task.verified",
            Claw10Event::TaskFailed { .. } => "claw10.task.failed",
            Claw10Event::WorkerHeartbeat { .. } => "claw10.worker.heartbeat",
            Claw10Event::WorkerStale { .. } => "claw10.worker.stale",
        }
    }

    /// Timestamp event.
    #[must_use]
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Claw10Event::AgentSpawned { timestamp, .. }
            | Claw10Event::AgentHibernated { timestamp, .. }
            | Claw10Event::AgentWoken { timestamp, .. }
            | Claw10Event::AgentTerminated { timestamp, .. }
            | Claw10Event::AgentMigrated { timestamp, .. }
            | Claw10Event::SpawnRequestApproved { timestamp, .. }
            | Claw10Event::SpawnRequestDenied { timestamp, .. }
            | Claw10Event::ScheduleDue { timestamp, .. }
            | Claw10Event::MemoryCandidateSubmitted { timestamp, .. }
            | Claw10Event::MemoryActivated { timestamp, .. }
            | Claw10Event::MemoryRejected { timestamp, .. }
            | Claw10Event::TaskVerified { timestamp, .. }
            | Claw10Event::TaskFailed { timestamp, .. }
            | Claw10Event::WorkerHeartbeat { timestamp, .. }
            | Claw10Event::WorkerStale { timestamp, .. } => *timestamp,
        }
    }
}

/// Alasan agent dibangunkan dari hibernasi.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeTrigger {
    ScheduleDue,
    EventSubscription { event_type: String },
    ManualWake,
    Heartbeat,
}

/// Alasan terminasi agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    TaskCompleted,
    BudgetExhausted,
    TtlExpired,
    ParentTerminated,
    PolicyViolation,
    OperatorKill,
    Orphaned,
}
