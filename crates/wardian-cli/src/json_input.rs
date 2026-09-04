//! Shell-independent JSON arguments for automation input and assignments.

use std::io::{IsTerminal, Read};

use crate::errors::{CliError, ExitCode};

/// Read an inline JSON object, `@path`, or piped standard input (`-`).
/// UTF-8 BOMs from Windows text writers are accepted at the document boundary.
pub fn parse<T: serde::de::DeserializeOwned>(raw: &str, flag: &str) -> Result<T, CliError> {
    let document = if raw == "-" {
        if std::io::stdin().is_terminal() {
            return Err(invalid(flag, "`-` requires piped standard input"));
        }
        let mut document = String::new();
        std::io::stdin()
            .read_to_string(&mut document)
            .map_err(|error| invalid(flag, &format!("could not read stdin: {error}")))?;
        document
    } else if let Some(path) = raw.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|error| invalid(flag, &format!("could not read JSON file: {error}")))?
    } else {
        raw.to_string()
    };
    let value: serde_json::Value = serde_json::from_str(document.trim_start_matches('\u{feff}'))
        .map_err(|error| invalid(flag, &error.to_string()))?;
    if !value.is_object() {
        return Err(invalid(flag, "expected a JSON object"));
    }
    serde_json::from_value(value).map_err(|error| invalid(flag, &error.to_string()))
}

fn invalid(flag: &str, reason: &str) -> CliError {
    let mut error = CliError::backend(
        ExitCode::Generic,
        "invalid_json",
        format!("invalid {flag} JSON: {reason}"),
    );
    error.hint = Some(format!(
        "Pass an inline JSON object, {flag} @<path>, or {flag} - with piped UTF-8 JSON."
    ));
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_preserves_quotes_unicode_newlines_and_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input with spaces.json");
        let expected = serde_json::json!({"prompt": "Quote: \"hello\"\nλ $() `literal`"});
        std::fs::write(&path, format!("\u{feff}{expected}")).unwrap();
        let actual: serde_json::Value = parse(&format!("@{}", path.display()), "--input").unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            parse::<serde_json::Value>(&expected.to_string(), "--input").unwrap(),
            expected
        );
    }

    #[test]
    fn rejects_non_objects_invalid_json_and_missing_files() {
        for raw in ["[]", "null", "42", "{unquoted:1}", "@"] {
            let error = parse::<serde_json::Value>(raw, "--input").unwrap_err();
            assert_eq!(error.code, "invalid_json", "{raw}");
            assert!(error.hint.unwrap().contains("--input @"));
        }
    }
}
