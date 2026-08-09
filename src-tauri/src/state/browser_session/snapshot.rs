//! Page snapshots and the element references actions target.
//!
//! Agents act on refs (`e1`, `e2`, ...) taken from a snapshot rather than on
//! raw selectors. A ref is bound to the snapshot generation that produced it,
//! and acting on a stale generation is refused instead of guessed at, so a
//! navigation between snapshot and click can never turn into a misclick.

use std::collections::HashMap;

use serde_json::Value;
pub use wardian_core::browser::{
    render_snapshot, PageSnapshot, SnapshotElement, MAX_SNAPSHOT_ELEMENTS,
    MAX_SNAPSHOT_FIELD_CHARS,
};

/// Attribute the injected walker stamps on every referenced element.
pub const REF_ATTRIBUTE: &str = "data-wardian-ref";
/// Attribute recording which snapshot generation stamped a ref.
pub const GENERATION_ATTRIBUTE: &str = "data-wardian-snapshot";

/// Why a ref could not be acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    /// The page changed generation since the snapshot that minted this ref.
    Stale {
        element_ref: String,
        snapshot_generation: u64,
        current_generation: u64,
    },
    /// No snapshot has been taken on the current generation.
    NoSnapshot,
    /// The ref is not a `e<number>` token.
    Malformed { element_ref: String },
    /// The ref was minted by the current snapshot but is no longer in the DOM.
    Detached { element_ref: String },
    /// The element still exists but is no longer what the snapshot described.
    Changed { element_ref: String },
    /// The ref matched several elements, so acting on it would be a guess.
    Ambiguous { element_ref: String },
}

impl std::fmt::Display for RefError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefError::Stale {
                element_ref,
                snapshot_generation,
                current_generation,
            } => write!(
                formatter,
                "{element_ref} came from snapshot generation {snapshot_generation}, but the page is now at generation {current_generation}. Re-run snapshot and use the new refs."
            ),
            RefError::NoSnapshot => write!(
                formatter,
                "no snapshot has been taken for the current page. Run snapshot first."
            ),
            RefError::Malformed { element_ref } => write!(
                formatter,
                "{element_ref} is not a valid element ref; refs look like e1, e2, e3"
            ),
            RefError::Detached { element_ref } => write!(
                formatter,
                "{element_ref} is no longer present in the page. Re-run snapshot and use the new refs."
            ),
            RefError::Changed { element_ref } => write!(
                formatter,
                "{element_ref} now points at different content than the snapshot described. Re-run snapshot and use the new refs."
            ),
            RefError::Ambiguous { element_ref } => write!(
                formatter,
                "{element_ref} matches more than one element. Re-run snapshot and use the new refs."
            ),
        }
    }
}

impl std::error::Error for RefError {}

impl RefError {
    /// Stable machine-readable code, so agents can branch without parsing prose.
    pub fn code(&self) -> &'static str {
        match self {
            RefError::Stale { .. } => "snapshot_stale",
            RefError::NoSnapshot => "snapshot_missing",
            RefError::Malformed { .. } => "ref_malformed",
            RefError::Detached { .. } => "ref_detached",
            RefError::Changed { .. } => "ref_changed",
            RefError::Ambiguous { .. } => "ref_ambiguous",
        }
    }
}

/// What a ref pointed at when the snapshot minted it.
///
/// Re-checked at action time so a recycled DOM node — the same element reused
/// for different content, as virtualized lists do — cannot be acted on as if
/// it were still the element the agent saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefIdentity {
    pub role: String,
    pub name: String,
}

/// Records which generation the live snapshot belongs to.
#[derive(Debug, Default, Clone)]
pub struct SnapshotLedger {
    /// Bumped by navigation; refs minted before the bump stop being valid.
    current_generation: u64,
    /// Generation of the most recent snapshot, if any.
    snapshot_generation: Option<u64>,
    /// What each minted ref pointed at, by ref token.
    minted: HashMap<String, RefIdentity>,
}

impl SnapshotLedger {
    pub fn current_generation(&self) -> u64 {
        self.current_generation
    }

    /// Invalidates outstanding refs. Called on every committed navigation.
    ///
    /// The previous snapshot's generation is deliberately retained so a ref
    /// used after a navigation reports the actionable "stale, re-snapshot"
    /// rather than the misleading "no snapshot has been taken".
    pub fn invalidate(&mut self) -> u64 {
        self.current_generation += 1;
        self.current_generation
    }

