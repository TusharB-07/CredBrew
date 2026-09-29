use object_store::{ObjectStore, path::Path, PutPayload};
use object_store::aws::AmazonS3Builder;
use std::sync::Arc;
use crate::error::CoreError;

#[derive(Clone, Debug)]
pub struct Config {
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub public_base_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, CoreError> {
        Ok(Self {
            s3_endpoint: std::env::var("S3_ENDPOINT")?,
            s3_bucket: std::env::var("S3_BUCKET")?,
            s3_access_key: std::env::var("S3_ACCESS_KEY")?,
            s3_secret_key: std::env::var("S3_SECRET_KEY")?,
            public_base_url: std::env::var("PUBLIC_BASE_URL")?,
        })
    }
}

#[derive(Clone)]
pub struct StorageClient {
    inner: Arc<dyn ObjectStore>,
    public_base_url: String,
}

impl StorageClient {
    pub fn new(config: &Config) -> Result<Self, CoreError> {
        let s3 = AmazonS3Builder::new()
            .with_bucket_name(&config.s3_bucket)
            .with_endpoint(&config.s3_endpoint)
            .with_access_key_id(&config.s3_access_key)
            .with_secret_access_key(&config.s3_secret_key)
            .with_region("us-east-1")
            .with_allow_http(true)
            .build()?;

        Ok(Self {
            inner: Arc::new(s3),
            public_base_url: config.public_base_url.clone(),
        })
    }

    pub async fn put(&self, key: &str, data: Vec<u8>, _content_type: &str) -> Result<(), CoreError> {
        let path = Path::from(key);
        let payload = PutPayload::from(bytes::Bytes::from(data));
        self.inner.put(&path, payload).await?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Vec<u8>, CoreError> {
        let path = Path::from(key);
        let result = self.inner.get(&path).await?;
        let bytes = result.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub fn public_url(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url.trim_end_matches('/'), key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_storage_roundtrip() {
        let config = Config {
            s3_endpoint: "http://localhost:9000".into(),
            s3_bucket: "credbrew".into(),
            s3_access_key: "minioadmin".into(),
            s3_secret_key: "minioadmin".into(),
            public_base_url: "http://localhost:9000".into(),
        };
        let client = StorageClient::new(&config).unwrap();
        let key = "test/roundtrip.txt";
        let data = b"hello world";
        client.put(key, data.to_vec(), "text/plain").await.unwrap();
        let got = client.get(key).await.unwrap();
        assert_eq!(got, data);
    }
}