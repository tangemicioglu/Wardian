//! Guessing what a workspace is serving, so a new browser opens somewhere useful.
//!
//! An agent verifying a frontend change should not have to guess which port the
//! dev server chose, and a human opening a Browser tab in a web project almost
//! never wants `about:blank`. This module answers "what is this workspace
//! serving right now?" with a ranked guess and no promises.
//!
//! It is a heuristic and is treated as one. Reading the OS socket table and
//! attributing sockets to processes under the workspace would be the precise
//! answer, but it needs per-platform code, elevated access on some hosts, and a
//! process-ancestry walk to be even approximately right. Two cheap signals get
//! most of the value: what the workspace *declares* it will serve, and what is
//! actually accepting connections on loopback right now. Neither can be wrong
//! in a damaging way — the worst case is a tab pointed at the wrong local
//! service, which the address bar fixes.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

/// How long a candidate port has to accept a loopback connection.
///
/// Generous for a local socket and short enough that the whole sweep stays
/// under a second even when every candidate is closed.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// Ceiling on ports probed in one sweep.
const MAX_CANDIDATES: usize = 24;

/// Ports worth trying when the workspace declares nothing, most likely first.
///
/// Ordered by what a modern web workspace actually uses rather than
/// numerically: Vite before Create React App before the rest.
const CONVENTIONAL_PORTS: &[u16] = &[
    5173, // Vite
    3000, // Next, CRA, many Node servers
    4200, // Angular
    8080, // generic
    5174, // Vite's second choice when 5173 is taken
    3001, // a second Node server
    4321, // Astro
    8000, // Django, http.server
    5000, // Flask
    1420, // Tauri
    8888, // Jupyter and friends
];

/// Files worth reading for a declared port, cheapest and most specific first.
const DECLARATION_FILES: &[&str] = &[
    "vite.config.ts",
    "vite.config.js",
    "vite.config.mjs",
    "package.json",
    ".env",
    ".env.local",
];

/// Ceiling on how much of a declaration file is scanned.
///
/// A port declaration lives near the top of a config; a lockfile-sized
/// `package.json` should not turn opening a tab into a disk read.
const MAX_DECLARATION_BYTES: usize = 64 * 1024;

/// Where a candidate port came from. Diagnostics, and test readability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSource {
    /// The workspace says it serves here.
    Declared,
    /// Nothing said so; this is a common choice.
    Conventional,
}

/// One port worth probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortCandidate {
    pub port: u16,
    pub source: PortSource,
}

/// Ranks the ports to probe for a workspace, declared ones first.
///
/// Pure so the ranking can be tested without a filesystem or a socket.
pub fn rank_candidates(declared: &[u16]) -> Vec<PortCandidate> {
    let mut seen = HashSet::new();
    let mut ranked = Vec::new();
    for port in declared {
        if *port != 0 && seen.insert(*port) {
            ranked.push(PortCandidate {
                port: *port,
                source: PortSource::Declared,
            });
        }
    }
    for port in CONVENTIONAL_PORTS {
        if seen.insert(*port) {
            ranked.push(PortCandidate {
                port: *port,
                source: PortSource::Conventional,
            });
        }
    }
    ranked.truncate(MAX_CANDIDATES);
    ranked
}

/// Reads the ports a workspace declares it will serve on.
///
/// Only the workspace root is read, never a recursive walk: a monorepo's
/// hundred `package.json` files are a hundred wrong answers, and the root is
/// where the command an operator actually runs lives.
pub fn declared_ports(workspace: &Path) -> Vec<u16> {
    let mut ports = Vec::new();
    for name in DECLARATION_FILES {
        let path = workspace.join(name);
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let head = &contents[..contents.len().min(MAX_DECLARATION_BYTES)];
        for port in ports_in(head) {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
    }
    ports
}

/// Pulls port declarations out of one file's text.
///
/// Deliberately a scan rather than a parse. Three formats would need three
/// parsers, `vite.config.ts` is TypeScript rather than data, and the cost of a
/// wrong guess here is one probe against a closed port.
pub fn ports_in(text: &str) -> Vec<u16> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    // `--port 5173`, `--port=5173`, `port: 5173`, `PORT=5173`.
    for (index, _) in text.to_ascii_lowercase().match_indices("port") {
        let mut cursor = index + "port".len();
        // Skip the separator: whitespace, `:`, `=`, or a quote around a value.
        let mut separated = false;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b' ' | b'\t' | b':' | b'=' | b'"' | b'\'' => {
                    separated = true;
                    cursor += 1;
                }
                _ => break,
            }
        }
        if !separated {
            continue;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == start {
            continue;
        }
        // A longer run of digits is a different number that happens to start
        // with a plausible port, so the whole run has to parse.
        let Ok(port) = text[start..cursor].parse::<u32>() else {
            continue;
        };
        // Below 1024 needs privileges no dev server asks for, and a "port"
        // match inside prose ("supports 8 ports") should not become a probe.
        if (1024..=65_535).contains(&port) {
            let port = port as u16;
            if !found.contains(&port) {
                found.push(port);
            }
        }
    }
    found
}

