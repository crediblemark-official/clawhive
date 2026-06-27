use tracing::warn;

use clawhive_domain::{
    Agent, Evidence, Lineage, Memory, Mission, PolicyBundle, Skill, Task, Worker,
};
use clawhive_toon::{ToonContext, ToonEncoder};

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("Token budget exhausted: needed {needed}, available {available}")]
    TokenBudgetExhausted { needed: usize, available: usize },
    #[error("No suitable context sources available")]
    NoSources,
}

/// Defines what context sources are available for building.
#[derive(Debug, Default)]
pub struct ContextSources<'a> {
    pub task: Option<&'a Task>,
    pub mission: Option<&'a Mission>,
    pub memories: &'a [Memory],
    pub policies: &'a [PolicyBundle],
    pub skills: &'a [Skill],
    pub history: &'a [String],
    pub tools: &'a [String],
    pub agents: &'a [Agent],
    pub lineage: Option<&'a Lineage>,
    pub workers: &'a [Worker],
    pub evidence: &'a [Evidence],
}

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub max_token_budget: usize,
    pub include_task: bool,
    pub include_mission: bool,
    pub include_memories: bool,
    pub include_policy: bool,
    pub include_skills: bool,
    pub include_history: bool,
    pub include_tools: bool,
    pub include_agent: bool,
    pub include_lineage: bool,
    pub include_workers: bool,
    pub include_evidence: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_token_budget: 4096,
            include_task: true,
            include_mission: true,
            include_memories: true,
            include_policy: true,
            include_skills: true,
            include_history: true,
            include_tools: true,
            include_agent: true,
            include_lineage: true,
            include_workers: false,
            include_evidence: true,
        }
    }
}

pub struct ContextPipeline {
    config: PipelineConfig,
}

impl ContextPipeline {
    #[must_use]
    pub fn new(config: PipelineConfig) -> Self {
        Self { config }
    }

    /// Build a complete context string from available sources.
    /// Orchestrates selection -> encoding -> assembly within token budget.
    pub async fn build_context(&self, sources: ContextSources<'_>) -> Result<String, ContextError> {
        let mut ctx = ToonContext::new();

        if self.config.include_task {
            if let Some(task) = sources.task {
                ctx.add_section("task", ToonEncoder::encode_task(task));
            }
        }

        if self.config.include_mission {
            if let Some(mission) = sources.mission {
                ctx.add_section("mission", ToonEncoder::encode_mission(mission));
            }
        }

        if self.config.include_memories && !sources.memories.is_empty() {
            ctx.add_section("memory", ToonEncoder::encode_memories(sources.memories));
        }

        if self.config.include_policy && !sources.policies.is_empty() {
            ctx.add_section(
                "policy",
                ToonEncoder::encode_policy_summary(sources.policies),
            );
        }

        if self.config.include_skills && !sources.skills.is_empty() {
            ctx.add_section("skills", ToonEncoder::encode_skills(sources.skills));
        }

        if self.config.include_history && !sources.history.is_empty() {
            ctx.add_section("history", ToonEncoder::encode_history(sources.history));
        }

        if self.config.include_tools && !sources.tools.is_empty() {
            ctx.add_section("tools", ToonEncoder::encode_tools(sources.tools));
        }

        if self.config.include_agent && !sources.agents.is_empty() {
            ctx.add_section("agents", ToonEncoder::encode_agent_roster(sources.agents));
        }

        if self.config.include_lineage {
            if let Some(lineage) = sources.lineage {
                ctx.add_section("lineage", ToonEncoder::encode_lineage(lineage));
            }
        }

        if self.config.include_workers && !sources.workers.is_empty() {
            ctx.add_section("workers", ToonEncoder::encode_workers(sources.workers));
        }

        if self.config.include_evidence && !sources.evidence.is_empty() {
            ctx.add_section("evidence", ToonEncoder::encode_evidence(sources.evidence));
        }

        let context = ctx.build();
        let estimated_tokens = estimate_tokens(&context);

        if estimated_tokens > self.config.max_token_budget {
            warn!(
                estimated_tokens,
                max_budget = self.config.max_token_budget,
                "Context exceeds token budget, trimming"
            );
            Ok(self.trim_to_budget(&context, self.config.max_token_budget))
        } else {
            Ok(context)
        }
    }

