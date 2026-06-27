use std::sync::Arc;

use chrono::Utc;

use clawhive_domain::{AgentId, Artifact, ArtifactId, TaskId};
use clawhive_store::{Store, StoreError, StoreExt};

const META_PREFIX: &str = "artifact:meta:";
const CONTENT_PREFIX: &str = "artifact:content:";

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("artifact not found: {0}")]
    NotFound(String),
    #[error("content hash mismatch for artifact {0}")]
    ContentHashMismatch(String),
    #[error("{0}")]
    Store(#[from] StoreError),
}

pub struct ArtifactService {
    store: Arc<dyn Store>,
}

impl ArtifactService {
    #[must_use]
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    fn meta_key(id: &ArtifactId) -> String {
        format!("{META_PREFIX}{}", id.0)
    }

    fn content_key(id: &ArtifactId) -> String {
        format!("{CONTENT_PREFIX}{}", id.0)
    }

    fn compute_hash(content: &[u8]) -> String {
        use sha2::Digest;
        let hash = sha2::Sha256::digest(content);
        hash.iter().map(|b| format!("{:02x}", b)).collect::<String>()
    }

    pub async fn store_artifact(
        &self,
        task_id: TaskId,
        agent_id: AgentId,
        name: String,
        mime_type: String,
        content: Vec<u8>,
    ) -> Result<Artifact, ArtifactError> {
        let now = Utc::now();
        let content_hash = Self::compute_hash(&content);
        let id = ArtifactId(uuid::Uuid::now_v7());
        let storage_path = format!("store://local/{}", id.0);

        let artifact = Artifact {
            id: id.clone(),
            task_id,
            agent_id,
            name,
            mime_type,
            size_bytes: content.len() as u64,
            storage_path,
            content_hash: content_hash.clone(),
            created_at: now,
        };

        self.store
            .set(&Self::meta_key(&id), &artifact)
            .await?;
        self.store
            .set_raw(&Self::content_key(&id), content)
            .await?;

        Ok(artifact)
    }

