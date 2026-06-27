use thiserror::Error;

#[derive(Error, Debug)]
pub enum SpawnError {
    #[error("validation failed: {0}")]
    Validation(String),

    #[error("parent not active")]
    ParentNotActive,

    #[error("parent cannot spawn")]
    ParentCannotSpawn,

    #[error("spawn depth exceeded: max {max}, current {current}")]
    DepthExceeded { max: u32, current: u32 },

    #[error("swarm size exceeded")]
    SwarmSizeExceeded,

    #[error("budget insufficient: remaining {remaining}, required {required}")]
    BudgetInsufficient { remaining: f64, required: f64 },

    #[error("permission not delegable: {0}")]
    PermissionNotDelegable(String),

    #[error("duplicate objective: {0}")]
    DuplicateObjective(String),

    #[error("policy denied: {0}")]
    PolicyDenied(String),

    #[error("agent not found: {0}")]
    AgentNotFound(String),

    #[error("mission not found: {0}")]
    MissionNotFound(String),

    #[error("descendant termination failed: {0}")]
    DescendantTerminationFailed(String),

    #[error("spawn error: {0}")]
    Other(String),
}
