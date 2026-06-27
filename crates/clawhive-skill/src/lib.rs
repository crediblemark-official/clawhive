use std::sync::Arc;

use chrono::Utc;

use clawhive_domain::{
    Permission, Skill, SkillCostProfile, SkillId, SkillState, SkillVersion,
};
use clawhive_store::{Store, StoreError, StoreExt};

const KEY_PREFIX: &str = "skill:";
const VERSION_PREFIX: &str = "skill:ver:";

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("invalid state transition: {from:?} -> {to:?}")]
    InvalidTransition { from: SkillState, to: SkillState },
    #[error("unsigned skills cannot be made active")]
    Unsigned,
    #[error("{0}")]
    Store(#[from] StoreError),
}

pub struct SkillService {
    store: Arc<dyn Store>,
}

impl SkillService {
    #[must_use]
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    /// Create a new skill in Candidate state.
    pub async fn create_skill(
        &self,
        name: String,
        purpose: String,
        version: String,
        input_schema: serde_json::Value,
        output_schema: serde_json::Value,
        steps: Vec<String>,
        required_tools: Vec<String>,
        required_permissions: Vec<Permission>,
        cost_profile: SkillCostProfile,
    ) -> Result<Skill, SkillError> {
        let now = Utc::now();
        let skill = Skill {
            id: SkillId(uuid::Uuid::now_v7()),
            name,
            purpose,
            version,
            input_schema,
            output_schema,
            steps,
            required_tools,
            required_permissions,
            state: SkillState::Candidate,
            signature: None,
            cost_profile,
            created_at: now,
            updated_at: now,
        };
        let key = format!("{KEY_PREFIX}{}", skill.id.0);
        self.store.set(&key, &skill).await?;
        Ok(skill)
    }

    /// Transition a skill through its lifecycle states.
    ///
    /// Valid transitions:
    /// Candidate -> Scanning
    /// Scanning -> Testing | Rejected
    /// Testing -> Review | Failed
    /// Review -> Approved | Rejected
    /// Approved -> Staged
    /// Staged -> Active (requires signature)
    /// Active -> Deprecated | Quarantined
    /// Deprecated -> Retired | Active
    /// Quarantined -> Scanning | Retired
    pub async fn transition_state(
        &self,
        id: &SkillId,
        to: SkillState,
    ) -> Result<Skill, SkillError> {
        let key = format!("{KEY_PREFIX}{}", id.0);
        let mut skill = self
            .store
            .get::<Skill>(&key)
            .await?
            .ok_or_else(|| SkillError::NotFound(id.0.to_string()))?;

        if !is_valid_transition(&skill.state, &to) {
            return Err(SkillError::InvalidTransition {
                from: skill.state,
                to: to.clone(),
            });
        }

        // Active requires signature (FR-064)
        if to == SkillState::Active && skill.signature.is_none() {
            return Err(SkillError::Unsigned);
        }

        skill.state = to;
        skill.updated_at = Utc::now();
        self.store.set(&key, &skill).await?;
        Ok(skill)
    }

    /// Sign a skill so it can be activated.
    pub async fn sign_skill(
        &self,
        id: &SkillId,
        signature: String,
    ) -> Result<Skill, SkillError> {
        let key = format!("{KEY_PREFIX}{}", id.0);
        let mut skill = self
            .store
            .get::<Skill>(&key)
            .await?
            .ok_or_else(|| SkillError::NotFound(id.0.to_string()))?;

        skill.signature = Some(signature);
        skill.updated_at = Utc::now();
        self.store.set(&key, &skill).await?;
        Ok(skill)
    }

    /// Record a new version entry for this skill.
    pub async fn create_version(
        &self,
        skill_id: &SkillId,
        version: String,
        previous_version: Option<String>,
        change_log: String,
    ) -> Result<SkillVersion, SkillError> {
        let skill = self
            .get_skill(skill_id)
            .await?
            .ok_or_else(|| SkillError::NotFound(skill_id.0.to_string()))?;

        let sv = SkillVersion {
            skill_id: skill.id,
            version,
            previous_version,
            change_log,
            is_deprecated: false,
            created_at: Utc::now(),
        };
        let key = format!("{VERSION_PREFIX}{}:{}", sv.skill_id.0, sv.version);
        self.store.set(&key, &sv).await?;
        Ok(sv)
    }

    pub async fn get_skill(&self, id: &SkillId) -> Result<Option<Skill>, SkillError> {
        let key = format!("{KEY_PREFIX}{}", id.0);
        Ok(self.store.get::<Skill>(&key).await?)
    }

    pub async fn list_skills(
        &self,
        state_filter: Option<SkillState>,
    ) -> Result<Vec<Skill>, SkillError> {
        let all: Vec<(String, Skill)> = self.store.scan_prefix(KEY_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, s)| s)
            .filter(|s| match &state_filter {
                Some(state) => &s.state == state,
                None => true,
            })
            .collect())
    }

    pub async fn list_by_required_tool(&self, tool_name: &str) -> Result<Vec<Skill>, SkillError> {
        let all: Vec<(String, Skill)> = self.store.scan_prefix(KEY_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, s)| s)
            .filter(|s| s.required_tools.iter().any(|t| t == tool_name))
            .collect())
    }

    pub async fn count_by_state(
        &self,
    ) -> Result<std::collections::HashMap<String, usize>, SkillError> {
        let all: Vec<(String, Skill)> = self.store.scan_prefix(KEY_PREFIX).await?;
        let mut counts = std::collections::HashMap::new();
        for (_, skill) in all {
            *counts.entry(format!("{:?}", skill.state)).or_insert(0) += 1;
        }
        Ok(counts)
    }
}