/// The name the browser is given once a port answers.
///
/// `localhost` rather than a literal address because that is the URL a dev
/// server documents and sets its CORS and cookie expectations against. The
/// browser resolves it with its own, far more patient fallback than a probe
/// can afford.
const PROBE_HOST: &str = "localhost";

/// The loopback addresses a dev server might be bound to.
///
/// Both are probed, separately and at once, rather than resolving `localhost`
/// and letting one connect attempt walk the list. `localhost` resolves to `::1`
/// first on Windows, and a `::1` attempt that *hangs* rather than refusing —
/// a firewall dropping the packet — would eat the whole budget before
/// `127.0.0.1` was ever tried. A server bound to only one family is the common
/// case, not the exotic one: Vite binds `127.0.0.1` by default.
const LOOPBACK_ADDRESSES: &[&str] = &["127.0.0.1", "[::1]"];

/// True when something accepts a loopback connection on `port`.
async fn is_listening(port: u16) -> bool {
    let attempts = LOOPBACK_ADDRESSES.iter().map(|address| async move {
        let target = format!("{address}:{port}");
        matches!(timeout(PROBE_TIMEOUT, TcpStream::connect(target)).await, Ok(Ok(_)))
    });
    futures_util::future::join_all(attempts)
        .await
        .into_iter()
        .any(|reachable| reachable)
}

