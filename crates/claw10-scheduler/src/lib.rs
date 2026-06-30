use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use cron::Schedule as CronSchedule;
use uuid::Uuid;

use claw10_domain::{AgentId, Schedule};
use claw10_store::{Store, StoreError, StoreExt};

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("agent not found: {0}")]
    AgentNotFound(String),
    #[error("schedule not found for agent {agent_id}: {schedule_id}")]
    ScheduleNotFound {
        agent_id: String,
        schedule_id: String,
    },
    #[error("timezone parsing failed: {0}")]
    InvalidTimezone(String),
    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for SchedulerError {
    fn from(e: StoreError) -> Self {
        Self::Other(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct DueSchedule {
    pub agent_id: AgentId,
    pub schedule: Schedule,
}

const KEY_PREFIX: &str = "schedule:";

pub struct ScheduleService {
    store: Arc<dyn Store>,
}

impl ScheduleService {
    #[must_use]
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    fn agent_key(agent_id: &AgentId) -> String {
        format!("{KEY_PREFIX}{}", agent_id.0)
    }

    /// Add a schedule for an agent.
    ///
    /// # Errors
    /// Returns `SchedulerError::InvalidCron` if the cron expression is invalid.
    /// Returns `SchedulerError::InvalidTimezone` if the timezone is invalid.
    pub async fn add_schedule(
        &self,
        agent_id: &AgentId,
        schedule: Schedule,
    ) -> Result<(), SchedulerError> {
        // Validate cron expression
        CronSchedule::from_str(&schedule.cron)
            .map_err(|e| SchedulerError::InvalidCron(e.to_string()))?;

        // Validate timezone
        schedule
            .timezone
            .parse::<chrono_tz::Tz>()
            .map_err(|e| SchedulerError::InvalidTimezone(e.to_string()))?;

        let key = Self::agent_key(agent_id);
        let mut schedules: Vec<Schedule> = self.store.get(&key).await?.unwrap_or_default();
        schedules.push(schedule);
        self.store.set(&key, &schedules).await?;

        Ok(())
    }

    /// Remove a schedule from an agent by index.
    ///
    /// # Errors
    /// Returns `SchedulerError::AgentNotFound` if the agent has no schedules.
    /// Returns `SchedulerError::ScheduleNotFound` if the index is out of bounds.
    pub async fn remove_schedule(
        &self,
        agent_id: &AgentId,
        schedule_index: usize,
    ) -> Result<(), SchedulerError> {
        let key = Self::agent_key(agent_id);
        let mut schedules: Vec<Schedule> = self
            .store
            .get(&key)
            .await?
            .ok_or(SchedulerError::AgentNotFound(agent_id.0.to_string()))?;

        if schedule_index >= schedules.len() {
            return Err(SchedulerError::ScheduleNotFound {
                agent_id: agent_id.0.to_string(),
                schedule_id: schedule_index.to_string(),
            });
        }

        schedules.remove(schedule_index);
        if schedules.is_empty() {
            self.store.delete(&key).await?;
        } else {
            self.store.set(&key, &schedules).await?;
        }

        Ok(())
    }

    /// List all schedules for an agent.
    pub async fn list_schedules(&self, agent_id: &AgentId) -> Result<Vec<Schedule>, SchedulerError> {
        let key = Self::agent_key(agent_id);
        Ok(self.store.get::<Vec<Schedule>>(&key).await?.unwrap_or_default())
    }

    /// Get all schedules that are due at the given time.
    pub async fn get_due_schedules(
        &self,
        now: &DateTime<Utc>,
    ) -> Result<Vec<DueSchedule>, SchedulerError> {
        let all: Vec<(String, Vec<Schedule>)> = self.store.scan_prefix(KEY_PREFIX).await?;
        let mut due = Vec::new();

        for (agent_key, agent_schedules) in &all {
            let agent_id_str = agent_key
                .strip_prefix(KEY_PREFIX)
                .unwrap_or(agent_key);
            let Ok(agent_uuid) = Uuid::parse_str(agent_id_str) else {
                continue;
            };
            let agent_id = AgentId(agent_uuid);

            for schedule in agent_schedules {
                if Self::is_schedule_due(schedule, now) {
                    due.push(DueSchedule {
                        agent_id: agent_id.clone(),
                        schedule: schedule.clone(),
                    });
                }
            }
        }

        Ok(due)
    }

    /// Check if a schedule is due at the given time.
    #[must_use]
    pub fn is_schedule_due(schedule: &Schedule, now: &DateTime<Utc>) -> bool {
        let Ok(cron) = CronSchedule::from_str(&schedule.cron) else {
            return false;
        };

        let Ok(tz) = schedule.timezone.parse::<chrono_tz::Tz>() else {
            return false;
        };

        let local_now = now.with_timezone(&tz);

        let Some(upcoming) = cron.upcoming(tz).next() else {
            return false;
        };

        let diff = (upcoming - local_now).num_seconds().abs();
        diff <= 60
    }
}