    /// Records a snapshot taken against the current generation.
    pub fn record_snapshot(&mut self, elements: &[SnapshotElement]) -> u64 {
        self.snapshot_generation = Some(self.current_generation);
        self.minted = elements
            .iter()
            .map(|element| {
                (
                    element.element_ref.clone(),
                    RefIdentity {
                        role: element.role.clone(),
                        name: element.name.clone(),
                    },
                )
            })
            .collect();
        self.current_generation
    }

    /// Checks a ref against the ledger before it is used in an action.
    pub fn validate(&self, element_ref: &str) -> Result<&RefIdentity, RefError> {
        if parse_ref(element_ref).is_none() {
            return Err(RefError::Malformed {
                element_ref: element_ref.to_string(),
            });
        }
        let Some(snapshot_generation) = self.snapshot_generation else {
            return Err(RefError::NoSnapshot);
        };
        if snapshot_generation != self.current_generation {
            return Err(RefError::Stale {
                element_ref: element_ref.to_string(),
                snapshot_generation,
                current_generation: self.current_generation,
            });
        }
        self.minted
            .get(element_ref)
            .ok_or_else(|| RefError::Detached {
                element_ref: element_ref.to_string(),
            })
    }
}

/// Parses `e12` into `12`. Returns `None` for anything else.
pub fn parse_ref(element_ref: &str) -> Option<usize> {
    let digits = element_ref.strip_prefix('e')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Clamps a field to [`MAX_SNAPSHOT_FIELD_CHARS`] on a character boundary.
pub fn clamp_field(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_SNAPSHOT_FIELD_CHARS {
        return normalized;
    }
    let mut clamped: String = normalized.chars().take(MAX_SNAPSHOT_FIELD_CHARS - 1).collect();
    clamped.push('…');
    clamped
}

/// Role and accessible-name derivation, shared by the walker and the guard.
///
/// Both must agree exactly: the guard compares what an element looks like now
/// against what the snapshot recorded, so any drift between the two
/// definitions would produce spurious refusals.
const IDENTITY_JS_TEMPLATE: &str = r#"  const MAX_FIELD = __MAX_FIELD__;
  // Mirrors `clamp_field` exactly. `Array.from` counts code points, matching
  // Rust's `chars()`; plain indexing would diverge on astral characters.
  const clampField = (value) => {
    const normalized = String(value || '').split(/\s+/).join(' ').trim();
    const points = Array.from(normalized);
    if (points.length <= MAX_FIELD) return normalized;
    return points.slice(0, MAX_FIELD - 1).join('') + '…';
  };
  const accessibleName = (element) => {
    const aria = element.getAttribute('aria-label');
    if (aria) return aria;
    const labelledBy = element.getAttribute('aria-labelledby');
    if (labelledBy) {
      const labelled = labelledBy
        .split(/\s+/)
        .map((id) => document.getElementById(id))
        .filter(Boolean)
        .map((node) => node.textContent || '')
        .join(' ');
      if (labelled.trim()) return labelled;
    }
    if (element.labels && element.labels.length > 0) {
      const labelText = Array.from(element.labels)
        .map((label) => label.textContent || '')
        .join(' ');
      if (labelText.trim()) return labelText;
    }
    const attributes = ['placeholder', 'alt', 'title', 'name'];
    for (const attribute of attributes) {
      const found = element.getAttribute(attribute);
      if (found) return found;
    }
    return element.textContent || '';
  };
  const roleOf = (element) => {
    const explicit = element.getAttribute('role');
    if (explicit) return explicit;
    const tag = element.tagName.toLowerCase();
    if (tag === 'a' && element.hasAttribute('href')) return 'link';
    if (tag === 'input') {
      const type = (element.getAttribute('type') || 'text').toLowerCase();
      if (type === 'checkbox' || type === 'radio' || type === 'button' || type === 'submit') {
        return type === 'submit' ? 'button' : type;
      }
      return 'textbox';
    }
    if (tag === 'textarea') return 'textbox';
    if (tag === 'select') return 'combobox';
    if (tag === 'button') return 'button';
    if (/^h[1-6]$/.test(tag)) return 'heading';
    return tag;
  };
"#;

/// The shared identity helpers with the field cap substituted in.
fn identity_js() -> String {
    IDENTITY_JS_TEMPLATE.replace("__MAX_FIELD__", &MAX_SNAPSHOT_FIELD_CHARS.to_string())
}

/// Builds the expression injected to snapshot the page.
///
/// Every referenced element is stamped with [`REF_ATTRIBUTE`] so a later action
/// can find it again by ref rather than by re-deriving a selector.
pub fn snapshot_expression(generation: u64, interactive_only: bool) -> String {
    format!(
        r#"(() => {{
  const REF = {ref_attribute:?};
  const GEN = {generation_attribute:?};
  const MAX = {max_elements};
  const INTERACTIVE_ONLY = {interactive_only};
  const generation = {generation};
  for (const stamped of document.querySelectorAll('[' + REF + ']')) {{
    stamped.removeAttribute(REF);
    stamped.removeAttribute(GEN);
  }}
  const interactiveTags = new Set([
    'a', 'button', 'input', 'select', 'textarea', 'summary', 'option', 'label'
  ]);
  const interactiveRoles = new Set([
    'button', 'link', 'checkbox', 'radio', 'textbox', 'combobox', 'listbox',
    'menuitem', 'option', 'searchbox', 'slider', 'switch', 'tab'
  ]);
  const isVisible = (element) => {{
    const style = window.getComputedStyle(element);
    if (style.visibility === 'hidden' || style.display === 'none' || style.opacity === '0') {{
      return false;
    }}
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }};
{identity_js}
  const isInteractive = (element, role) =>
    interactiveTags.has(element.tagName.toLowerCase())
    || interactiveRoles.has(role)
    || element.hasAttribute('onclick')
    || element.tabIndex >= 0;
  const elements = [];
  let truncated = false;
  let index = 0;
  for (const element of document.querySelectorAll('*')) {{
    if (!isVisible(element)) continue;
    const role = roleOf(element);
    if (INTERACTIVE_ONLY && !isInteractive(element, role)) continue;
    if (!INTERACTIVE_ONLY) {{
      const ownText = Array.from(element.childNodes)
        .filter((node) => node.nodeType === Node.TEXT_NODE)
        .map((node) => node.textContent || '')
        .join('')
        .trim();
      if (!ownText && !isInteractive(element, role)) continue;
    }}
    if (elements.length >= MAX) {{
      truncated = true;
      break;
    }}
    index += 1;
    const elementRef = 'e' + index;
    element.setAttribute(REF, elementRef);
    element.setAttribute(GEN, String(generation));
    const record = {{
      element_ref: elementRef,
      role,
      name: accessibleName(element),
      value: typeof element.value === 'string' ? element.value : '',
      enabled: !element.disabled,
    }};
    // Only checkables report checked. Every text input also carries a
    // `checked` property, and reporting it would put a meaningless
    // `checked=false` on the most common element in any snapshot.
    const inputType = (element.getAttribute('type') || '').toLowerCase();
    if (role === 'checkbox' || role === 'radio' || role === 'switch'
      || inputType === 'checkbox' || inputType === 'radio') {{
      record.checked = element.checked === true;
    }}
    elements.push(record);
  }}
  return {{
    url: window.location.href,
    title: document.title,
    elements,
    truncated,
  }};
}})()"#,
        ref_attribute = REF_ATTRIBUTE,
        generation_attribute = GENERATION_ATTRIBUTE,
        max_elements = MAX_SNAPSHOT_ELEMENTS,
        interactive_only = interactive_only,
        generation = generation,
        identity_js = identity_js(),
    )
}

