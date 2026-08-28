use serde_json::Value;

/// The v1 condition language is a dot-separated path into the run registry.
pub const CONDITION_HELP: &str =
    "A dot-separated registry path such as nodes.agent-1.output.ready. Operators and comparisons are not supported.";

/// Validate and normalize a v1 registry-path condition.
pub fn validate_path(condition: &str) -> Result<String, &'static str> {
    let path = condition.trim();
    if path.is_empty() {
        return Err("expected a non-empty dot-separated registry path");
    }

    for segment in path.split('.') {
        let mut chars = segment.chars();
        let Some(first) = chars.next() else {
            return Err("expected a dot-separated registry path");
        };
        if !(first.is_ascii_alphabetic() || first == '_')
            || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(
                "expected a dot-separated registry path; operators and comparisons are not supported",
            );
        }
    }

    Ok(path.to_string())
}

/// Resolve a validated registry path and apply workflow truthiness rules.
pub(crate) fn lookup_truthy(registry: &Value, path: &str) -> bool {
    let mut current = registry;
    for segment in path.split('.') {
        match current.get(segment) {
            Some(value) => current = value,
            None => return false,
        }
    }
    match current {
        Value::Bool(value) => *value,
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Number(value) => value.as_f64().map(|number| number != 0.0).unwrap_or(false),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_registry_paths_with_hyphenated_ids() {
        assert_eq!(
            validate_path("nodes.agent-1.output.ready").unwrap(),
            "nodes.agent-1.output.ready"
        );
    }

    #[test]
    fn rejects_expression_operators() {
        let error =
            validate_path("nodes.agent-1.output.decision === 'HEARTBEAT_ACTION'").unwrap_err();
        assert!(error.contains("operators and comparisons are not supported"));
    }

    #[test]
    fn trims_outer_whitespace() {
        assert_eq!(
            validate_path("  trigger.output.ready  ").unwrap(),
            "trigger.output.ready"
        );
    }

    #[test]
    fn evaluates_registry_truthiness() {
        let registry = serde_json::json!({
            "nodes": {"agent-1": {"output": {"ready": true}}}
        });
        assert!(lookup_truthy(&registry, "nodes.agent-1.output.ready"));
        assert!(!lookup_truthy(&registry, "nodes.agent-1.output.missing"));
    }
}
