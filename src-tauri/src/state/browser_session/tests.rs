//! Engine-backed integration tests.
//!
//! These launch a real Chromium and are `#[ignore]`d so a machine without one
//! still runs the suite. Run them with:
//!
//! ```text
//! cargo test --lib browser_session::tests -- --ignored --test-threads=1
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::actor::{
    BrowserSessionBroker, ElementAction, LoadState, OpenBrowserRequest, PageField, Viewport,
    WaitCondition,
};
use super::engine::discover_engine;

const FIXTURE: &str = r#"<!doctype html>
<html>
  <head><title>Wardian Fixture</title></head>
  <body>
    <h1 id="heading">Browser surface fixture</h1>
    <input id="search" type="text" placeholder="Search" />
    <button id="go" onclick="document.getElementById('result').textContent = 'clicked ' + document.getElementById('search').value">Go</button>
    <p id="result"></p>
    <a href="/second">Second page</a>
  </body>
</html>"#;

const SECOND_PAGE: &str = r#"<!doctype html>
<html><head><title>Second</title></head><body><p id="marker">arrived</p></body></html>"#;

/// Serves the fixture pages on an ephemeral loopback port.
///
/// A hand-rolled responder keeps the test free of a web-framework dependency
/// and of any listener that outlives the test process.
async fn serve_fixture() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buffer = vec![0_u8; 2048];
                let Ok(read) = stream.read(&mut buffer).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let body = if request.starts_with("GET /second") {
                    SECOND_PAGE
                } else {
                    FIXTURE
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}/"), handle)
}

fn broker() -> BrowserSessionBroker {
    BrowserSessionBroker::new(
        std::env::temp_dir().join(format!("wardian-browser-it-{}", uuid::Uuid::new_v4())),
    )
}

#[test]
#[ignore = "requires a Chromium-based browser on the host"]
fn an_engine_is_discoverable_on_this_host() {
    let engine = discover_engine().expect("a browser should be installed");
    assert!(engine.path.is_file());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn opens_a_page_and_reads_it_back() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url.clone()),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");

    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("load");

    assert_eq!(
        session.get(PageField::Title, None).await.expect("title"),
        "Wardian Fixture"
    );
    let text = session.get(PageField::Text, None).await.expect("text");
    assert!(text.contains("Browser surface fixture"), "text was {text:?}");
    let scoped = session
        .get(PageField::Text, Some("#heading"))
        .await
        .expect("scoped text");
    assert_eq!(scoped, "Browser surface fixture");

    let summary = session.summary().await;
    assert_eq!(summary.short_ref, "browser:1");
    assert!(summary.url.starts_with(&base_url));

    broker.close(&summary.browser_id).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn snapshots_mint_refs_that_actions_can_use() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("load");

    let snapshot = session.snapshot(true).await.expect("snapshot");
    assert!(!snapshot.elements.is_empty());
    let search = snapshot
        .elements
        .iter()
        .find(|element| element.name == "Search")
        .expect("the search box should be in an interactive snapshot");
    let button = snapshot
        .elements
        .iter()
        .find(|element| element.name.trim() == "Go")
        .expect("the button should be in an interactive snapshot");

    session
        .act(&search.element_ref, &ElementAction::Fill("wardian".to_string()))
        .await
        .expect("fill");
    session
        .act(&button.element_ref, &ElementAction::Click)
        .await
        .expect("click");
    session
        .wait(&WaitCondition::Text("clicked wardian".to_string()), 10_000)
        .await
        .expect("the click should have run the page's handler");

    broker
        .close(session.browser_id())
        .await
        .expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_navigation_makes_earlier_refs_stale_instead_of_misclicking() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url.clone()),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("load");
    let snapshot = session.snapshot(true).await.expect("snapshot");
    let stale_ref = snapshot.elements.first().expect("an element").element_ref.clone();

    session
        .navigate(&format!("{base_url}second"))
        .await
        .expect("navigate");
    session
        .wait(&WaitCondition::UrlContains("/second".to_string()), 10_000)
        .await
        .expect("second page");

    let error = session
        .act(&stale_ref, &ElementAction::Click)
        .await
        .expect_err("a ref from the previous page must not be actionable");
    assert_eq!(error.code(), "snapshot_stale", "got {error}");

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn navigation_still_completes_while_a_screencast_is_streaming() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url.clone()),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("load");

    // A surface attaching is what makes this different from a headless CLI
    // session, and it is the case that regressed.
    session.attach_screencast().await.expect("attach");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    session
        .navigate(&format!("{base_url}second"))
        .await
        .expect("navigate must not hang while frames are streaming");
    session
        .wait(&WaitCondition::UrlContains("/second".to_string()), 10_000)
        .await
        .expect("second page");

    session.detach_screencast().await.expect("detach");
    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn waits_report_a_timeout_rather_than_hanging() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    let error = session
        .wait(&WaitCondition::Selector("#never-present".to_string()), 800)
        .await
        .expect_err("the selector is not on the page");
    assert_eq!(error.code(), "browser_wait_timeout");

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn captures_a_screenshot_and_honors_a_viewport_override() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("load");

    session
        .set_viewport(Some(Viewport {
            width: 640,
            height: 480,
        }))
        .await
        .expect("viewport");
    assert_eq!(session.summary().await.viewport.width, 640);

    let path: PathBuf = std::env::temp_dir()
        .join(format!("wardian-shot-{}.png", uuid::Uuid::new_v4()))
        .to_path_buf();
    session.screenshot(&path, false).await.expect("screenshot");
    let bytes = std::fs::read(&path).expect("screenshot file");
    assert!(bytes.len() > 1000, "screenshot was {} bytes", bytes.len());
    assert_eq!(&bytes[1..4], b"PNG");
    let _ = std::fs::remove_file(&path);

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn short_refs_and_id_prefixes_both_resolve() {
    let broker = Arc::new(broker());
    let first = broker.open(OpenBrowserRequest::default()).await.expect("first");
    let second = broker.open(OpenBrowserRequest::default()).await.expect("second");

    assert_eq!(
        broker.resolve("browser:1").await.expect("short ref").browser_id(),
        first.browser_id()
    );
    assert_eq!(
        broker.resolve(second.browser_id()).await.expect("uuid").browser_id(),
        second.browser_id()
    );
    let prefix = &second.browser_id()[..8];
    assert_eq!(
        broker.resolve(prefix).await.expect("prefix").browser_id(),
        second.browser_id()
    );

    let listed = broker.list().await;
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].short_ref, "browser:1");
    assert_eq!(listed[1].short_ref, "browser:2");

    broker.shutdown_all().await;
    assert!(broker.list().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn closing_an_agents_sessions_leaves_other_sessions_running() {
    let broker = broker();
    let owned = broker
        .open(OpenBrowserRequest {
            owner_agent_id: Some("agent-1".to_string()),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("owned");
    let unowned = broker.open(OpenBrowserRequest::default()).await.expect("unowned");

    let closed = broker.close_for_agent("agent-1").await;
    assert_eq!(closed, vec![owned.browser_id().to_string()]);
    let remaining = broker.list().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].browser_id, unowned.browser_id());

    broker.shutdown_all().await;
}
