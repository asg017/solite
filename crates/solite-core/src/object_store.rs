//! Object store integration for exporting to S3-compatible storage.
//!
//! Supports `s3://` and `t3://` URL schemes. Defaults to the Tigris
//! endpoint (`https://t3.storage.dev`) unless `AWS_ENDPOINT_URL_S3` is set.

use crate::exporter::ExportError;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::ObjectStoreExt;

/// Check if a target string is an object store URL.
pub fn is_object_store_url(path: &str) -> bool {
    path.starts_with("s3://") || path.starts_with("t3://")
}

/// Parse an object store URL into (bucket, key).
///
/// Accepts `s3://bucket/path/to/file.csv` or `t3://bucket/path/to/file.csv`.
fn parse_url(url: &str) -> Result<(&str, &str), ExportError> {
    let without_scheme = url
        .strip_prefix("s3://")
        .or_else(|| url.strip_prefix("t3://"))
        .ok_or_else(|| ExportError::Io(std::io::Error::other("unsupported URL scheme")))?;

    let (bucket, key) = without_scheme
        .split_once('/')
        .ok_or_else(|| ExportError::Io(std::io::Error::other("URL must include a key path after the bucket name")))?;

    if bucket.is_empty() || key.is_empty() {
        return Err(ExportError::Io(std::io::Error::other(
            "bucket and key must not be empty",
        )));
    }

    Ok((bucket, key))
}

/// Upload bytes to an S3-compatible object store.
///
/// Defaults to the Tigris endpoint. Override with `AWS_ENDPOINT_URL_S3`.
/// Credentials are read from `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.
pub fn upload(url: &str, data: Vec<u8>) -> Result<(), ExportError> {
    let (bucket, key) = parse_url(url)?;

    let endpoint = std::env::var("AWS_ENDPOINT_URL_S3")
        .unwrap_or_else(|_| "https://t3.storage.dev".to_string());

    let store = AmazonS3Builder::new()
        .with_endpoint(&endpoint)
        .with_region(
            std::env::var("AWS_REGION")
                .unwrap_or_else(|_| "auto".to_string()),
        )
        .with_bucket_name(bucket)
        .with_access_key_id(
            std::env::var("AWS_ACCESS_KEY_ID")
                .map_err(|_| ExportError::Io(std::io::Error::other(
                    "AWS_ACCESS_KEY_ID environment variable is not set",
                )))?,
        )
        .with_secret_access_key(
            std::env::var("AWS_SECRET_ACCESS_KEY")
                .map_err(|_| ExportError::Io(std::io::Error::other(
                    "AWS_SECRET_ACCESS_KEY environment variable is not set",
                )))?,
        )
        .build()
        .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))?;

    let path = Path::from(key);

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| ExportError::Io(e))?;

    rt.block_on(async {
        store
            .put(&path, data.into())
            .await
            .map_err(|e| ExportError::Io(std::io::Error::other(e.to_string())))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_object_store_url() {
        assert!(is_object_store_url("s3://bucket/key.csv"));
        assert!(is_object_store_url("t3://bucket/key.csv"));
        assert!(!is_object_store_url("/tmp/local.csv"));
        assert!(!is_object_store_url("output.csv"));
    }

    #[test]
    fn test_parse_url_s3() {
        let (bucket, key) = parse_url("s3://my-bucket/path/to/file.csv").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "path/to/file.csv");
    }

    #[test]
    fn test_parse_url_t3() {
        let (bucket, key) = parse_url("t3://my-bucket/file.json").unwrap();
        assert_eq!(bucket, "my-bucket");
        assert_eq!(key, "file.json");
    }

    #[test]
    fn test_parse_url_no_key() {
        assert!(parse_url("s3://bucket-only").is_err());
    }

    #[test]
    fn test_parse_url_empty_parts() {
        assert!(parse_url("s3:///key").is_err());
        assert!(parse_url("s3://bucket/").is_err());
    }

    #[test]
    fn test_parse_url_bad_scheme() {
        assert!(parse_url("http://bucket/key").is_err());
    }
}
