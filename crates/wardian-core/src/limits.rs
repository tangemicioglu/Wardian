//! Bounds on the collections Wardian will materialise or return in one page.
//!
//! These were introduced together, to keep a large workspace, library, run
//! history, or interaction database from producing an unbounded response. They
//! then lived as nine separate magic numbers in nine files, which made them
//! impossible to compare and easy to diverge.
//!
//! Naming them in one place makes the policy legible: a page size is a product
//! decision about how much an operator sees at once, not an implementation
//! detail of the module that happens to enumerate the directory.
//!
//! # What a bound here does and does not promise
//!
//! Each constant bounds what one call **returns**. It does not by itself bound
//! what the call **materialises** on the way there — several producers still
//! collect and sort a full set before taking a page. Where that distinction
//! matters for a given surface it is called out on the constant.
//!
//! Every capped surface reports `truncated` and, where a continuation exists,
//! a `next_offset`. A caller that drops either presents a partial result as
//! complete, which is the failure these bounds exist to prevent.

/// Directory children returned per `get_directory_tree` page.
///
/// A source tree with a large `node_modules` is the ordinary case, not the
/// pathological one.
pub const MAX_DIRECTORY_CHILDREN: usize = 500;

/// Changed files returned in one Git status result.
pub const MAX_GIT_STATUS_FILES: usize = 1_000;

/// Inbox notifications returned per page, newest first.
///
/// The producer sorts the whole interaction set before taking a page, so this
/// bounds the response rather than the work.
pub const MAX_INBOX_NOTIFICATIONS: usize = 200;

/// Interaction records scanned for the topology activity view.
pub const MAX_ACTIVITY_RECORDS: usize = 5_000;

/// Distinct agent pairs reported by the topology activity view.
pub const MAX_ACTIVITY_PAIRS: usize = 1_000;

/// Workflow blueprint files listed per page.
pub const MAX_WORKFLOW_BLUEPRINTS: usize = 500;

/// Workflow runs listed per page, newest first.
pub const MAX_WORKFLOW_RUNS: usize = 200;

/// Skill source directories considered when resolving library deployments.
pub const MAX_LIBRARY_SKILL_SOURCES: usize = 1_000;

/// Deployment records returned for the library.
pub const MAX_LIBRARY_DEPLOYMENTS: usize = 2_000;

/// Nodes returned per library index section.
pub const MAX_LIBRARY_NODES_PER_SECTION: usize = 1_000;

/// Directory depth the library index will recurse to.
///
/// Unlike the others this is a guard rather than a page size: it has no
/// continuation, so a tree deeper than this is simply not indexed below the
/// limit. Junction loops make an unbounded walk a real possibility.
pub const MAX_LIBRARY_DEPTH: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    /// A page size of zero would return nothing and report no truncation,
    /// which reads to every consumer as a legitimately empty collection.
    #[test]
    fn every_bound_is_non_zero() {
        for (name, value) in [
            ("MAX_DIRECTORY_CHILDREN", MAX_DIRECTORY_CHILDREN),
            ("MAX_GIT_STATUS_FILES", MAX_GIT_STATUS_FILES),
            ("MAX_INBOX_NOTIFICATIONS", MAX_INBOX_NOTIFICATIONS),
            ("MAX_ACTIVITY_RECORDS", MAX_ACTIVITY_RECORDS),
            ("MAX_ACTIVITY_PAIRS", MAX_ACTIVITY_PAIRS),
            ("MAX_WORKFLOW_BLUEPRINTS", MAX_WORKFLOW_BLUEPRINTS),
            ("MAX_WORKFLOW_RUNS", MAX_WORKFLOW_RUNS),
            ("MAX_LIBRARY_SKILL_SOURCES", MAX_LIBRARY_SKILL_SOURCES),
            ("MAX_LIBRARY_DEPLOYMENTS", MAX_LIBRARY_DEPLOYMENTS),
            (
                "MAX_LIBRARY_NODES_PER_SECTION",
                MAX_LIBRARY_NODES_PER_SECTION,
            ),
            ("MAX_LIBRARY_DEPTH", MAX_LIBRARY_DEPTH),
        ] {
            assert!(value > 0, "{name} must be greater than zero");
        }
    }

    /// The activity view reports pairs derived from records, so a pair budget
    /// above the record budget could never be reached and would mislead a
    /// reader about what the view actually shows.
    #[test]
    fn activity_pairs_cannot_exceed_the_records_they_come_from() {
        assert!(MAX_ACTIVITY_PAIRS <= MAX_ACTIVITY_RECORDS);
    }
}
