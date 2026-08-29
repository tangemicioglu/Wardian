use crate::automation::field_type::FieldType;
use crate::automation::registry::{node_types, NodeKind};
use std::fmt::Write as _;

/// Render the node-type reference as markdown, generated from the registry so
/// it can never drift from the contract agents author against.
pub fn reference_doc() -> String {
    let mut out = String::new();
    out.push_str("# Automation Node Reference\n\n");
    out.push_str("> Generated from the node type registry. Do not edit by hand.\n\n");

    for def in node_types() {
        let _ = writeln!(out, "## {}", def.label);
        let _ = writeln!(out);
        let _ = writeln!(out, "- **id:** `{}`", def.id);
        let _ = writeln!(out, "- **kind:** {}", kind_label(def.kind));
        let _ = writeln!(out, "- **category:** {}", def.category);
        let _ = writeln!(out, "- **version:** {}", def.version);
        if !def.supported {
            let _ = writeln!(out, "- **status:** unsupported");
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", def.description);
        let _ = writeln!(out);

        if def.fields.is_empty() {
            let _ = writeln!(out, "_No fields._\n");
        } else {
            let _ = writeln!(out, "### Fields\n");
            for field in &def.fields {
                let req = if field.required { " (required)" } else { "" };
                let mult = if field.multiple { ", multiple" } else { "" };
                let _ = writeln!(
                    out,
                    "- `{}` — {} [{}{}{}]",
                    field.id,
                    field.label,
                    type_label(&field.field_type),
                    req,
                    mult
                );
                if !field.help.is_empty() {
                    let _ = writeln!(out, "  Help: {}", escape_template_delimiters(&field.help));
                }
            }
            let _ = writeln!(out);
        }

        let ports: Vec<&str> = if let Some(f) = &def.outputs_from_field {
            let _ = writeln!(out, "Outgoing ports are derived from the `{f}` field.\n");
            vec![]
        } else {
            def.outputs.iter().map(|p| p.id.as_str()).collect()
        };
        if !ports.is_empty() {
            let _ = writeln!(out, "Outgoing ports: {}\n", ports.join(", "));
        }
    }
    format!("{}\n", out.trim_end())
}

fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Agent => "agent",
        NodeKind::Engine => "engine",
        NodeKind::Trigger => "trigger",
    }
}

fn escape_template_delimiters(text: &str) -> String {
    text.replace("{{", "&#123;&#123;")
        .replace("}}", "&#125;&#125;")
}

fn type_label(ty: &FieldType) -> String {
    match ty {
        FieldType::Code { language } => format!("code:{language}"),
        FieldType::Enum { options } => format!("enum:{}", options.join("|")),
        other => format!("{other:?}").to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_has_a_section_per_node_type() {
        let md = reference_doc();
        assert!(md.starts_with("# Automation Node Reference"));
        for def in crate::automation::registry::node_types() {
            assert!(
                md.contains(&format!("## {}", def.label)),
                "missing {}",
                def.label
            );
        }
    }

    #[test]
    fn doc_lists_fields_with_required_marker() {
        let md = reference_doc();
        // `task.prompt` is required.
        assert!(md.contains("`prompt`"));
        assert!(md.contains("required"));
    }

    #[test]
    fn doc_marks_reserved_node_types_as_unsupported() {
        let md = reference_doc();
        let start = md
            .find("## Sub-automation")
            .expect("sub-automation section");
        let section = &md[start..];
        assert!(section.contains("**status:** unsupported"));
    }

    #[test]
    fn doc_escapes_template_delimiters_for_vitepress() {
        let md = reference_doc();
        assert!(md.contains("&#123;&#123;trigger.output.agent_id&#125;&#125;"));
        assert!(!md.contains("only {{trigger.output.agent_id}}"));
    }

    #[test]
    fn doc_ends_with_exactly_one_newline() {
        let md = reference_doc();
        assert!(md.ends_with('\n'));
        assert!(!md.ends_with("\n\n"));
    }
}
