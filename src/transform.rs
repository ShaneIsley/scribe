use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;

/// Apply a JMESPath transformation to parsed items
pub fn apply_transform(
    items: Vec<BTreeMap<String, Value>>,
    expression: &str,
) -> Result<Vec<BTreeMap<String, Value>>> {
    // Convert items to JSON Value for JMESPath processing
    let input_json: Value = items
        .iter()
        .map(|item| {
            let map: serde_json::Map<String, Value> = item
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Value::Object(map)
        })
        .collect::<Vec<_>>()
        .into();

    // Compile and execute JMESPath expression
    let compiled = jmespath::compile(expression)
        .context("Invalid JMESPath expression")?;

    let result = compiled.search(&input_json)
        .context("Failed to apply JMESPath transformation")?;

    // Convert JMESPath Variable back to serde_json::Value
    let result_json: Value = serde_json::from_str(&result.to_string())
        .context("Failed to convert JMESPath result to JSON")?;

    // Validate result is an array
    let result_array = match result_json {
        Value::Array(arr) => arr,
        _ => bail!("JMESPath expression must return an array, got: {}",
                   type_name(&result_json)),
    };

    // Convert array elements to BTreeMap
    let mut output_items = Vec::new();
    for (idx, item) in result_array.iter().enumerate() {
        match item {
            Value::Object(obj) => {
                let btree: BTreeMap<String, Value> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                output_items.push(btree);
            }
            _ => bail!(
                "JMESPath expression must return array of objects, but element {} is: {}",
                idx,
                type_name(item)
            ),
        }
    }

    Ok(output_items)
}

/// Helper to get type name for error messages
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_item(id: i64, name: &str, age: i64) -> BTreeMap<String, Value> {
        let mut item = BTreeMap::new();
        item.insert("id".to_string(), json!(id));
        item.insert("name".to_string(), json!(name));
        item.insert("age".to_string(), json!(age));
        item
    }

    fn make_nested_item(id: i64, author_name: &str, author_email: &str) -> BTreeMap<String, Value> {
        let mut item = BTreeMap::new();
        item.insert("id".to_string(), json!(id));
        item.insert(
            "author".to_string(),
            json!({
                "name": author_name,
                "email": author_email
            }),
        );
        item
    }

    #[test]
    fn test_identity_transform() {
        // Identity transformation should return items unchanged
        let items = vec![make_item(1, "Alice", 30), make_item(2, "Bob", 25)];

        let result = apply_transform(items.clone(), "[*]").unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].get("name").unwrap(), &json!("Alice"));
        assert_eq!(result[1].get("name").unwrap(), &json!("Bob"));
    }

    #[test]
    fn test_field_selection() {
        // Select only specific fields
        let items = vec![make_item(1, "Alice", 30), make_item(2, "Bob", 25)];

        let result = apply_transform(items, "[*].{id: id, name: name}").unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].get("id").unwrap(), &json!(1));
        assert_eq!(result[0].get("name").unwrap(), &json!("Alice"));
        assert_eq!(result[0].get("age"), None); // age should be excluded
    }

    #[test]
    fn test_flatten_nested_object() {
        // Flatten nested author object
        let items = vec![
            make_nested_item(1, "Alice", "alice@example.com"),
            make_nested_item(2, "Bob", "bob@example.com"),
        ];

        let result = apply_transform(
            items,
            "[*].{id: id, author_name: author.name, author_email: author.email}",
        )
        .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].get("id").unwrap(), &json!(1));
        assert_eq!(result[0].get("author_name").unwrap(), &json!("Alice"));
        assert_eq!(
            result[0].get("author_email").unwrap(),
            &json!("alice@example.com")
        );
        assert_eq!(result[0].get("author"), None); // original nested object should be gone
    }

    #[test]
    fn test_extract_array_element() {
        // Extract first element from array
        let mut item = BTreeMap::new();
        item.insert("id".to_string(), json!(1));
        item.insert("tags".to_string(), json!(["python", "rust", "go"]));

        let items = vec![item];

        let result = apply_transform(items, "[*].{id: id, primary_tag: tags[0]}").unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get("id").unwrap(), &json!(1));
        assert_eq!(result[0].get("primary_tag").unwrap(), &json!("python"));
    }

    #[test]
    fn test_invalid_expression() {
        // Invalid JMESPath syntax should return error
        let items = vec![make_item(1, "Alice", 30)];

        let result = apply_transform(items, "[invalid syntax}}");

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid JMESPath expression"));
    }

    #[test]
    fn test_transform_returns_non_array() {
        // Transformation that doesn't return an array should fail
        let items = vec![make_item(1, "Alice", 30)];

        let result = apply_transform(items, "[0]"); // Returns single object, not array

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must return an array"));
    }

    #[test]
    fn test_transform_returns_non_objects() {
        // Array of non-objects should fail
        let items = vec![make_item(1, "Alice", 30)];

        let result = apply_transform(items, "[*].id"); // Returns array of numbers

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must return array of objects"));
    }

    #[test]
    fn test_empty_array() {
        // Empty input should return empty output
        let items: Vec<BTreeMap<String, Value>> = vec![];

        let result = apply_transform(items, "[*].{id: id}").unwrap();

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_rename_fields() {
        // Rename fields during transformation
        let items = vec![make_item(1, "Alice", 30)];

        let result = apply_transform(items, "[*].{user_id: id, full_name: name, years: age}")
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].get("user_id").unwrap(), &json!(1));
        assert_eq!(result[0].get("full_name").unwrap(), &json!("Alice"));
        assert_eq!(result[0].get("years").unwrap(), &json!(30));
        assert_eq!(result[0].get("id"), None);
        assert_eq!(result[0].get("name"), None);
    }
}
