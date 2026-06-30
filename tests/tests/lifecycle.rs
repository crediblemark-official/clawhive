use chrono::{Duration, Utc};
use uuid::Uuid;

use claw10_domain::{
    Agent, AgentGenome, AgentId, AgentState, AutonomyConfig, Budget, CheckpointReason, IdentityId,
    LifecycleMode, MemoryConfig, ModelPolicy, NetworkPolicy, PolicyBundle,
    PolicyBundleId, RuntimeConfig,
};
use claw10_lifecycle::{LifecycleError, LifecycleService};

fn make_test_agent() -> Agent {
    let now = Utc::now();
    Agent {
        id: AgentId(Uuid::now_v7()),
        identity_id: IdentityId(Uuid::now_v7()),
        mission_id: claw10_domain::MissionId(Uuid::now_v7()),
        parent_agent_id: None,
        lineage_id: claw10_domain::LineageId(Uuid::now_v7()),
        name: "test-agent".into(),
        role: "tester".into(),
        genome: AgentGenome {
            id: "test-genome".into(),
            version: "1.0".into(),
            role: "tester".into(),
            lifecycle_modes: vec![LifecycleMode::Persistent],
            model_policy: ModelPolicy {
                preferred_profile: "gpt-4".into(),
                fallback_profiles: vec![],
                max_context_tokens: 4096,
            },
            autonomy: AutonomyConfig {
                can_spawn: false,
                max_spawn_depth: 0,
                max_children: 0,
            },
            delegable_permissions: vec![],
            non_delegable_permissions: vec![],
            memory: MemoryConfig {
                default_read_scopes: vec![],
                default_write_scope: None,
            },
            runtime: RuntimeConfig {
                preferred_class: "standard".into(),
                network: NetworkPolicy::AllowByDefault,
            },
            verification_required: false,
        },
        state: AgentState::Active,
        lifecycle_mode: LifecycleMode::Persistent,
        persistent_pattern: None,
        budget: Budget {
            allocated_usd: 100.0,
            spent_usd: 0.0,
            soft_limit_usd: None,
            hard_limit_usd: None,
            recurring_monthly_usd: None,
        },
        delegable_permissions: vec![],
        non_delegable_permissions: vec![],
        current_runtime: Some(claw10_domain::RuntimeLease {
            worker_id: "worker-1".into(),
            acquired_at: now,
            expires_at: now + Duration::seconds(60),
            renewal_interval_seconds: 60,
        }),
        checkpoints: vec![],
        subscriptions: vec![],
        schedules: vec![],
        policy_bundle: PolicyBundle {
            id: PolicyBundleId(Uuid::now_v7()),
            name: "default".into(),
            version: "1.0".into(),
            rules: vec![],
            is_active: true,
            signed_by: None,
            signature: None,
            activated_at: None,
            created_at: now,
        },
        turn_count: 10,
        total_cost_usd: 5.0,
        created_at: now,
        updated_at: now,
        terminated_at: None,
    }
}

fn make_lease(worker_id: &str) -> claw10_domain::RuntimeLease {
    claw10_domain::RuntimeLease {
        worker_id: worker_id.into(),
        acquired_at: Utc::now(),
        expires_at: Utc::now() + Duration::seconds(120),
        renewal_interval_seconds: 120,
    }
}

#[test]
fn test_create_checkpoint() {
    let agent = make_test_agent();
    let cp = LifecycleService::create_checkpoint(&agent, CheckpointReason::PreHibernation);

    assert_eq!(cp.agent_id, agent.id.0.to_string());
    assert_eq!(cp.reason, CheckpointReason::PreHibernation);
    assert!(cp.state_snapshot.get("turn_count").is_some());
}

#[test]
fn test_restore_checkpoint() {
    let mut agent = make_test_agent();
    let cp = LifecycleService::create_checkpoint(&agent, CheckpointReason::PreHibernation);
    agent.turn_count = 99;

    LifecycleService::restore_checkpoint(&mut agent, &cp).unwrap();
    assert_eq!(agent.turn_count, 10); // restored from snapshot
}