    /// Trim context string to fit within token budget by truncating earliest/least important sections.
    fn trim_to_budget(&self, context: &str, max_tokens: usize) -> String {
        let sections: Vec<&str> = context.split("\n---\n").collect();
        let mut selected = Vec::new();
        let mut running_tokens = 0usize;

        for section in sections.iter().rev() {
            let section_tokens = estimate_tokens(section);
            if running_tokens + section_tokens <= max_tokens {
                selected.push(*section);
                running_tokens += section_tokens;
            } else {
                let available = max_tokens.saturating_sub(running_tokens);
                if available > 10 {
                    let chars_per_token = 4;
                    let max_chars = available * chars_per_token;
                    let truncated: String = section.chars().take(max_chars).collect();
                    selected.push(&*Box::leak(truncated.into_boxed_str()));
                }
                break;
            }
        }

        selected.reverse();
        selected.join("\n---\n")
    }
}

/// Rough token estimate: 1 token ≈ 4 characters.
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use clawhive_domain::{
        AgentId, Budget, IdentityId, LifecycleMode, Memory,
        MemoryId, MemorySource, MemoryStatus, MemoryType, Mission, MissionId,
        MissionState, OrganizationId, PolicyBundle, PolicyBundleId,
        PolicyEffect, PolicyRule, PolicyRuleId, PolicySubject, RetryPolicy, RiskLevel,
        Skill, SkillCostProfile, SkillId, SkillState, Task, TaskId, TaskState,
    };
    use chrono::Utc;

    fn dummy_task() -> Task {
        Task {
            id: TaskId(uuid::Uuid::now_v7()),
            mission_id: MissionId(uuid::Uuid::now_v7()),
            parent_task_id: None,
            owner_id: AgentId(uuid::Uuid::now_v7()),
            objective: "test task".into(),
            dependencies: vec![],
            risk: RiskLevel("medium".into()),
            budget: Budget::default(),
            deadline: None,
            input: serde_json::Value::Null,
            output_contract: serde_json::Value::Null,
            evidence_contract: vec![],
            retry_policy: RetryPolicy {
                max_retries: 0,
                backoff_seconds: 0,
            },
            idempotency_key: None,
            lifecycle_mode: "persistent".into(),
            state: TaskState::Created,
            evidence: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn dummy_mission() -> Mission {
        Mission {
            id: MissionId(uuid::Uuid::now_v7()),
            organization_id: OrganizationId(uuid::Uuid::now_v7()),
            owner_id: IdentityId(uuid::Uuid::now_v7()),
            objective: "an objective".into(),
            scope: None,
            lifecycle_mode: LifecycleMode::Persistent,
            campaign_end: None,
            review_interval_days: None,
            budget: Budget::default(),
            risk: RiskLevel("medium".into()),
            require_evidence: false,
            minimum_verifiers: 1,
            state: MissionState::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_build_context_with_task_and_mission() {
        let pipeline = ContextPipeline::new(PipelineConfig::default());
        let task = dummy_task();
        let mission = dummy_mission();
        let sources = ContextSources {
            task: Some(&task),
            mission: Some(&mission),
            ..ContextSources::default()
        };
        let context = pipeline.build_context(sources).await.unwrap();
        assert!(!context.is_empty());
        assert!(context.contains("test task"));
        assert!(context.contains("an objective"));
    }

    #[tokio::test]
    async fn test_build_context_empty_sources() {
        let pipeline = ContextPipeline::new(PipelineConfig::default());
        let sources = ContextSources::default();
        let context = pipeline.build_context(sources).await.unwrap();
        assert!(context.contains("[TOON v1]"));
    }

    #[tokio::test]
    async fn test_token_budget_trimming() {
        let config = PipelineConfig {
            max_token_budget: 1,
            ..Default::default()
        };
        let pipeline = ContextPipeline::new(config);
        let history = vec!["very long message to trigger trimming".into()];
        let sources = ContextSources {
            history: &history,
            ..ContextSources::default()
        };
        // Should return without panicking even with tight budget
        let result = pipeline.build_context(sources).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_selective_sources() {
        let config = PipelineConfig {
            include_mission: false,
            include_memories: false,
            include_policy: false,
            include_skills: false,
            include_history: false,
            include_tools: false,
            include_agent: false,
            include_lineage: false,
            include_workers: false,
            include_evidence: false,
            ..Default::default()
        };
        let pipeline = ContextPipeline::new(config);
        let task = dummy_task();
        let sources = ContextSources {
            task: Some(&task),
            ..ContextSources::default()
        };
        let context = pipeline.build_context(sources).await.unwrap();
        assert!(context.contains("test task"));
    }

    #[tokio::test]
    async fn test_context_with_memories() {
        let pipeline = ContextPipeline::new(PipelineConfig::default());
        let memory = Memory {
            id: MemoryId(uuid::Uuid::now_v7()),
            tenant_id: "t1".into(),
            scope: "test".into(),
            memory_type: MemoryType::Semantic,
            content: "key insight".into(),
            source: MemorySource {
                agent_id: AgentId(uuid::Uuid::nil()),
                task_id: TaskId(uuid::Uuid::nil()),
                evidence_id: None,
            },
            confidence: 0.9,
            classification: "public".into(),
            status: MemoryStatus::Active,
            verified_by: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let sources = ContextSources {
            memories: &[memory],
            ..ContextSources::default()
        };
        let context = pipeline.build_context(sources).await.unwrap();
        assert!(context.contains("[memory]"));
        assert!(context.contains("key insight"));
    }

    #[tokio::test]
    async fn test_context_with_policies() {
        let pipeline = ContextPipeline::new(PipelineConfig::default());
        let policy = PolicyBundle {
            id: PolicyBundleId(uuid::Uuid::now_v7()),
            name: "main".into(),
            version: "1.0".into(),
            is_active: true,
            rules: vec![PolicyRule {
                id: PolicyRuleId(uuid::Uuid::now_v7()),
                subject: PolicySubject::Role("admin".into()),
                effect: PolicyEffect::Allow,
                action: "execute:*".into(),
                resource: "tool:http".into(),
                condition: None,
                priority: 100,
            }],
            signed_by: None,
            signature: None,
            activated_at: Some(Utc::now()),
            created_at: Utc::now(),
        };
        let sources = ContextSources {
            policies: &[policy],
            ..ContextSources::default()
        };
        let context = pipeline.build_context(sources).await.unwrap();
        assert!(context.contains("[policy]"));
    }

    #[tokio::test]
    async fn test_context_with_skills() {
        let pipeline = ContextPipeline::new(PipelineConfig::default());
        let skill = Skill {
            id: SkillId(uuid::Uuid::now_v7()),
            name: "search".into(),
            purpose: "search web".into(),
            version: "1.0".into(),
            input_schema: serde_json::Value::Null,
            output_schema: serde_json::Value::Null,
            steps: vec![],
            required_tools: vec![],
            required_permissions: vec![],
            state: SkillState::Active,
            signature: None,
            cost_profile: SkillCostProfile {
                estimated_cost_usd: 0.01,
                average_duration_seconds: 1.0,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let sources = ContextSources {
            skills: &[skill],
            ..ContextSources::default()
        };
        let context = pipeline.build_context(sources).await.unwrap();
        assert!(context.contains("[skills]"));
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }
}
