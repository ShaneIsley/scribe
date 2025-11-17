use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Compute a stable hash for a primary key
/// Uses a sorted concatenation of key-value pairs to ensure consistency
pub fn compute_primary_key_hash(
    primary_keys: &[String],
    item: &BTreeMap<String, serde_json::Value>,
) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();

    // Sort keys to ensure consistent ordering
    let mut sorted_keys: Vec<_> = primary_keys.iter().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if let Some(value) = item.get(key) {
            // Serialize the key-value pair
            hasher.update(key.as_bytes());
            hasher.update(b":");
            hasher.update(value.to_string().as_bytes());
            hasher.update(b";");
        } else {
            anyhow::bail!("Primary key column '{}' not found in item", key);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute a hash of the entire item content
/// This is used to detect if an item has changed between commits
pub fn compute_content_hash(item: &BTreeMap<String, serde_json::Value>) -> String {
    let mut hasher = Sha256::new();

    // Sort keys to ensure consistent ordering
    let mut sorted_keys: Vec<_> = item.keys().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if let Some(value) = item.get(key) {
            hasher.update(key.as_bytes());
            hasher.update(b":");
            hasher.update(value.to_string().as_bytes());
            hasher.update(b";");
        }
    }

    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_primary_key_hash_consistency() {
        let mut item1 = BTreeMap::new();
        item1.insert("id".to_string(), json!(123));
        item1.insert("name".to_string(), json!("test"));

        let mut item2 = BTreeMap::new();
        item2.insert("name".to_string(), json!("test"));
        item2.insert("id".to_string(), json!(123));

        let keys = vec!["id".to_string()];
        let hash1 = compute_primary_key_hash(&keys, &item1).unwrap();
        let hash2 = compute_primary_key_hash(&keys, &item2).unwrap();

        assert_eq!(hash1, hash2, "Hashes should be consistent regardless of insertion order");
    }

    #[test]
    fn test_content_hash_detects_changes() {
        let mut item1 = BTreeMap::new();
        item1.insert("id".to_string(), json!(123));
        item1.insert("value".to_string(), json!("a"));

        let mut item2 = BTreeMap::new();
        item2.insert("id".to_string(), json!(123));
        item2.insert("value".to_string(), json!("b"));

        let hash1 = compute_content_hash(&item1);
        let hash2 = compute_content_hash(&item2);

        assert_ne!(hash1, hash2, "Content hash should change when values change");
    }
}
