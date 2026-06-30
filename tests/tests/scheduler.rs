use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use claw10_domain::{AgentId, Schedule, ScheduleAction};
use claw10_scheduler::ScheduleService;
use claw10_store::InMemoryStore;

fn make_agent_id() -> AgentId {
    AgentId(Uuid::now_v7())
}

fn make_svc() -> ScheduleService {
    let store = Arc::new(InMemoryStore::new()) as Arc<dyn claw10_store::Store>;
    ScheduleService::new(store)
}

#[tokio::test]
async fn test_add_and_list_schedule() {
    let svc = make_svc();
    let agent_id = make_agent_id();

    let schedule = Schedule {
        cron: "0 0 * * * *".into(),
        timezone: "UTC".into(),
        action: ScheduleAction::Wake,
    };

    svc.add_schedule(&agent_id, schedule.clone()).await.unwrap();
    let schedules = svc.list_schedules(&agent_id).await.unwrap();
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0].action, ScheduleAction::Wake);
}

#[tokio::test]
async fn test_remove_schedule() {
    let svc = make_svc();
    let agent_id = make_agent_id();

    svc.add_schedule(
        &agent_id,
        Schedule {
            cron: "0 0 * * * *".into(),
            timezone: "UTC".into(),
            action: ScheduleAction::Wake,
        },
    )
    .await
    .unwrap();

    svc.remove_schedule(&agent_id, 0).await.unwrap();
    let schedules = svc.list_schedules(&agent_id).await.unwrap();
    assert!(schedules.is_empty());
}

#[tokio::test]
async fn test_invalid_cron_expression() {
    let svc = make_svc();
    let agent_id = make_agent_id();

    let result = svc
        .add_schedule(
            &agent_id,
            Schedule {
                cron: "not-a-cron".into(),
                timezone: "UTC".into(),
                action: ScheduleAction::Checkpoint,
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_invalid_timezone() {
    let svc = make_svc();
    let agent_id = make_agent_id();

    let result = svc
        .add_schedule(
            &agent_id,
            Schedule {
                cron: "0 0 * * * *".into(),
                timezone: "Fake/Zone".into(),
                action: ScheduleAction::Checkpoint,
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_due_schedules() {
    let svc = make_svc();
    let agent_id = make_agent_id();

    svc.add_schedule(
        &agent_id,
        Schedule {
            cron: "* * * * * *".into(),
            timezone: "UTC".into(),
            action: ScheduleAction::Wake,
        },
    )
    .await
    .unwrap();

    let now = Utc::now();
    let due = svc.get_due_schedules(&now).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].agent_id, agent_id);
    assert_eq!(due[0].schedule.action, ScheduleAction::Wake);
}

#[tokio::test]
async fn test_get_due_schedules_filters_non_matching() {
    let svc = make_svc();

    svc.add_schedule(
        &make_agent_id(),
        Schedule {
            cron: "0 0 1 1 * *".into(),
            timezone: "UTC".into(),
            action: ScheduleAction::Review,
        },
    )
    .await
    .unwrap();

    let now = Utc::now();
    let due = svc.get_due_schedules(&now).await.unwrap();
    assert!(due.is_empty());
}

#[tokio::test]
async fn test_multiple_agents_schedules() {
    let svc = make_svc();
    let agent_a = make_agent_id();
    let agent_b = make_agent_id();

    svc.add_schedule(
        &agent_a,
        Schedule {
            cron: "* * * * * *".into(),
            timezone: "UTC".into(),
            action: ScheduleAction::Wake,
        },
    )
    .await
    .unwrap();

    svc.add_schedule(
        &agent_b,
        Schedule {
            cron: "* * * * * *".into(),
            timezone: "UTC".into(),
            action: ScheduleAction::Checkpoint,
        },
    )
    .await
    .unwrap();

    let now = Utc::now();
    let due = svc.get_due_schedules(&now).await.unwrap();
    assert_eq!(due.len(), 2);
}

#[tokio::test]
async fn test_schedule_all_actions() {
    let svc = make_svc();
    let agent_id = make_agent_id();
    let actions = vec![
        ScheduleAction::Wake,
        ScheduleAction::Review,
        ScheduleAction::Checkpoint,
        ScheduleAction::PolicyRenewal,
        ScheduleAction::CredentialRotation,
    ];

    for action in &actions {
        svc.add_schedule(
            &agent_id,
            Schedule {
                cron: "0 0 * * * *".into(),
                timezone: "UTC".into(),
                action: action.clone(),
            },
        )
        .await
        .unwrap();
    }

    let schedules = svc.list_schedules(&agent_id).await.unwrap();
    assert_eq!(schedules.len(), 5);

    for (i, action) in actions.iter().enumerate() {
        assert_eq!(&schedules[i].action, action);
    }
}
