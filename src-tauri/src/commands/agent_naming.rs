use std::collections::HashSet;

use crate::manager;

pub(super) const MAX_AGENT_NAME_CHARS: usize = 64;
pub(super) const GENERATED_AGENT_NAME_SUFFIXES: &[&str] = &[
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliett",
    "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra", "tango",
    "uniform", "victor", "whiskey", "xray", "yankee", "zulu",
];
pub(super) const MAX_GENERATED_AGENT_NAME_CYCLES: usize = 100;
const GENERATED_FALLBACK_UUID_CHARS: usize = 32;
const GENERATED_FALLBACK_PREFIX: &str = "-agent-";
const MAX_GENERATED_AGENT_BASE_CHARS: usize =
    MAX_AGENT_NAME_CHARS - GENERATED_FALLBACK_PREFIX.len() - GENERATED_FALLBACK_UUID_CHARS;

pub(super) fn validate_agent_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Agent name cannot be empty when specified.".to_string());
    }
    if name.chars().count() > MAX_AGENT_NAME_CHARS {
        return Err(format!(
            "Agent name must be {MAX_AGENT_NAME_CHARS} characters or fewer."
        ));
    }
    if name.chars().any(char::is_whitespace) {
        return Err(
            "Agent name may contain only letters, numbers, underscores, or hyphens; spaces are not allowed."
                .to_string(),
        );
    }
    let mut chars = name.chars();
    let first = chars.next().expect("empty name checked above");
    if !first.is_ascii_alphanumeric() {
        return Err("Agent name must start with a letter or number; leading hyphens are reserved by the CLI.".to_string());
    }
    if chars.any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')) {
        return Err(
            "Agent name may contain only letters, numbers, underscores, or hyphens; spaces are not allowed."
                .to_string(),
        );
    }
    if name.eq_ignore_ascii_case("all") {
        return Err("Agent name 'all' is reserved for broadcast commands.".to_string());
    }
    if uuid::Uuid::parse_str(name).is_ok() {
        return Err("UUID-shaped agent names are reserved for session identifiers.".to_string());
    }
    Ok(())
}

pub(super) fn generated_agent_name(agent_class: &str, existing_names: &HashSet<String>) -> String {
    generated_agent_name_from_base(&generated_agent_name_base(agent_class), existing_names)
}

fn generated_agent_name_base(agent_class: &str) -> String {
    let mut previous_was_separator = false;
    let mut base = String::new();

    for ch in agent_class.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            base.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator && !base.is_empty() {
            base.push('-');
            previous_was_separator = true;
        }
    }

    let base = base.trim_matches('-');
    if base.is_empty() {
        "Agent".to_string()
    } else {
        base.chars().take(MAX_GENERATED_AGENT_BASE_CHARS).collect()
    }
}

fn generated_agent_name_from_base(base: &str, existing_names: &HashSet<String>) -> String {
    for suffix in GENERATED_AGENT_NAME_SUFFIXES {
        let candidate = format!("{base}-{suffix}");
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }

    for cycle in 2..=MAX_GENERATED_AGENT_NAME_CYCLES {
        for suffix in GENERATED_AGENT_NAME_SUFFIXES {
            let candidate = format!("{base}-{suffix}-{cycle}");
            if !existing_names.contains(&candidate) {
                return candidate;
            }
        }
    }

    loop {
        let candidate = format!("{base}-agent-{}", uuid::Uuid::new_v4().simple());
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }
}

pub(super) fn persisted_agent_session_names() -> Result<HashSet<String>, String> {
    wardian_core::db::get_all_agents()
        .map(|agents| {
            agents
                .into_iter()
                .map(|agent| agent.session_name)
                .filter(|name| !name.trim().is_empty())
                .collect()
        })
        .map_err(|error| {
            manager::log_debug(&format!(
                "[WARDIAN] Could not read persisted agent names before spawn: {error}"
            ));
            format!("Could not read persisted agent names before spawn: {error}")
        })
}

pub(super) fn resolve_requested_spawn_session_name(
    requested_session_name: &str,
    agent_class: &str,
    existing_names: &HashSet<String>,
) -> Result<String, String> {
    if requested_session_name.trim().is_empty() {
        return Ok(generated_agent_name(agent_class, existing_names));
    }

    validate_agent_name(requested_session_name)?;

    if existing_names.contains(requested_session_name) {
        return Err(format!(
            "An agent with the name '{}' already exists.",
            requested_session_name
        ));
    }

    Ok(requested_session_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn generated_agent_name_uses_class_and_phonetic_suffixes() {
        assert_eq!(
            generated_agent_name("Coder", &HashSet::new()),
            "Coder-alpha"
        );
        assert_eq!(
            generated_agent_name("Coder", &name_set(&["Coder-alpha"])),
            "Coder-bravo"
        );
    }

    #[test]
    fn generated_agent_name_sanitizes_class_name() {
        assert_eq!(
            generated_agent_name("Data Analyst", &HashSet::new()),
            "Data-Analyst-alpha"
        );
    }

    #[test]
    fn generated_agent_name_falls_back_when_class_has_no_valid_name_chars() {
        assert_eq!(
            generated_agent_name(" !!! ", &HashSet::new()),
            "Agent-alpha"
        );
    }

    #[test]
    fn generated_agent_name_uses_a_bounded_non_ordinal_fallback() {
        let names = GENERATED_AGENT_NAME_SUFFIXES
            .iter()
            .map(|suffix| format!("Coder-{suffix}"))
            .chain((2..=MAX_GENERATED_AGENT_NAME_CYCLES).flat_map(|cycle| {
                GENERATED_AGENT_NAME_SUFFIXES
                    .iter()
                    .map(move |suffix| format!("Coder-{suffix}-{cycle}"))
            }))
            .collect::<HashSet<_>>();

        let generated = generated_agent_name("Coder", &names);
        assert!(generated.starts_with("Coder-agent-"));
        assert!(generated.chars().count() <= MAX_AGENT_NAME_CHARS);
        assert!(!names.contains(&generated));
    }

    #[test]
    fn explicit_spawn_name_with_spaces_still_fails_validation() {
        let err = resolve_requested_spawn_session_name(" Coder ", "Coder", &HashSet::new())
            .expect_err("explicit names with spaces must remain invalid");

        assert!(err.contains("spaces are not allowed"));
    }

    #[test]
    fn reserved_and_unaddressable_agent_names_explain_their_rule() {
        let cases = [
            ("-reviewer", "leading hyphens"),
            ("all", "reserved"),
            ("550e8400-e29b-41d4-a716-446655440000", "UUID-shaped"),
            ("Coder name", "spaces"),
        ];

        for (name, expected) in cases {
            let error = resolve_requested_spawn_session_name(name, "Coder", &HashSet::new())
                .expect_err("reserved name should be rejected");
            assert!(
                error
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase()),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn agent_name_length_is_bounded() {
        let name = "a".repeat(MAX_AGENT_NAME_CHARS + 1);
        let error = resolve_requested_spawn_session_name(&name, "Coder", &HashSet::new())
            .expect_err("overlong name should be rejected");
        assert!(error.contains("64 characters or fewer"));
    }
}
