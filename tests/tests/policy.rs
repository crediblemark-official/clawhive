use uuid::Uuid;

use clawhive_domain::{
    PolicyBundle, PolicyBundleId, PolicyEffect, PolicyRule, PolicyRuleId, PolicySubject,
};
use clawhive_policy::PolicyService;

fn make_rule(
    id: u128,
    subject: PolicySubject,
    effect: PolicyEffect,
    action: &str,
    resource: &str,
    priority: u32,
) -> PolicyRule {
    PolicyRule {
        id: PolicyRuleId(Uuid::from_u128(id)),
        subject,
        effect,
        action: action.into(),
        resource: resource.into(),
        condition: None,
        priority,
    }
}

fn make_bundle(rules: Vec<PolicyRule>) -> PolicyBundle {
    let mut bundle = PolicyBundle {
        id: PolicyBundleId(Uuid::now_v7()),
        name: "test-bundle".into(),
        version: "1.0".into(),
        rules,
        is_active: false,
        signed_by: None,
        signature: None,
        activated_at: None,
        created_at: chrono::Utc::now(),
    };
    PolicyService::activate(&mut bundle);
    bundle
}

#[test]
fn test_allow_action() {
    let bundle = make_bundle(vec![make_rule(
        1,
        PolicySubject::Role("admin".into()),
        PolicyEffect::Allow,
        "read",
        "secrets",
        100,
    )]);

    let result = PolicyService::evaluate(
        &bundle,
        &PolicySubject::Role("admin".into()),
        "read",
        "secrets",
        None,
    )
    .unwrap();

    assert!(result.allowed);
    assert!(result.reason.contains("allowed by rule"));
}

#[test]
fn test_deny_action() {
    let bundle = make_bundle(vec![make_rule(
        1,
        PolicySubject::Role("admin".into()),
        PolicyEffect::ExplicitDeny,
        "delete",
        "database",
        100,
    )]);

    let result = PolicyService::evaluate(
        &bundle,
        &PolicySubject::Role("admin".into()),
        "delete",
        "database",
        None,
    )
    .unwrap();

    assert!(!result.allowed);
}

#[test]
fn test_default_deny() {
    let bundle = make_bundle(vec![]);

    let result = PolicyService::evaluate(
        &bundle,
        &PolicySubject::Role("admin".into()),
        "read",
        "anything",
        None,
    )
    .unwrap();

    assert!(!result.allowed);
    assert_eq!(result.reason, "no matching rule — default deny");
}

#[test]
fn test_priority_wildcard() {
    let bundle = make_bundle(vec![
        make_rule(
            1,
            PolicySubject::Role("*".into()),
            PolicyEffect::Allow,
            "read",
            "*",
            50,
        ),
        make_rule(
            2,
            PolicySubject::Role("guest".into()),
            PolicyEffect::ExplicitDeny,
            "read",
            "secrets",
            100,
        ),
    ]);

    // Guest should be denied due to higher-priority explicit deny
    let result = PolicyService::evaluate(
        &bundle,
        &PolicySubject::Role("guest".into()),
        "read",
        "secrets",
        None,
    )
    .unwrap();

    assert!(!result.allowed);
}

#[test]
fn test_pattern_matching_wildcard() {
    assert!(PolicyService::pattern_matches("*", "anything"));
}

#[test]
fn test_pattern_matching_exact() {
    assert!(PolicyService::pattern_matches("exact", "exact"));
}

#[test]
fn test_pattern_matching_prefix() {
    assert!(PolicyService::pattern_matches("read:*", "read:secrets"));
    assert!(!PolicyService::pattern_matches("read:*", "write:secrets"));
}

#[test]
fn test_pattern_matching_suffix() {
    assert!(PolicyService::pattern_matches("*.log", "access.log"));
    assert!(!PolicyService::pattern_matches("*.log", "access.txt"));
}

#[test]
fn test_inactive_bundle_rejected() {
    let bundle = PolicyBundle {
        id: PolicyBundleId(Uuid::now_v7()),
        name: "inactive".into(),
        version: "1.0".into(),
        rules: vec![],
        is_active: false,
        signed_by: None,
        signature: None,
        activated_at: None,
        created_at: chrono::Utc::now(),
    };

    let result = PolicyService::evaluate(
        &bundle,
        &PolicySubject::Role("admin".into()),
        "read",
        "anything",
        None,
    );

    assert!(result.is_err());
}

#[test]
fn test_simulate() {
    let rules = vec![make_rule(
        1,
        PolicySubject::Role("dev".into()),
        PolicyEffect::Allow,
        "deploy",
        "staging",
        100,
    )];

    let result = PolicyService::simulate(
        &rules,
        &PolicySubject::Role("dev".into()),
        "deploy",
        "staging",
        None,
    );

    assert!(result.allowed);
}

#[test]
fn test_compile_sorts_by_priority() {
    let rules = vec![
        make_rule(
            1,
            PolicySubject::Role("a".into()),
            PolicyEffect::Allow,
            "r",
            "*",
            10,
        ),
        make_rule(
            2,
            PolicySubject::Role("b".into()),
            PolicyEffect::Allow,
            "r",
            "*",
            100,
        ),
    ];

    let compiled = PolicyService::compile(rules);
    assert_eq!(compiled[0].priority, 100);
    assert_eq!(compiled[1].priority, 10);
}