#[test]
fn test_hibernate_and_wake() {
    let mut agent = make_test_agent();
    assert_eq!(agent.state, AgentState::Active);
    assert!(agent.current_runtime.is_some());

    let cp = LifecycleService::hibernate(&mut agent).unwrap();
    assert_eq!(agent.state, AgentState::Hibernating);
    assert!(agent.current_runtime.is_none());
    assert_eq!(cp.reason, CheckpointReason::PreHibernation);
    assert_eq!(agent.checkpoints.len(), 1);

    let lease = make_lease("worker-2");
    LifecycleService::wake(&mut agent, lease).unwrap();
    assert_eq!(agent.state, AgentState::Active);
    assert!(agent.current_runtime.is_some());
}

#[test]
fn test_hibernate_twice_fails() {
    let mut agent = make_test_agent();
    LifecycleService::hibernate(&mut agent).unwrap();
    let result = LifecycleService::hibernate(&mut agent);
    assert!(matches!(result, Err(LifecycleError::AlreadyHibernating)));
}

#[test]
fn test_wake_non_hibernating_fails() {
    let mut agent = make_test_agent();
    let lease = make_lease("worker-2");
    let result = LifecycleService::wake(&mut agent, lease);
    assert!(matches!(result, Err(LifecycleError::NotHibernating)));
}

#[test]
fn test_heartbeat_renews_lease() {
    let mut agent = make_test_agent();
    let original_expiry = agent.current_runtime.as_ref().unwrap().expires_at;

    // Sleep a tiny bit so renewed expiry is different
    std::thread::sleep(std::time::Duration::from_millis(10));

    let remaining = LifecycleService::heartbeat(&mut agent).unwrap();
    let new_expiry = agent.current_runtime.as_ref().unwrap().expires_at;

    assert!(new_expiry > original_expiry);
    assert!(remaining.num_seconds() > 0);
}

#[test]
fn test_heartbeat_expired_lease_fails() {
    let mut agent = make_test_agent();
    agent.current_runtime = Some(claw10_domain::RuntimeLease {
        worker_id: "worker-1".into(),
        acquired_at: Utc::now() - Duration::hours(2),
        expires_at: Utc::now() - Duration::seconds(10),
        renewal_interval_seconds: 60,
    });

    let result = LifecycleService::heartbeat(&mut agent);
    assert!(matches!(result, Err(LifecycleError::LeaseExpired)));
}

#[test]
fn test_detect_stale() {
    let fresh = make_test_agent();
    let mut stale = make_test_agent();
    stale.current_runtime = Some(claw10_domain::RuntimeLease {
        worker_id: "stale-worker".into(),
        acquired_at: Utc::now() - Duration::hours(2),
        expires_at: Utc::now() - Duration::seconds(30),
        renewal_interval_seconds: 60,
    });

    let agents = vec![fresh, stale];
    let stale_agents = LifecycleService::detect_stale(&agents, 5);
    assert_eq!(stale_agents.len(), 1);
}

#[test]
fn test_migrate() {
    let mut agent = make_test_agent();
    agent.lifecycle_mode = LifecycleMode::Persistent;

    let cp = LifecycleService::migrate(&mut agent, "worker-3", 300).unwrap();
    assert_eq!(agent.state, AgentState::Active);
    assert_eq!(
        agent.current_runtime.as_ref().unwrap().worker_id,
        "worker-3"
    );
    assert_eq!(cp.reason, CheckpointReason::PreMigration);
}

#[test]
fn test_migrate_non_persistent_fails() {
    let mut agent = make_test_agent();
    agent.lifecycle_mode = LifecycleMode::Ephemeral;

    let result = LifecycleService::migrate(&mut agent, "worker-3", 300);
    assert!(matches!(result, Err(LifecycleError::NotPersistent)));
}

#[test]
fn test_assign_lease() {
    let mut agent = make_test_agent();
    agent.current_runtime = None;

    LifecycleService::assign_lease(&mut agent, "worker-4", 120);
    assert!(agent.current_runtime.is_some());
    assert_eq!(agent.current_runtime.unwrap().worker_id, "worker-4");
}