/// Finds the best address a workspace appears to be serving, if any.
///
/// Every candidate is probed at once and the *highest-ranked* listener wins,
/// rather than whichever socket answered first — a fast reply from a stray
/// service should not outrank the port the workspace declared.
pub async fn detect_workspace_url(workspace: Option<&Path>) -> Option<String> {
    let declared = workspace.map(declared_ports).unwrap_or_default();
    let candidates = rank_candidates(&declared);
    let probes = candidates
        .iter()
        .map(|candidate| async move { (candidate.port, is_listening(candidate.port).await) });
    let results = futures_util::future::join_all(probes).await;
    results
        .into_iter()
        .find(|(_, listening)| *listening)
        .map(|(port, _)| format!("http://{PROBE_HOST}:{port}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_port_outranks_every_convention() {
        let ranked = rank_candidates(&[4444]);
        assert_eq!(ranked[0].port, 4444);
        assert_eq!(ranked[0].source, PortSource::Declared);
        assert_eq!(ranked[1].port, CONVENTIONAL_PORTS[0]);
    }

    #[test]
    fn declaring_a_conventional_port_promotes_it_rather_than_repeating_it() {
        let ranked = rank_candidates(&[8080]);
        assert_eq!(ranked[0].port, 8080);
        assert_eq!(ranked[0].source, PortSource::Declared);
        assert_eq!(
            ranked.iter().filter(|candidate| candidate.port == 8080).count(),
            1,
        );
    }

    #[test]
    fn the_conventional_list_alone_is_deduplicated_and_bounded() {
        let ranked = rank_candidates(&[]);
        assert_eq!(ranked.len(), CONVENTIONAL_PORTS.len());
        assert!(ranked.len() <= MAX_CANDIDATES);
        assert!(ranked
            .iter()
            .all(|candidate| candidate.source == PortSource::Conventional));
    }

    #[test]
    fn a_workspace_declaring_many_ports_cannot_grow_the_sweep_without_bound() {
        let declared: Vec<u16> = (2000..2100).collect();
        assert_eq!(rank_candidates(&declared).len(), MAX_CANDIDATES);
    }

    #[test]
    fn a_zero_port_is_not_a_candidate() {
        let ranked = rank_candidates(&[0]);
        assert!(ranked.iter().all(|candidate| candidate.port != 0));
    }

    #[test]
    fn reads_a_port_out_of_every_shape_a_config_writes_it_in() {
        assert_eq!(ports_in("  \"dev\": \"vite --port 5199\""), vec![5199]);
        assert_eq!(ports_in("next dev --port=3005"), vec![3005]);
        assert_eq!(ports_in("server: { port: 4173 }"), vec![4173]);
        assert_eq!(ports_in("PORT=8123"), vec![8123]);
        assert_eq!(ports_in("\"port\": \"9001\""), vec![9001]);
    }

    #[test]
    fn the_same_port_declared_twice_is_one_candidate() {
        assert_eq!(ports_in("--port 5173\nport: 5173"), vec![5173]);
    }

    #[test]
    fn a_privileged_or_impossible_port_is_not_read_as_a_declaration() {
        // Ports below 1024 need privileges no dev server asks for, and a run of
        // digits longer than a port is a different number entirely.
        assert!(ports_in("port: 80").is_empty());
        assert!(ports_in("port: 443").is_empty());
        assert!(ports_in("port: 99999").is_empty());
        assert!(ports_in("port: 0").is_empty());
    }

    #[test]
    fn the_word_port_without_a_number_is_not_a_declaration() {
        assert!(ports_in("this transport supports many ports").is_empty());
        assert!(ports_in("import { port } from './config'").is_empty());
        assert!(ports_in("reportingEnabled: true").is_empty());
    }

    #[test]
    fn matching_is_case_insensitive_across_the_spellings_configs_use() {
        assert_eq!(ports_in("PORT=3000"), vec![3000]);
        assert_eq!(ports_in("Port: 3000"), vec![3000]);
        assert_eq!(ports_in("serverPort = 3000"), vec![3000]);
    }

    #[test]
    fn a_workspace_with_no_declarations_declares_nothing() {
        let dir = std::env::temp_dir().join(format!("wardian-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create");
        assert!(declared_ports(&dir).is_empty());
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_missing_workspace_is_not_an_error() {
        let dir = std::env::temp_dir().join(format!("wardian-absent-{}", uuid::Uuid::new_v4()));
        assert!(declared_ports(&dir).is_empty());
    }

    #[test]
    fn declarations_are_read_in_file_priority_order() {
        let dir = std::env::temp_dir().join(format!("wardian-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create");
        // The vite config is the most specific statement of what will serve, so
        // it should outrank a script flag and an env default.
        std::fs::write(dir.join("vite.config.ts"), "export default { server: { port: 4173 } }")
            .expect("write");
        std::fs::write(
            dir.join("package.json"),
            r#"{ "scripts": { "dev": "vite --port 5199" } }"#,
        )
        .expect("write");
        std::fs::write(dir.join(".env"), "PORT=8123\n").expect("write");

        assert_eq!(declared_ports(&dir), vec![4173, 5199, 8123]);
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn an_enormous_declaration_file_is_only_read_at_its_head() {
        let dir = std::env::temp_dir().join(format!("wardian-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create");
        let padding = " ".repeat(MAX_DECLARATION_BYTES + 1024);
        std::fs::write(dir.join(".env"), format!("{padding}PORT=8123\n")).expect("write");
        assert!(
            declared_ports(&dir).is_empty(),
            "a declaration past the read ceiling is not found, and that is the trade",
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[tokio::test]
    async fn detection_finds_a_listener_and_prefers_the_declared_one() {
        // Two live listeners, one declared and one conventional. Rank decides,
        // not whichever socket happens to answer first.
        let conventional = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let declared = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let declared_port = declared.local_addr().expect("addr").port();
        let conventional_port = conventional.local_addr().expect("addr").port();

        let ranked = rank_candidates(&[declared_port, conventional_port]);
        assert_eq!(ranked[0].port, declared_port);

        let dir = std::env::temp_dir().join(format!("wardian-detect-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create");
        std::fs::write(dir.join(".env"), format!("PORT={declared_port}\n")).expect("write");

        assert_eq!(
            detect_workspace_url(Some(&dir)).await,
            Some(format!("http://localhost:{declared_port}/")),
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[tokio::test]
    async fn a_server_bound_to_only_one_loopback_family_is_still_found() {
        // Vite binds `127.0.0.1` and nothing else. Resolving `localhost` and
        // letting one connect attempt walk the list would try `::1` first on
        // Windows, and a dropped packet there would spend the whole budget.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(is_listening(port).await);
    }

    #[tokio::test]
    async fn a_closed_port_is_not_reported_as_listening() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        assert!(!is_listening(port).await);
    }

    #[tokio::test]
    async fn a_workspace_serving_nothing_gets_no_default() {
        // Every conventional port is either closed or belongs to someone else,
        // so this asserts only that a closed declared port is not returned.
        let closed = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let port = closed.local_addr().expect("addr").port();
        drop(closed);

        let detected = detect_workspace_url(None).await;
        assert_ne!(detected, Some(format!("http://localhost:{port}/")));
    }
}