fn is_valid_transition(from: &SkillState, to: &SkillState) -> bool {
    use SkillState::*;
    matches!(
        (from, to),
        (Candidate, Scanning)
            | (Scanning, Testing)
            | (Scanning, Rejected)
            | (Testing, Review)
            | (Testing, Failed)
            | (Review, Approved)
            | (Review, Rejected)
            | (Approved, Staged)
            | (Staged, Active)
            | (Active, Deprecated)
            | (Active, Quarantined)
            | (Deprecated, Retired)
            | (Deprecated, Active)
            | (Quarantined, Scanning)
            | (Quarantined, Retired)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_svc() -> SkillService {
        let store = Arc::new(clawhive_store::InMemoryStore::new()) as Arc<dyn Store>;
        SkillService::new(store)
    }

    fn sample_skill(svc: &SkillService) -> impl std::future::Future<Output = Skill> + use<'_> {
        // clippy: ok

        async {
            svc.create_skill(
                "web-search".into(),
                "Search the web".into(),
                "1.0.0".into(),
                serde_json::json!({"query": "string"}),
                serde_json::json!({"results": "array"}),
                vec!["search".into()],
                vec!["http".into()],
                vec![],
                SkillCostProfile {
                    estimated_cost_usd: 0.01,
                    average_duration_seconds: 2.0,
                },
            )
            .await
            .unwrap()
        }
    }

    #[tokio::test]
    async fn test_create_skill() {
        let svc = make_svc();
        let skill = sample_skill(&svc).await;
        assert_eq!(skill.name, "web-search");
        assert_eq!(skill.state, SkillState::Candidate);
    }

    #[tokio::test]
    async fn test_full_lifecycle() {
        let svc = make_svc();
        let skill = sample_skill(&svc).await;

        let s = svc.transition_state(&skill.id, SkillState::Scanning).await.unwrap();
        assert_eq!(s.state, SkillState::Scanning);

        let s = svc.transition_state(&skill.id, SkillState::Testing).await.unwrap();
        assert_eq!(s.state, SkillState::Testing);

        let s = svc.transition_state(&skill.id, SkillState::Review).await.unwrap();
        assert_eq!(s.state, SkillState::Review);

        let s = svc.transition_state(&skill.id, SkillState::Approved).await.unwrap();
        assert_eq!(s.state, SkillState::Approved);

        let s = svc.transition_state(&skill.id, SkillState::Staged).await.unwrap();
        assert_eq!(s.state, SkillState::Staged);

        // Must sign before Active
        svc.sign_skill(&skill.id, "sig-abc123".into()).await.unwrap();
        let s = svc.transition_state(&skill.id, SkillState::Active).await.unwrap();
        assert_eq!(s.state, SkillState::Active);

        let s = svc.transition_state(&skill.id, SkillState::Deprecated).await.unwrap();
        assert_eq!(s.state, SkillState::Deprecated);

        let s = svc.transition_state(&skill.id, SkillState::Retired).await.unwrap();
        assert_eq!(s.state, SkillState::Retired);
    }

    #[tokio::test]
    async fn test_invalid_transition() {
        let svc = make_svc();
        let skill = sample_skill(&svc).await;

        // Candidate -> Active (skip all intermediate)
        let result = svc.transition_state(&skill.id, SkillState::Active).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unsigned_cannot_activate() {
        let svc = make_svc();
        let skill = sample_skill(&svc).await;

        svc.transition_state(&skill.id, SkillState::Scanning).await.unwrap();
        svc.transition_state(&skill.id, SkillState::Testing).await.unwrap();
        svc.transition_state(&skill.id, SkillState::Review).await.unwrap();
        svc.transition_state(&skill.id, SkillState::Approved).await.unwrap();
        svc.transition_state(&skill.id, SkillState::Staged).await.unwrap();

        // No signature -> cannot activate
        let result = svc.transition_state(&skill.id, SkillState::Active).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SkillError::Unsigned));
    }

    #[tokio::test]
    async fn test_list_skills_with_filter() {
        let svc = make_svc();
        let s1 = sample_skill(&svc).await;
        let _s2 = sample_skill(&svc).await;

        svc.transition_state(&s1.id, SkillState::Scanning).await.unwrap();
        // _s2 stays Candidate

        let candidates = svc.list_skills(Some(SkillState::Candidate)).await.unwrap();
        assert_eq!(candidates.len(), 1);

        let all = svc.list_skills(None).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_list_by_required_tool() {
        let svc = make_svc();
        let _ = sample_skill(&svc).await; // requires "http"

        let with_http = svc.list_by_required_tool("http").await.unwrap();
        assert_eq!(with_http.len(), 1);

        let with_db = svc.list_by_required_tool("database").await.unwrap();
        assert_eq!(with_db.len(), 0);
    }

    #[tokio::test]
    async fn test_count_by_state() {
        let svc = make_svc();
        let s1 = sample_skill(&svc).await;
        let _s2 = sample_skill(&svc).await;
        svc.transition_state(&s1.id, SkillState::Scanning).await.unwrap();

        let counts = svc.count_by_state().await.unwrap();
        assert_eq!(*counts.get("Candidate").unwrap_or(&0), 1);
        assert_eq!(*counts.get("Scanning").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_create_version() {
        let svc = make_svc();
        let skill = sample_skill(&svc).await;

        let ver = svc
            .create_version(&skill.id, "1.0.0".into(), None, "initial release".into())
            .await
            .unwrap();
        assert_eq!(ver.version, "1.0.0");
        assert!(ver.previous_version.is_none());
    }

    #[tokio::test]
    async fn test_sign_skill() {
        let svc = make_svc();
        let skill = sample_skill(&svc).await;
        let signed = svc.sign_skill(&skill.id, "test-signature".into()).await.unwrap();
        assert_eq!(signed.signature.as_deref(), Some("test-signature"));
    }
}
