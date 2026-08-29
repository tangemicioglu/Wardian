//! Stable identities for records and files.
//!
//! Deduplication needs a key that depends only on the content of a record, never
//! on how a read happened to be chunked. Position would be the obvious choice
//! and is the wrong one: a byte offset survives appends but not a file being
//! rewritten, and reusing offsets across a replaced file would make new records
//! collide with old ones and silently vanish into `INSERT OR IGNORE`.

/// FNV-1a 64-bit, written out rather than taken from the standard library.
///
/// `DefaultHasher` would be simpler and is wrong here for one reason: its output
/// is explicitly not guaranteed to be stable across Rust releases. These digests
/// are persisted as deduplication keys, so a toolchain upgrade would silently
/// re-key every future read of an already-ingested record — and the next full
/// re-read would then insert duplicates of rows already in the store instead of
/// colliding with them. FNV-1a is a fixed, published algorithm, so the keys mean
/// the same thing in five years as they do today.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A short, stable, non-cryptographic digest of some bytes.
///
/// This is a deduplication key, not a security primitive: it defends against
/// re-reading the same record, not against an adversary constructing a
/// collision. The inputs are whole provider log lines, which carry millisecond
/// timestamps and monotonically changing token counters, so two distinct records
/// being byte-identical does not arise in practice.
pub fn content_key(bytes: &[u8]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Identity of an append-only file, taken from its first line.
///
/// A byte-offset cursor is only meaningful against the file it was taken from.
/// Detecting replacement by length alone catches a file that shrank and misses
/// one replaced by something the same size or larger, which then resumes at an
/// offset into unrelated content and drops everything before it.
///
/// Only the first line is hashed, and that is the whole point: the fingerprint
/// has to stay constant while the file grows, or every append would look like a
/// replacement and trigger a full re-read forever. Codex opens each rollout with
/// a session-meta record carrying the session's own id, so line one identifies
/// the file and never changes afterwards.
///
/// `None` until a complete first line exists — a file too new to identify is not
/// the same claim as a file that changed.
pub fn file_fingerprint(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = [0_u8; MAX_FINGERPRINT_BYTES];
    let mut filled = 0;
    // A single `read` is allowed to return less than was asked for, so a short
    // read must not be mistaken for a short file.
    while filled < head.len() {
        match file.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(_) => return None,
        }
    }

    match head[..filled].iter().position(|byte| *byte == b'\n') {
        Some(line_end) => Some(content_key(&head[..line_end])),
        // No line terminator within the window. If the window is full, those
        // bytes are still a stable identity: the file is append-only, so its
        // first N bytes never change once written. Falling back to `None` here
        // would leave a source with an unusually long first line permanently
        // unable to detect replacement.
        None if filled == head.len() => Some(content_key(&head)),
        // Genuinely short and still being written; too young to identify.
        None => None,
    }
}

/// How far to look for the end of the first line.
///
/// Bounded so a pathological source cannot pull an unbounded read into memory.
/// Beyond it the window itself becomes the identity, which is sound for the
/// append-only files this applies to.
const MAX_FINGERPRINT_BYTES: usize = 8 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_content_keys_match() {
        assert_eq!(content_key(b"same record"), content_key(b"same record"));
    }

    #[test]
    fn different_content_keys_differ() {
        assert_ne!(content_key(b"record a"), content_key(b"record b"));
    }

    #[test]
    fn content_keys_match_published_fnv1a_vectors() {
        // Pinned against the published algorithm rather than against whatever
        // this build happens to produce. These keys are persisted, so a silent
        // change to the digest would make a re-read duplicate rows it should
        // have collided with — and a test that only compares the code to itself
        // would not notice.
        assert_eq!(content_key(b""), "cbf29ce484222325");
        assert_eq!(content_key(b"a"), "af63dc4c8601ec8c");
        assert_eq!(content_key(b"foobar"), "85944171f73967e8");
    }

    #[test]
    fn a_single_byte_difference_changes_the_key() {
        // Adjacent token counters differ by very little; the key has to
        // separate them anyway.
        let left = content_key(br#"{"input_tokens":100544}"#);
        let right = content_key(br#"{"input_tokens":100545}"#);
        assert_ne!(left, right);
    }

    #[test]
    fn fingerprint_identifies_the_file_not_its_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");

        std::fs::write(&path, "first session\nmore\n").unwrap();
        let original = file_fingerprint(&path).unwrap();

        // Appending must not change identity, or every ingest would look like a
        // replacement and re-read the whole file.
        std::fs::write(&path, "first session\nmore\nand more\n").unwrap();
        assert_eq!(file_fingerprint(&path).as_deref(), Some(original.as_str()));

        // A different file at the same path, deliberately longer than the
        // original: exactly the replacement a length comparison cannot see.
        std::fs::write(&path, "second session\nwith considerably more content\n").unwrap();
        assert_ne!(file_fingerprint(&path).as_deref(), Some(original.as_str()));
    }

    #[test]
    fn a_missing_file_has_no_fingerprint() {
        assert_eq!(
            file_fingerprint(std::path::Path::new("nowhere.jsonl")),
            None
        );
    }

    #[test]
    fn an_empty_file_has_no_fingerprint() {
        // Nothing to identify yet; treating it as a distinct identity would
        // force a spurious re-read on the first write.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.jsonl");
        std::fs::write(&path, "").unwrap();
        assert_eq!(file_fingerprint(&path), None);
    }

    #[test]
    fn a_very_long_first_line_is_still_identifiable() {
        // Returning None here would leave such a source permanently unable to
        // notice replacement, since the length check only catches a file that
        // shrank. The window itself is a stable identity for an append-only
        // file, because its opening bytes never change once written.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.jsonl");

        let long_line = "x".repeat(MAX_FINGERPRINT_BYTES * 2);
        std::fs::write(&path, format!("{long_line}\n")).unwrap();
        let original = file_fingerprint(&path).unwrap();

        // Appending leaves the opening bytes untouched, so identity holds.
        std::fs::write(&path, format!("{long_line}\nmore\n")).unwrap();
        assert_eq!(file_fingerprint(&path).as_deref(), Some(original.as_str()));

        // A different long-first-line file is distinguishable.
        let other = format!("y{}", "x".repeat(MAX_FINGERPRINT_BYTES * 2));
        std::fs::write(&path, format!("{other}\n")).unwrap();
        assert_ne!(file_fingerprint(&path).as_deref(), Some(original.as_str()));
    }

    #[test]
    fn a_file_without_a_complete_first_line_has_no_fingerprint() {
        // Identifying a half-written line would fix an identity that changes
        // the moment the writer finishes it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.jsonl");
        std::fs::write(&path, "{\"type\":\"session_me").unwrap();
        assert_eq!(file_fingerprint(&path), None);
    }

    #[test]
    fn growth_alone_never_changes_identity() {
        // The failure mode this rules out: a fingerprint that moved with the
        // file would mark every append as a replacement, so each ingest cycle
        // would re-read the log from byte zero.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(&path, "session one\n").unwrap();
        let original = file_fingerprint(&path).unwrap();

        let mut content = String::from("session one\n");
        for index in 0..500 {
            content.push_str(&format!("record {index}\n"));
            std::fs::write(&path, &content).unwrap();
            assert_eq!(file_fingerprint(&path).as_deref(), Some(original.as_str()));
        }
    }
}