/// Builds the expression that performs one action against a ref.
///
/// The guard re-derives the element's role and accessible name and compares
/// them to what the snapshot recorded. A ref alone is not enough: a page can
/// recycle a DOM node for different content without navigating, and the
/// stamped attribute would travel with it.
pub fn action_expression(
    element_ref: &str,
    generation: u64,
    expected: &RefIdentity,
    body: &str,
) -> String {
    let selector = serde_json::json!(format!(
        "[{REF_ATTRIBUTE}={element_ref:?}][{GENERATION_ATTRIBUTE}={:?}]",
        generation.to_string()
    ));
    format!(
        r#"(() => {{
{identity_js}
  const matches = document.querySelectorAll({selector});
  if (matches.length === 0) return 'detached';
  if (matches.length > 1) return 'ambiguous';
  const node = matches[0];
  if (clampField(roleOf(node)) !== {expected_role} || clampField(accessibleName(node)) !== {expected_name}) {{
    return 'changed';
  }}
  {body}
  return 'ok';
}})()"#,
        identity_js = identity_js(),
        selector = selector,
        expected_role = serde_json::json!(expected.role),
        expected_name = serde_json::json!(expected.name),
        body = body,
    )
}

/// Converts the walker's JSON into a snapshot with every field clamped.
pub fn parse_snapshot(
    generation: u64,
    interactive_only: bool,
    value: &Value,
) -> Result<PageSnapshot, String> {
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let raw_elements = value
        .get("elements")
        .and_then(Value::as_array)
        .ok_or_else(|| "snapshot result is missing its elements array".to_string())?;
    let mut elements = Vec::with_capacity(raw_elements.len().min(MAX_SNAPSHOT_ELEMENTS));
    for raw in raw_elements.iter().take(MAX_SNAPSHOT_ELEMENTS) {
        let element_ref = raw
            .get("element_ref")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if parse_ref(&element_ref).is_none() {
            continue;
        }
        elements.push(SnapshotElement {
            element_ref,
            role: clamp_field(raw.get("role").and_then(Value::as_str).unwrap_or("")),
            name: clamp_field(raw.get("name").and_then(Value::as_str).unwrap_or("")),
            value: clamp_field(raw.get("value").and_then(Value::as_str).unwrap_or("")),
            enabled: raw.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            checked: raw.get("checked").and_then(Value::as_bool),
        });
    }
    Ok(PageSnapshot {
        generation,
        url,
        title,
        interactive_only,
        truncated: value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || raw_elements.len() > MAX_SNAPSHOT_ELEMENTS,
        elements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_well_formed_refs_only() {
        assert_eq!(parse_ref("e1"), Some(1));
        assert_eq!(parse_ref("e42"), Some(42));
        assert_eq!(parse_ref("e"), None);
        assert_eq!(parse_ref("x1"), None);
        assert_eq!(parse_ref("e1x"), None);
        assert_eq!(parse_ref("e-1"), None);
        assert_eq!(parse_ref(""), None);
    }

    /// Builds `n` refs named `e1..eN` with distinct identities.
    fn minted(count: usize) -> Vec<SnapshotElement> {
        (1..=count)
            .map(|index| SnapshotElement {
                element_ref: format!("e{index}"),
                role: "button".to_string(),
                name: format!("Button {index}"),
                value: String::new(),
                enabled: true,
                checked: None,
            })
            .collect()
    }

    #[test]
    fn a_ref_taken_before_a_navigation_is_refused_as_stale() {
        let mut ledger = SnapshotLedger::default();
        ledger.record_snapshot(&minted(5));
        assert_eq!(ledger.validate("e3").expect("valid").name, "Button 3");
        ledger.invalidate();
        let error = ledger.validate("e3").expect_err("stale");
        assert_eq!(error.code(), "snapshot_stale");
        assert!(error.to_string().contains("Re-run snapshot"));
    }

    #[test]
    fn acting_before_any_snapshot_is_refused() {
        let ledger = SnapshotLedger::default();
        assert_eq!(ledger.validate("e1").expect_err("no snapshot").code(), "snapshot_missing");
    }

    #[test]
    fn a_ref_beyond_what_the_snapshot_minted_is_refused() {
        let mut ledger = SnapshotLedger::default();
        ledger.record_snapshot(&minted(2));
        assert_eq!(ledger.validate("e3").expect_err("detached").code(), "ref_detached");
        assert_eq!(ledger.validate("e0").expect_err("detached").code(), "ref_detached");
    }

    #[test]
    fn a_malformed_ref_is_refused_before_anything_else() {
        let ledger = SnapshotLedger::default();
        // No snapshot exists either, but shape is checked first so the agent
        // gets the actionable error rather than a misleading one.
        assert_eq!(ledger.validate("button").expect_err("malformed").code(), "ref_malformed");
    }

    #[test]
    fn re_snapshotting_after_a_navigation_restores_validity() {
        let mut ledger = SnapshotLedger::default();
        ledger.record_snapshot(&minted(3));
        ledger.invalidate();
        ledger.record_snapshot(&minted(4));
        assert_eq!(ledger.validate("e4").expect("valid").name, "Button 4");
    }

    #[test]
    fn a_snapshot_replaces_rather_than_merges_the_previous_refs() {
        let mut ledger = SnapshotLedger::default();
        ledger.record_snapshot(&minted(4));
        ledger.record_snapshot(&minted(2));
        // e3 existed in the earlier snapshot; carrying it forward would let an
        // agent act on a ref the current page never offered.
        assert_eq!(ledger.validate("e3").expect_err("gone").code(), "ref_detached");
    }

    #[test]
    fn the_action_guard_pins_the_generation_and_the_expected_identity() {
        let expected = RefIdentity {
            role: "button".to_string(),
            name: "Go".to_string(),
        };
        let script = action_expression("e2", 7, &expected, "node.click();");
        assert!(
            script.contains(r#"[data-wardian-ref=\"e2\"][data-wardian-snapshot=\"7\"]"#)
                || script.contains("data-wardian-snapshot"),
            "the selector must pin the snapshot generation: {script}"
        );
        assert!(script.contains("\"button\""), "the expected role must be embedded");
        assert!(script.contains("\"Go\""), "the expected name must be embedded");
        assert!(script.contains("matches.length > 1"), "an ambiguous ref must be refused");
        assert!(script.contains("return 'changed'"), "a repurposed node must be refused");
        assert!(script.contains("node.click();"));
    }

    #[test]
    fn the_guard_clamps_identity_the_same_way_the_ledger_recorded_it() {
        // The ledger stores clamped fields. A guard that compared the raw name
        // would refuse every element whose name exceeds the cap.
        let script = identity_js();
        assert!(script.contains(&format!("const MAX_FIELD = {MAX_SNAPSHOT_FIELD_CHARS};")));
        assert!(script.contains("const clampField"));
        let guard = action_expression(
            "e1",
            1,
            &RefIdentity {
                role: "link".to_string(),
                name: "x".to_string(),
            },
            "node.click();",
        );
        assert!(guard.contains("clampField(accessibleName(node))"));
        assert!(guard.contains("clampField(roleOf(node))"));
    }

    #[test]
    fn the_action_guard_and_the_walker_derive_identity_the_same_way() {
        // Drift between the two would produce spurious `ref_changed` refusals.
        let walker = snapshot_expression(1, true);
        let guard = action_expression(
            "e1",
            1,
            &RefIdentity {
                role: "link".to_string(),
                name: "Home".to_string(),
            },
            "node.click();",
        );
        assert!(walker.contains(&identity_js()));
        assert!(guard.contains(&identity_js()));
    }

    #[test]
    fn a_changed_or_ambiguous_ref_has_its_own_code_and_advice() {
        let changed = RefError::Changed {
            element_ref: "e2".to_string(),
        };
        assert_eq!(changed.code(), "ref_changed");
        assert!(changed.to_string().contains("Re-run snapshot"));
        let ambiguous = RefError::Ambiguous {
            element_ref: "e2".to_string(),
        };
        assert_eq!(ambiguous.code(), "ref_ambiguous");
        assert!(ambiguous.to_string().contains("more than one element"));
    }

    #[test]
    fn clamps_long_fields_and_collapses_whitespace() {
        assert_eq!(clamp_field("  a \n b  "), "a b");
        let long = "x".repeat(MAX_SNAPSHOT_FIELD_CHARS + 40);
        let clamped = clamp_field(&long);
        assert_eq!(clamped.chars().count(), MAX_SNAPSHOT_FIELD_CHARS);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn clamps_on_a_character_boundary_for_multi_byte_text() {
        let long = "é".repeat(MAX_SNAPSHOT_FIELD_CHARS + 10);
        let clamped = clamp_field(&long);
        assert_eq!(clamped.chars().count(), MAX_SNAPSHOT_FIELD_CHARS);
    }

    #[test]
    fn parses_a_walker_result_into_clamped_elements() {
        let value = json!({
            "url": "https://example.com/",
            "title": "Example",
            "truncated": false,
            "elements": [
                { "element_ref": "e1", "role": "link", "name": "  More   info  ", "value": "", "enabled": true },
                { "element_ref": "e2", "role": "checkbox", "name": "Agree", "value": "", "enabled": false, "checked": true },
                { "element_ref": "bogus", "role": "link", "name": "skipped" }
            ]
        });
        let snapshot = parse_snapshot(4, true, &value).expect("snapshot");
        assert_eq!(snapshot.generation, 4);
        assert!(snapshot.interactive_only);
        assert_eq!(snapshot.elements.len(), 2, "malformed refs are dropped");
        assert_eq!(snapshot.elements[0].name, "More info");
        assert_eq!(snapshot.elements[1].checked, Some(true));
        assert!(!snapshot.elements[1].enabled);
    }


    #[test]
    fn rejects_a_result_without_an_elements_array() {
        assert!(parse_snapshot(1, true, &json!({ "url": "x" })).is_err());
    }


    #[test]
    fn the_injected_walker_carries_the_generation_and_caps_it_was_built_with() {
        let script = snapshot_expression(9, true);
        assert!(script.contains("const generation = 9;"));
        assert!(script.contains("const INTERACTIVE_ONLY = true;"));
        assert!(script.contains(&format!("const MAX = {MAX_SNAPSHOT_ELEMENTS};")));
        assert!(script.contains(REF_ATTRIBUTE));
    }

    #[test]
    fn the_walker_reports_checked_only_for_checkable_roles() {
        let script = snapshot_expression(1, true);
        assert!(
            script.contains("role === 'checkbox' || role === 'radio'"),
            "a text input also has a boolean `checked`, so the walker must gate on role"
        );
        assert!(!script.contains("typeof element.checked === 'boolean'"));
    }
}
