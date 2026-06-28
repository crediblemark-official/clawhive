use std::sync::Arc;

use async_trait::async_trait;

use crate::{Store, StoreError};

/// `NamespacedStore` membungkus store apapun dan menambahkan prefix namespace ke setiap key.
/// Consumer store tidak perlu sadar tentang namespace — semua operasi key transparan.
/// 
/// Contoh: namespace `"ws:a3f9b2c1:"` + key `"agent:xxx"` → disimpan sebagai `"ws:a3f9b2c1:agent:xxx"`.
/// Saat scan/read, namespace di-strip dari key yang dikembalikan ke consumer.
#[derive(Clone)]
pub struct NamespacedStore {
    inner: Arc<dyn Store>,
    namespace: String,
}

impl NamespacedStore {
    pub fn new(inner: Arc<dyn Store>, namespace: impl Into<String>) -> Self {
        Self {
            inner,
            namespace: namespace.into(),
        }
    }

    fn ns_key(&self, key: &str) -> String {
        format!("{}{}", self.namespace, key)
    }

    fn strip_ns<'a>(&self, key: &'a str) -> &'a str {
        key.strip_prefix(&self.namespace).unwrap_or(key)
    }
}

#[async_trait]
impl Store for NamespacedStore {
    async fn get_raw(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get_raw(&self.ns_key(key)).await
    }

    async fn set_raw(&self, key: &str, value: Vec<u8>) -> Result<(), StoreError> {
        self.inner.set_raw(&self.ns_key(key), value).await
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.inner.delete(&self.ns_key(key)).await
    }

    async fn exists(&self, key: &str) -> Result<bool, StoreError> {
        self.inner.exists(&self.ns_key(key)).await
    }

    async fn scan_prefix_raw(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, StoreError> {
        let ns_prefix = self.ns_key(prefix);
        let raw = self.inner.scan_prefix_raw(&ns_prefix).await?;
        Ok(raw
            .into_iter()
            .map(|(key, value)| (self.strip_ns(&key).to_string(), value))
            .collect())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let ns_prefix = self.ns_key(prefix);
        let keys = self.inner.list_keys(&ns_prefix).await?;
        Ok(keys
            .into_iter()
            .map(|k| self.strip_ns(&k).to_string())
            .collect())
    }

    /// Hanya menghapus key yang berawalan namespace ini — tidak menghapus seluruh database.
    async fn clear(&self) -> Result<(), StoreError> {
        let keys = self.inner.list_keys(&self.namespace).await?;
        for key in keys {
            self.inner.delete(&key).await?;
        }
        Ok(())
    }
}