    pub async fn get_artifact(&self, id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError> {
        Ok(self.store.get::<Artifact>(&Self::meta_key(id)).await?)
    }

    pub async fn get_content(&self, id: &ArtifactId) -> Result<Option<Vec<u8>>, ArtifactError> {
        Ok(self.store.get_raw(&Self::content_key(id)).await?)
    }

    pub async fn verify_content(&self, id: &ArtifactId) -> Result<bool, ArtifactError> {
        let meta = self
            .get_artifact(id)
            .await?
            .ok_or_else(|| ArtifactError::NotFound(id.0.to_string()))?;
        let content = self
            .get_content(id)
            .await?
            .ok_or_else(|| ArtifactError::NotFound(id.0.to_string()))?;
        let computed = Self::compute_hash(&content);
        Ok(computed == meta.content_hash)
    }

    pub async fn delete_artifact(&self, id: &ArtifactId) -> Result<(), ArtifactError> {
        self.store.delete(&Self::meta_key(id)).await?;
        self.store.delete(&Self::content_key(id)).await?;
        Ok(())
    }

    pub async fn list_by_task(&self, task_id: &TaskId) -> Result<Vec<Artifact>, ArtifactError> {
        let all: Vec<(String, Artifact)> = self.store.scan_prefix(META_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, a)| a)
            .filter(|a| a.task_id.0 == task_id.0)
            .collect())
    }

    pub async fn list_by_agent(&self, agent_id: &AgentId) -> Result<Vec<Artifact>, ArtifactError> {
        let all: Vec<(String, Artifact)> = self.store.scan_prefix(META_PREFIX).await?;
        Ok(all
            .into_iter()
            .map(|(_, a)| a)
            .filter(|a| a.agent_id.0 == agent_id.0)
            .collect())
    }

    pub async fn count(&self) -> Result<usize, ArtifactError> {
        let keys = self.store.list_keys(META_PREFIX).await?;
        Ok(keys.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_svc() -> ArtifactService {
        let store = Arc::new(clawhive_store::InMemoryStore::new()) as Arc<dyn Store>;
        ArtifactService::new(store)
    }

    fn make_task_id() -> TaskId {
        TaskId(uuid::Uuid::now_v7())
    }

    fn make_agent_id() -> AgentId {
        AgentId(uuid::Uuid::now_v7())
    }

    #[tokio::test]
    async fn test_store_and_get_artifact() {
        let svc = make_svc();
        let task_id = make_task_id();
        let agent_id = make_agent_id();

        let artifact = svc
            .store_artifact(task_id.clone(), agent_id.clone(), "report.txt".into(), "text/plain".into(), b"hello world".to_vec())
            .await
            .unwrap();

        assert_eq!(artifact.name, "report.txt");
        assert_eq!(artifact.size_bytes, 11);

        let retrieved = svc.get_artifact(&artifact.id).await.unwrap().unwrap();
        assert_eq!(retrieved.name, "report.txt");
        assert_eq!(retrieved.content_hash, artifact.content_hash);
    }

    #[tokio::test]
    async fn test_get_content() {
        let svc = make_svc();
        let content = b"hello world".to_vec();
        let artifact = svc
            .store_artifact(make_task_id(), make_agent_id(), "f.txt".into(), "text/plain".into(), content.clone())
            .await
            .unwrap();

        let stored = svc.get_content(&artifact.id).await.unwrap().unwrap();
        assert_eq!(stored, content);
    }

    #[tokio::test]
    async fn test_verify_content_correct() {
        let svc = make_svc();
        let artifact = svc
            .store_artifact(make_task_id(), make_agent_id(), "f.txt".into(), "text/plain".into(), b"data".to_vec())
            .await
            .unwrap();

        assert!(svc.verify_content(&artifact.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_verify_content_fails_after_corruption() {
        let svc = make_svc();
        let artifact = svc
            .store_artifact(make_task_id(), make_agent_id(), "f.txt".into(), "text/plain".into(), b"original".to_vec())
            .await
            .unwrap();

        // Corrupt content directly in store
        svc.store
            .set_raw(&ArtifactService::content_key(&artifact.id), b"tampered".to_vec())
            .await
            .unwrap();

        assert!(!svc.verify_content(&artifact.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_artifact() {
        let svc = make_svc();
        let artifact = svc
            .store_artifact(make_task_id(), make_agent_id(), "f.txt".into(), "text/plain".into(), b"data".to_vec())
            .await
            .unwrap();

        svc.delete_artifact(&artifact.id).await.unwrap();
        assert!(svc.get_artifact(&artifact.id).await.unwrap().is_none());
        assert!(svc.get_content(&artifact.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_list_by_task() {
        let svc = make_svc();
        let task_a = make_task_id();
        let task_b = make_task_id();
        let agent = make_agent_id();

        svc.store_artifact(task_a.clone(), agent.clone(), "a1.txt".into(), "text/plain".into(), b"a1".to_vec())
            .await.unwrap();
        svc.store_artifact(task_a.clone(), agent.clone(), "a2.txt".into(), "text/plain".into(), b"a2".to_vec())
            .await.unwrap();
        svc.store_artifact(task_b.clone(), agent.clone(), "b1.txt".into(), "text/plain".into(), b"b1".to_vec())
            .await.unwrap();

        let by_a = svc.list_by_task(&task_a).await.unwrap();
        assert_eq!(by_a.len(), 2);

        let by_b = svc.list_by_task(&task_b).await.unwrap();
        assert_eq!(by_b.len(), 1);
    }

    #[tokio::test]
    async fn test_list_by_agent() {
        let svc = make_svc();
        let task = make_task_id();
        let agent_a = make_agent_id();
        let agent_b = make_agent_id();

        svc.store_artifact(task.clone(), agent_a.clone(), "a1.txt".into(), "text/plain".into(), b"a1".to_vec())
            .await.unwrap();
        svc.store_artifact(task.clone(), agent_a.clone(), "a2.txt".into(), "text/plain".into(), b"a2".to_vec())
            .await.unwrap();
        svc.store_artifact(task.clone(), agent_b.clone(), "b1.txt".into(), "text/plain".into(), b"b1".to_vec())
            .await.unwrap();

        let by_a = svc.list_by_agent(&agent_a).await.unwrap();
        assert_eq!(by_a.len(), 2);

        let by_b = svc.list_by_agent(&agent_b).await.unwrap();
        assert_eq!(by_b.len(), 1);
    }

    #[tokio::test]
    async fn test_count() {
        let svc = make_svc();
        let agent = make_agent_id();
        svc.store_artifact(make_task_id(), agent.clone(), "f1".into(), "text/plain".into(), b"1".to_vec())
            .await.unwrap();
        svc.store_artifact(make_task_id(), agent.clone(), "f2".into(), "text/plain".into(), b"2".to_vec())
            .await.unwrap();
        assert_eq!(svc.count().await.unwrap(), 2);
    }
}
