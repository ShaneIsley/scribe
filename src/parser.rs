use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;

/// Represents the file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Json,
    Csv,
}

impl FileFormat {
    /// Detect the file format from the file path extension
    pub fn from_path(path: &str) -> Result<Self> {
        if path.ends_with(".json") {
            Ok(Self::Json)
        } else if path.ends_with(".csv") {
            Ok(Self::Csv)
        } else {
            anyhow::bail!(
                "Unsupported file format. Only .json and .csv files are supported. Got: {}",
                path
            )
        }
    }
}

/// Parse file contents into a vector of items
pub fn parse_file(
    content: &[u8],
    format: FileFormat,
) -> Result<Vec<BTreeMap<String, Value>>> {
    parse_file_with_transform(content, format, None)
}

/// Parse file contents and optionally apply JMESPath transformation
pub fn parse_file_with_transform(
    content: &[u8],
    format: FileFormat,
    transform: Option<&str>,
) -> Result<Vec<BTreeMap<String, Value>>> {
    let items = match format {
        FileFormat::Json => parse_json(content)?,
        FileFormat::Csv => parse_csv(content)?,
    };

    // Apply transformation if provided
    if let Some(expr) = transform {
        crate::transform::apply_transform(items, expr)
    } else {
        Ok(items)
    }
}

/// Parse JSON content (expects an array of flat objects)
fn parse_json(content: &[u8]) -> Result<Vec<BTreeMap<String, Value>>> {
    let text = std::str::from_utf8(content)
        .context("File content is not valid UTF-8")?;

    let value: Value = serde_json::from_str(text)
        .context("Failed to parse JSON")?;

    match value {
        Value::Array(arr) => {
            let mut items = Vec::new();
            for (idx, item) in arr.into_iter().enumerate() {
                match item {
                    Value::Object(obj) => {
                        // Convert serde_json::Map to BTreeMap for consistent ordering
                        let btree: BTreeMap<String, Value> = obj.into_iter().collect();
                        items.push(btree);
                    }
                    _ => {
                        anyhow::bail!(
                            "JSON array item at index {} is not an object. Expected array of objects.",
                            idx
                        );
                    }
                }
            }
            Ok(items)
        }
        _ => {
            anyhow::bail!(
                "JSON root element must be an array of objects. Got: {}",
                match value {
                    Value::Object(_) => "object",
                    Value::String(_) => "string",
                    Value::Number(_) => "number",
                    Value::Bool(_) => "boolean",
                    Value::Null => "null",
                    _ => "unknown",
                }
            );
        }
    }
}

/// Parse CSV content with a header row
fn parse_csv(content: &[u8]) -> Result<Vec<BTreeMap<String, Value>>> {
    let mut reader = csv::Reader::from_reader(content);

    let headers = reader
        .headers()
        .context("Failed to read CSV headers")?
        .clone();

    let mut items = Vec::new();

    for (idx, result) in reader.records().enumerate() {
        let record = result
            .with_context(|| format!("Failed to read CSV record at row {}", idx + 2))?;

        let mut item = BTreeMap::new();

        for (col_idx, header) in headers.iter().enumerate() {
            let value = record
                .get(col_idx)
                .context("CSV record has fewer columns than headers")?;

            // Try to parse as number or boolean, otherwise keep as string
            let json_value = if let Ok(num) = value.parse::<i64>() {
                Value::Number(num.into())
            } else if let Ok(num) = value.parse::<f64>() {
                Value::Number(
                    serde_json::Number::from_f64(num)
                        .unwrap_or_else(|| serde_json::Number::from(0))
                )
            } else if let Ok(b) = value.parse::<bool>() {
                Value::Bool(b)
            } else if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.to_string())
            };

            item.insert(header.to_string(), json_value);
        }

        items.push(item);
    }

    Ok(items)
}

/// Extract all unique column names from a collection of items
pub fn extract_columns(items: &[BTreeMap<String, Value>]) -> Vec<String> {
    let mut columns = std::collections::HashSet::new();

    for item in items {
        for key in item.keys() {
            columns.insert(key.clone());
        }
    }

    let mut columns: Vec<String> = columns.into_iter().collect();
    columns.sort();
    columns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_array() {
        let json = r#"[
            {"id": 1, "name": "Alice"},
            {"id": 2, "name": "Bob"}
        ]"#;

        let items = parse_json(json.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name"), Some(&Value::String("Alice".to_string())));
    }

    #[test]
    fn test_parse_csv() {
        let csv = "id,name,age\n1,Alice,30\n2,Bob,25";

        let items = parse_csv(csv.as_bytes()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name"), Some(&Value::String("Alice".to_string())));
        assert_eq!(items[0].get("age"), Some(&Value::Number(30.into())));
    }

    #[test]
    fn test_extract_columns() {
        let mut item1 = BTreeMap::new();
        item1.insert("id".to_string(), Value::Number(1.into()));
        item1.insert("name".to_string(), Value::String("Alice".to_string()));

        let mut item2 = BTreeMap::new();
        item2.insert("id".to_string(), Value::Number(2.into()));
        item2.insert("age".to_string(), Value::Number(30.into()));

        let items = vec![item1, item2];
        let columns = extract_columns(&items);

        assert_eq!(columns.len(), 3);
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"name".to_string()));
        assert!(columns.contains(&"age".to_string()));
    }
}
