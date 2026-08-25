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
    BrowserSessionBroker, ElementAction, LoadState, OpenBrowserRequest, PageField, PointerEvent,
    Viewport, WaitCondition,
};
use super::engine::discover_engine;
use wardian_core::browser::{
    CookieAction, NetworkFilter, StatusFilter, StorageAction, StorageArea, MAX_STORAGE_VALUE_CHARS,
};

const FIXTURE: &str = r#"<!doctype html>
<html>
  <head><title>Wardian Fixture</title></head>
  <body>
    <h1 id="heading">Browser surface fixture</h1>
    <input id="search" type="text" placeholder="Search" />
    <button id="go" onclick="document.getElementById('result').textContent = 'clicked ' + document.getElementById('search').value">Go</button>
    <p id="result"></p>
    <a href="/second">Second page</a>
    <button id="verbose">VERBOSE_LABEL</button>
  </body>
</html>"#;

/// A label longer than the field cap, so the clamp path is exercised.
fn fixture_with_long_label() -> String {
    FIXTURE.replace("VERBOSE_LABEL", &"Download the quarterly report ".repeat(12))
}

const SECOND_PAGE: &str = r#"<!doctype html>
<html><head><title>Second</title></head><body><p id="marker">arrived</p></body></html>"#;

/// A page whose iframe routes itself without touching the top-level document.
const IFRAME_HOST: &str = r##"<!doctype html>
<html>
  <head><title>Iframe Host</title></head>
  <body>
    <h1 id="heading">Outer document</h1>
    <button id="go">Go</button>
    <iframe id="inner" src="/inner"></iframe>
  </body>
</html>"##;

/// Loaded inside the iframe. The test drives its routing, so the moment it
/// happens is known rather than raced against.
const IFRAME_INNER: &str = r##"<!doctype html>
<html>
  <head><title>Inner</title></head>
  <body>
    <p id="inner-marker">inner</p>
  </body>
</html>"##;

/// Fires one request that succeeds and one that 404s, then says it is done.
///
/// Both outcomes come from one page load so a single navigation exercises the
/// whole ledger: a document, a successful fetch, and a failure.
const NETWORK_PAGE: &str = r##"<!doctype html>
<html>
  <head><title>Network</title></head>
  <body>
    <p id="status">pending</p>
    <script>
      (async () => {
        await fetch('/api/ok').then((response) => response.text());
        await fetch('/api/missing');
        document.getElementById('status').textContent = 'settled';
      })();
    </script>
  </body>
</html>"##;

/// A page whose only job is to start a download with a known filename.
const DOWNLOAD_PAGE: &str = r##"<!doctype html>
<html>
  <head><title>Download</title></head>
  <body><a id="grab" href="/report.csv" download>Download the report</a></body>
</html>"##;

/// One canned HTTP response.
struct FixtureResponse {
    status: &'static str,
    content_type: &'static str,
    extra_headers: String,
    body: String,
}

/// Routes one request line to its response.
///
/// Split out from the listener so a route can be read at a glance and the
/// listener stays a plain read/write loop.
fn fixture_response(request: &str) -> FixtureResponse {
    let html = |body: &str| FixtureResponse {
        status: "200 OK",
        content_type: "text/html; charset=utf-8",
        extra_headers: String::new(),
        body: body.to_string(),
    };
    if request.starts_with("GET /second") {
        return html(SECOND_PAGE);
    }
    if request.starts_with("GET /iframe-host") {
        return html(IFRAME_HOST);
    }
    if request.starts_with("GET /inner") {
        return html(IFRAME_INNER);
    }
    if request.starts_with("GET /network") {
        return html(NETWORK_PAGE);
    }
    if request.starts_with("GET /download-page") {
        return html(DOWNLOAD_PAGE);
    }
    if request.starts_with("GET /api/ok") {
        return FixtureResponse {
            status: "200 OK",
            content_type: "application/json",
            extra_headers: "X-Wardian-Fixture: yes\r\n".to_string(),
            body: r#"{"ok":true}"#.to_string(),
        };
    }
    if request.starts_with("GET /api/missing") {
        return FixtureResponse {
            status: "404 Not Found",
            content_type: "application/json",
            extra_headers: String::new(),
            body: r#"{"error":"nope"}"#.to_string(),
        };
    }
    if request.starts_with("GET /report.csv") {
        return FixtureResponse {
            status: "200 OK",
            content_type: "text/csv",
            extra_headers: "Content-Disposition: attachment; filename=\"report.csv\"\r\n"
                .to_string(),
            body: "quarter,revenue\nQ1,10\n".to_string(),
        };
    }
    html(&fixture_with_long_label())
}

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
                let reply = fixture_response(&request);
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.status,
                    reply.content_type,
                    reply.extra_headers,
                    reply.body.len(),
                    reply.body
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
    session.attach_screencast("pane-1").await.expect("attach");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    session
        .navigate(&format!("{base_url}second"))
        .await
        .expect("navigate must not hang while frames are streaming");
    session
        .wait(&WaitCondition::UrlContains("/second".to_string()), 10_000)
        .await
        .expect("second page");

    session.detach_screencast("pane-1").await.expect("detach");
    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn the_editing_keys_a_surface_forwards_actually_edit() {
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
    let attachment = session.attach_screencast("pane-1").await.expect("attach");
    let lease = Some(attachment.token.as_str());
    session
        .eval("document.getElementById('search').focus()")
        .await
        .expect("focus");

    // Exactly what a surface sends: printable keys carry their text, editing
    // keys carry none and are meaningless to Blink without a virtual key code.
    for (key, code, text) in [
        ("a", "KeyA", Some("a")),
        ("b", "KeyB", Some("b")),
        ("Backspace", "Backspace", None),
    ] {
        session
            .dispatch_key(lease, "keyDown", key, code, text, 0)
            .await
            .expect("key down");
        session
            .dispatch_key(lease, "keyUp", key, code, None, 0)
            .await
            .expect("key up");
    }

    let value = session
        .eval("document.getElementById('search').value")
        .await
        .expect("read the field back");
    assert_eq!(
        value.as_str(),
        Some("a"),
        "Backspace deleted nothing: the key event carried no virtual key code"
    );

    session
        .detach_screencast(&attachment.token)
        .await
        .expect("detach");
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
async fn a_first_load_that_fails_still_leaves_a_usable_session() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    // Nothing is listening on this port, so the first navigation cannot commit.
    let dead_port = {
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = probe.local_addr().expect("addr").port();
        drop(probe);
        port
    };

    let session = broker
        .open(OpenBrowserRequest {
            url: Some(format!("http://127.0.0.1:{dead_port}/")),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("a failed first load must not fail the open");

    // The session has to remain addressable, or its browser would be stranded
    // with no handle to close it.
    assert_eq!(broker.list().await.len(), 1);
    broker
        .resolve("browser:1")
        .await
        .expect("the session must still resolve");

    session.navigate(&base_url).await.expect("recovery navigate");
    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("the session is still usable after a failed first load");

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn closing_a_session_removes_its_profile_directory() {
    let browser_root = std::env::temp_dir().join(format!("wardian-profile-{}", uuid::Uuid::new_v4()));
    let profile_root = browser_root.join("profiles");
    let broker = BrowserSessionBroker::new(browser_root.clone());
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let profile = profile_root.join(session.browser_id());
    assert!(profile.is_dir(), "the session should own a profile directory");

    broker.close(session.browser_id()).await.expect("close");
    assert!(
        !profile.exists(),
        "a closed session must not leave its profile behind at {}",
        profile.display()
    );
    let _ = std::fs::remove_dir_all(&browser_root);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_browser_that_exits_reports_the_session_closed() {
    let broker = broker();
    let mut events = broker.subscribe();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let browser_id = session.browser_id().to_string();

    // Kill the process without going through `close`, which is what a crash
    // looks like. The surface must learn about it rather than waiting forever.
    session.kill_child_for_test().await;

    let closed = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            match events.recv().await {
                Ok(super::actor::BrowserSessionEvent::Closed { browser_id, reason }) => {
                    return (browser_id, reason);
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => panic!("channel closed"),
            }
        }
    })
    .await
    .expect("a dead browser must publish a closed event");

    assert_eq!(closed.0, browser_id);
    assert!(closed.1.contains("exited"), "reason was {:?}", closed.1);

    // Announcing is not enough: a dead session left in the map would keep
    // appearing in `browser list` and would resolve for later commands.
    assert!(
        broker.list().await.is_empty(),
        "a crashed session must not remain listed"
    );
    assert_eq!(
        broker.resolve(&browser_id).await.expect_err("gone").code(),
        "browser_not_found"
    );

    broker.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn concurrent_attaches_serialize_into_one_stream() {
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

    // Split panes and visibility flips produce overlapping attaches. Without
    // serialization one can observe the other's half-built state.
    let attaches = (0..4).map(|index| {
        let session = Arc::clone(&session);
        async move { session.attach_screencast(&format!("pane-{index}")).await }
    });
    let results = futures_util::future::join_all(attaches).await;
    let attachments: Vec<_> = results
        .into_iter()
        .map(|result| result.expect("every attach should succeed"))
        .collect();

    assert_eq!(session.attachment_count().await, 4);
    assert_eq!(
        attachments.iter().filter(|attachment| attachment.can_drive).count(),
        1,
        "exactly one attachment may hold the lease"
    );

    // A frame proves the stream actually started rather than being skipped by
    // an attach that observed a non-empty viewer list.
    let mut events = broker.subscribe();
    let framed = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if let Ok(super::actor::BrowserSessionEvent::Frame { .. }) = events.recv().await {
                return true;
            }
        }
    })
    .await;
    assert!(framed.is_ok(), "no frame arrived after concurrent attaches");

    for attachment in &attachments {
        session.detach_screencast(&attachment.token).await.expect("detach");
    }
    assert_eq!(session.attachment_count().await, 0);

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn an_iframe_routing_itself_leaves_the_outer_page_and_its_refs_alone() {
    let (base_url, server) = serve_fixture().await;
    let host_url = format!("{base_url}iframe-host");
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(host_url.clone()),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::LoadState(LoadState::Complete), 15_000)
        .await
        .expect("load");
    session
        .wait(&WaitCondition::Selector("#inner".to_string()), 15_000)
        .await
        .expect("the iframe should be in the tree");

    let snapshot = session.snapshot(true).await.expect("snapshot");
    let button = snapshot
        .elements
        .iter()
        .find(|element| element.name.trim() == "Go")
        .expect("the outer button should be in the snapshot");

    // The iframe routes itself, then the outer page logs an error. Waiting for
    // the error to land proves the pump has already processed the iframe's
    // navigation, so the assertions below are not racing it.
    session
        .eval(
            "document.getElementById('inner').contentWindow.location.hash = '#routed'; console.error('settled'); 1",
        )
        .await
        .expect("eval");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if session.summary().await.console_error_count > 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the console error should reach the session"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // An iframe routing itself is not a top-level navigation.
    assert_eq!(
        session.summary().await.url,
        host_url,
        "an iframe's own routing must not rewrite the session URL"
    );
    session
        .act(&button.element_ref, &ElementAction::Click)
        .await
        .expect("a ref taken before an iframe routed itself must still be valid");

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_surface_open_stays_outstanding_until_it_is_acknowledged() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let summary = session.summary().await;
    broker.queue_surface_open(summary.clone()).await;

    let pending = broker.pending_surface_opens().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].browser_id, summary.browser_id);
    // A frontend that reads and then dies before opening anything must not
    // have taken the work with it.
    assert_eq!(
        broker.pending_surface_opens().await.len(),
        1,
        "reading must not consume an open that was never acknowledged"
    );

    broker.ack_surface_open(&summary.browser_id).await;
    assert!(
        broker.pending_surface_opens().await.is_empty(),
        "an acknowledged open must not be handed out again"
    );

    broker.close(&summary.browser_id).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn queueing_the_same_open_twice_does_not_duplicate_it() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let summary = session.summary().await;

    broker.queue_surface_open(summary.clone()).await;
    broker.queue_surface_open(summary.clone()).await;
    assert_eq!(broker.pending_surface_opens().await.len(), 1);

    broker.close(&summary.browser_id).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_queued_open_for_a_closed_session_is_dropped() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let summary = session.summary().await;
    broker.queue_surface_open(summary.clone()).await;
    broker.close(&summary.browser_id).await.expect("close");

    // Reading must not resurrect a surface for a browser that is gone.
    assert!(broker.pending_surface_opens().await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn closing_a_session_twice_announces_exactly_once() {
    let broker = broker();
    let mut events = broker.subscribe();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let browser_id = session.browser_id().to_string();

    broker.close(&browser_id).await.expect("first close");
    // A crash racing an explicit close takes the same path; only whoever
    // removed the session from the broker may announce it.
    assert_eq!(
        broker.close(&browser_id).await.expect_err("already gone").code(),
        "browser_not_found"
    );

    let mut closures = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, super::actor::BrowserSessionEvent::Closed { .. }) {
            closures += 1;
        }
    }
    assert_eq!(closures, 1, "a session must report its closure exactly once");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn only_the_first_attachment_may_drive_the_page() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");

    let driver = session.attach_screencast("pane-a").await.expect("first");
    let mirror = session.attach_screencast("pane-b").await.expect("second");
    assert!(driver.can_drive);
    assert!(!mirror.can_drive, "a mirroring pane must not drive the shared page");
    assert_ne!(driver.token, mirror.token, "each attachment needs its own credential");

    let event = PointerEvent {
        event_type: "mousePressed",
        x: 1.0,
        y: 1.0,
        button: "left",
        click_count: 1,
        modifiers: 0,
    };
    session
        .dispatch_mouse(Some(&driver.token), &event)
        .await
        .expect("the lease holder drives");
    let refused = session
        .dispatch_mouse(Some(&mirror.token), &event)
        .await
        .expect_err("a mirror must not dispatch input");
    assert_eq!(refused.code(), "browser_read_only_presentation");

    // A guessed or invented token is not a bypass.
    let forged = session
        .dispatch_mouse(Some("pane-a"), &event)
        .await
        .expect_err("a presentation id is not a credential");
    assert_eq!(forged.code(), "browser_read_only_presentation");

    // The control plane carries no token and is not a competing presentation.
    session
        .dispatch_mouse(None, &event)
        .await
        .expect("the CLI path is not gated by the surface lease");

    // When the driver leaves, the lease passes rather than stranding the page.
    session.detach_screencast(&driver.token).await.expect("detach");
    assert!(session.token_may_drive(&mirror.token).await);

    session.detach_screencast(&mirror.token).await.expect("detach");
    assert_eq!(session.attachment_count().await, 0);
    broker.close(session.browser_id()).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn navigation_and_viewport_also_require_the_lease() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let driver = session.attach_screencast("pane-a").await.expect("first");
    let mirror = session.attach_screencast("pane-b").await.expect("second");

    // These are page mutations too; guarding only pointer input would leave
    // the chrome bar as an unguarded path to the same shared page.
    assert_eq!(
        session
            .require_drive(Some(&mirror.token))
            .await
            .expect_err("a mirror must not mutate")
            .code(),
        "browser_read_only_presentation"
    );
    session
        .require_drive(Some(&driver.token))
        .await
        .expect("the holder may mutate");

    broker.close(session.browser_id()).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn re_attaching_the_same_presentation_supersedes_the_older_attachment() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");

    // A hidden/shown race, or a reloaded webview, produces a second attach for
    // one presentation. The newer one replaces the older and inherits the
    // lease: leaving the older registered is what leaves a live surface
    // mirroring a lease nobody will ever release.
    let first = session.attach_screencast("pane-a").await.expect("first");
    let second = session.attach_screencast("pane-a").await.expect("second");
    assert_eq!(
        session.attachment_count().await,
        1,
        "one presentation streams once"
    );
    assert!(
        second.can_drive,
        "a replacement attachment must inherit the lease it replaced"
    );
    assert!(!session.token_may_drive(&first.token).await);

    session.detach_screencast(&first.token).await.expect("stale detach");
    assert_eq!(
        session.attachment_count().await,
        1,
        "the newer attachment must survive a stale cleanup"
    );
    assert!(
        session.token_may_drive(&second.token).await,
        "the lease must stay with the surviving attachment"
    );

    session.detach_screencast(&second.token).await.expect("detach");
    broker.close(session.browser_id()).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn the_lease_moves_to_the_remaining_presentation_when_the_driver_leaves() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let mut events = broker.subscribe();

    let driver = session.attach_screencast("pane-a").await.expect("first");
    let mirror = session.attach_screencast("pane-b").await.expect("second");
    assert!(driver.can_drive);
    assert!(!mirror.can_drive);

    session.detach_screencast(&driver.token).await.expect("detach");
    assert!(
        session.token_may_drive(&mirror.token).await,
        "the mirror inherits the lease once the driver leaves"
    );

    // The mirror learned it could not drive at attach time, so the handover
    // has to reach it as an event or its controls stay disabled forever.
    let announced = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(super::actor::BrowserSessionEvent::Lease {
                presentation_id: Some(presentation_id),
                ..
            }) = events.recv().await
            {
                if presentation_id == "pane-b" {
                    return true;
                }
            }
        }
    })
    .await;
    assert!(announced.is_ok(), "the handover was never announced");

    session.detach_screencast(&mirror.token).await.expect("detach");
    broker.close(session.browser_id()).await.expect("close");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_same_document_navigation_invalidates_refs_and_updates_the_url() {
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

    // A History API route change never commits a frame, so it would otherwise
    // leave both the URL and the refs pointing at the previous route.
    session
        .eval("history.pushState({}, '', '/routed'); true")
        .await
        .expect("pushState");
    session
        .wait(&WaitCondition::UrlContains("/routed".to_string()), 10_000)
        .await
        .expect("route change");

    let summary = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let summary = session.summary().await;
            if summary.url.contains("/routed") {
                return summary;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the session URL must follow a same-document navigation");
    assert!(summary.url.ends_with("/routed"), "url was {}", summary.url);

    let error = session
        .act(&stale_ref, &ElementAction::Click)
        .await
        .expect_err("refs from the previous route must not survive it");
    assert_eq!(error.code(), "snapshot_stale", "got {error}");

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_ref_whose_element_was_repurposed_is_refused() {
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
    let button = snapshot
        .elements
        .iter()
        .find(|element| element.name.trim() == "Go")
        .expect("the button");

    // Recycle the node in place, as a virtualized list does: same element,
    // same stamped ref, entirely different meaning. No navigation occurs, so
    // the generation check alone would let this through.
    session
        .eval("document.getElementById('go').textContent = 'Delete everything'; true")
        .await
        .expect("repurpose");

    let error = session
        .act(&button.element_ref, &ElementAction::Click)
        .await
        .expect_err("a repurposed element must not be clicked as the original");
    assert_eq!(error.code(), "ref_changed", "got {error}");

    broker.close(session.browser_id()).await.expect("close");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn an_element_with_a_clamped_name_is_still_actionable() {
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
    let verbose = snapshot
        .elements
        .iter()
        .find(|element| element.name.starts_with("Download the quarterly report"))
        .expect("the long-labelled button should be in the snapshot");
    assert!(
        verbose.name.ends_with('…'),
        "the fixture must actually exceed the field cap: {:?}",
        verbose.name
    );

    // The ledger holds the clamped name. A guard that compared the raw one
    // would refuse this element as `ref_changed` even though nothing changed.
    session
        .act(&verbose.element_ref, &ElementAction::Click)
        .await
        .expect("a clamped name must not refuse its own element");

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

// ---------------------------------------------------------------------------
// Phase 3: introspection
// ---------------------------------------------------------------------------

/// Blocks until the ledger satisfies `predicate`, or fails saying what it held.
///
/// The ledger is fed by the event pump, so it is eventually consistent with the
/// page: a `wait` on page text returns the moment the DOM changes, while the
/// `Network.responseReceived` that gives a record its status may still be in
/// the queue behind it. Settling on the ledger rather than on the page is what
/// keeps these tests from passing against a ledger that never fills in.
async fn await_ledger(
    session: &Arc<super::actor::BrowserSession>,
    what: &str,
    predicate: impl Fn(&[wardian_core::browser::NetworkEntry]) -> bool,
) -> Vec<wardian_core::browser::NetworkEntry> {
    for _ in 0..150 {
        let entries = session.network(&NetworkFilter::default()).await;
        if predicate(&entries) {
            return entries;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let entries = session.network(&NetworkFilter::default()).await;
    panic!("timed out waiting for {what}; the ledger held {entries:#?}");
}

/// Opens the network fixture and blocks until both of its fetches are recorded.
async fn network_session(
    broker: &BrowserSessionBroker,
    base_url: &str,
) -> Arc<super::actor::BrowserSession> {
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(format!("{base_url}network")),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::Text("settled".to_string()), 15_000)
        .await
        .expect("both fetches settle");
    await_ledger(&session, "both fetches to reach the ledger", |entries| {
        entries
            .iter()
            .any(|entry| entry.url.ends_with("/api/ok") && entry.status.is_some())
            && entries
                .iter()
                .any(|entry| entry.url.ends_with("/api/missing") && entry.status.is_some())
            && entries
                .iter()
                .any(|entry| entry.url.ends_with("/network") && entry.status.is_some())
    })
    .await;
    session
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn the_ledger_records_the_document_and_everything_the_page_fetched() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = network_session(&broker, &base_url).await;

    let entries = session.network(&NetworkFilter::default()).await;
    let document = entries
        .iter()
        .find(|entry| entry.url.ends_with("/network"))
        .expect("the document request is recorded");
    assert_eq!(document.status, Some(200));
    assert_eq!(document.resource_type, "document");

    let ok = entries
        .iter()
        .find(|entry| entry.url.ends_with("/api/ok"))
        .expect("the successful fetch is recorded");
    assert_eq!(ok.status, Some(200));
    assert_eq!(ok.mime_type.as_deref(), Some("application/json"));

    let missing = entries
        .iter()
        .find(|entry| entry.url.ends_with("/api/missing"))
        .expect("the 404 is recorded");
    assert_eq!(missing.status, Some(404));

    assert_eq!(
        session.summary().await.network_failure_count,
        1,
        "only the 404 counts as a failure"
    );

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_failed_request_announces_itself_to_the_surface() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let mut events = broker.subscribe();
    let session = network_session(&broker, &base_url).await;

    // A failure arrives with no navigation and no load event behind it, so the
    // ledger has to announce the change itself or the surface keeps showing a
    // count from before the request was even made.
    let announced = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            match events.recv().await {
                Ok(super::actor::BrowserSessionEvent::State { summary, .. })
                    if summary.browser_id == session.browser_id()
                        && summary.network_failure_count > 0 =>
                {
                    return summary;
                }
                Ok(_) => continue,
                Err(error) => panic!("the event stream ended: {error}"),
            }
        }
    })
    .await
    .expect("a state event carrying the new failure count");
    assert_eq!(announced.network_failure_count, 1);

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_filter_narrows_the_ledger_to_what_was_asked_for() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = network_session(&broker, &base_url).await;

    let failed = session
        .network(&NetworkFilter {
            failed_only: true,
            ..NetworkFilter::default()
        })
        .await;
    assert_eq!(failed.len(), 1);
    assert!(failed[0].url.ends_with("/api/missing"));

    let by_status = session
        .network(&NetworkFilter {
            status: StatusFilter::parse("2xx"),
            ..NetworkFilter::default()
        })
        .await;
    assert!(by_status.iter().all(|entry| entry.status == Some(200)));
    assert!(!by_status.is_empty());

    let by_text = session
        .network(&NetworkFilter {
            text: Some("/API/".to_string()),
            ..NetworkFilter::default()
        })
        .await;
    assert_eq!(by_text.len(), 2, "matching a URL ignores case");

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_request_detail_carries_its_headers_and_can_read_its_body_back() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = network_session(&broker, &base_url).await;

    let request_id = session
        .network(&NetworkFilter {
            text: Some("/api/ok".to_string()),
            ..NetworkFilter::default()
        })
        .await
        .first()
        .expect("the fetch is recorded")
        .request_id
        .clone();

    let detail = session
        .network_detail(&request_id, true)
        .await
        .expect("detail");
    assert_eq!(detail.entry.status, Some(200));
    assert_eq!(
        detail.response_headers.get("x-wardian-fixture").map(String::as_str),
        Some("yes"),
        "header names are lowercased so a caller need not guess the casing"
    );
    assert!(!detail.request_headers.is_empty());
    let body = detail.body.expect("body");
    assert!(body.text.contains("\"ok\":true"));
    assert!(!body.truncated);
    assert!(detail.body_error.is_none());

    // Without `--body` nothing is read back at all.
    let headers_only = session
        .network_detail(&request_id, false)
        .await
        .expect("detail");
    assert!(headers_only.body.is_none());
    assert!(headers_only.body_error.is_none());

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn an_unrecorded_request_id_is_refused_rather_than_answered_with_nothing() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = network_session(&broker, &base_url).await;

    let error = session
        .network_detail("not-a-request", true)
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "browser_invalid_request");
    assert!(error.to_string().contains("not-a-request"));

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn the_ledger_survives_the_navigation_that_clears_the_console() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = network_session(&broker, &base_url).await;
    let before = session.network(&NetworkFilter::default()).await.len();
    assert!(before > 0);

    session
        .navigate(&format!("{base_url}second"))
        .await
        .expect("navigate");
    session
        .wait(&WaitCondition::Text("arrived".to_string()), 15_000)
        .await
        .expect("second page");

    let after = await_ledger(&session, "the navigation to be recorded", |entries| {
        entries.iter().any(|entry| entry.url.ends_with("/second"))
    })
    .await;
    assert!(
        after.len() > before,
        "the earlier requests stay and the navigation adds its own"
    );
    assert!(
        after.iter().any(|entry| entry.url.ends_with("/api/missing")),
        "a navigation must not discard the record an agent is investigating"
    );
    assert!(after.iter().any(|entry| entry.url.ends_with("/second")));

    session.clear_network().await;
    assert!(session.network(&NetworkFilter::default()).await.is_empty());
    assert_eq!(session.summary().await.network_failure_count, 0);
    // Clearing the ledger is not a page operation.
    assert_eq!(
        session.get(PageField::Title, None).await.expect("title"),
        "Second"
    );

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn cookies_round_trip_through_the_sessions_own_profile() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(base_url.clone()),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");

    assert!(
        session
            .cookies(&CookieAction::List { all: false })
            .await
            .expect("list")
            .is_empty(),
        "a fresh profile inherits nothing from the human's browser"
    );

    session
        .cookies(&CookieAction::Set {
            name: "sid".to_string(),
            value: "abc".to_string(),
            url: None,
            domain: None,
            path: None,
            secure: false,
            http_only: true,
            same_site: Some("lax".to_string()),
            expires: None,
        })
        .await
        .expect("set");

    let cookies = session
        .cookies(&CookieAction::List { all: false })
        .await
        .expect("list");
    let sid = cookies
        .iter()
        .find(|cookie| cookie.name == "sid")
        .expect("the cookie the page can see");
    assert_eq!(sid.value, "abc");
    assert!(sid.http_only);
    assert_eq!(sid.same_site.as_deref(), Some("Lax"));
    assert_eq!(sid.expires, None, "no expiry means a session cookie");

    session
        .cookies(&CookieAction::Delete {
            name: "sid".to_string(),
            url: None,
            domain: None,
            path: None,
        })
        .await
        .expect("delete");
    assert!(session
        .cookies(&CookieAction::List { all: false })
        .await
        .expect("list")
        .iter()
        .all(|cookie| cookie.name != "sid"));

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_cookie_needs_somewhere_to_live_and_says_so_on_a_blank_page() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");

    let error = session
        .cookies(&CookieAction::Set {
            name: "sid".to_string(),
            value: "abc".to_string(),
            url: None,
            domain: None,
            path: None,
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        })
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "browser_invalid_request");
    assert!(error.to_string().contains("--url or --domain"));

    broker.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn an_unusable_same_site_is_refused_before_it_reaches_the_browser() {
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
        .cookies(&CookieAction::Set {
            name: "sid".to_string(),
            value: "abc".to_string(),
            url: None,
            domain: None,
            path: None,
            secure: false,
            http_only: false,
            same_site: Some("sometimes".to_string()),
            expires: None,
        })
        .await
        .expect_err("refused");
    assert!(error.to_string().contains("strict, lax, or none"));

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn the_two_storage_areas_round_trip_and_stay_separate() {
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
        .storage_mutate(
            StorageArea::Local,
            &StorageAction::Set {
                key: "theme".to_string(),
                value: "dark".to_string(),
            },
        )
        .await
        .expect("set local");
    session
        .storage_mutate(
            StorageArea::Session,
            &StorageAction::Set {
                key: "token".to_string(),
                value: "abc".to_string(),
            },
        )
        .await
        .expect("set session");

    assert_eq!(
        session
            .storage_get(StorageArea::Local, "theme")
            .await
            .expect("get"),
        Some("dark".to_string())
    );
    assert_eq!(
        session
            .storage_get(StorageArea::Local, "token")
            .await
            .expect("get"),
        None,
        "the two areas do not see each other"
    );

    let local = session.storage(StorageArea::Local).await.expect("list");
    assert_eq!(local.entries.len(), 1);
    assert_eq!(local.entries[0].key, "theme");
    assert!(local.origin.starts_with("http://127.0.0.1"));
    assert!(!local.truncated);

    session
        .storage_mutate(
            StorageArea::Local,
            &StorageAction::Remove {
                key: "theme".to_string(),
            },
        )
        .await
        .expect("remove");
    assert!(session
        .storage(StorageArea::Local)
        .await
        .expect("list")
        .entries
        .is_empty());

    session
        .storage_mutate(StorageArea::Session, &StorageAction::Clear)
        .await
        .expect("clear");
    assert!(session
        .storage(StorageArea::Session)
        .await
        .expect("list")
        .entries
        .is_empty());

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn storage_on_an_opaque_origin_names_the_fix() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");

    let error = session
        .storage(StorageArea::Local)
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "browser_invalid_request");
    assert!(
        error.to_string().contains("http or https"),
        "got {error}"
    );

    broker.shutdown_all().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn an_oversized_storage_value_comes_back_cut_and_flagged() {
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
        .storage_mutate(
            StorageArea::Local,
            &StorageAction::Set {
                key: "blob".to_string(),
                value: "v".repeat(MAX_STORAGE_VALUE_CHARS + 500),
            },
        )
        .await
        .expect("set");

    let listing = session.storage(StorageArea::Local).await.expect("list");
    assert!(listing.entries[0].truncated);
    assert_eq!(
        listing.entries[0].value.chars().count(),
        MAX_STORAGE_VALUE_CHARS
    );

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn a_download_lands_under_its_suggested_name_and_outlives_the_session() {
    let (base_url, server) = serve_fixture().await;
    let broker = broker();
    let session = broker
        .open(OpenBrowserRequest {
            url: Some(format!("{base_url}download-page")),
            ..OpenBrowserRequest::default()
        })
        .await
        .expect("open");
    session
        .wait(&WaitCondition::Selector("#grab".to_string()), 15_000)
        .await
        .expect("page");

    let snapshot = session.snapshot(true).await.expect("snapshot");
    let link = snapshot
        .elements
        .iter()
        .find(|element| element.name.contains("Download"))
        .expect("the download link");
    session
        .act(&link.element_ref, &ElementAction::Click)
        .await
        .expect("click");

    // The browser writes the file asynchronously, so this settles on the
    // recorded state rather than assuming the click was enough.
    let mut settled = None;
    for _ in 0..100 {
        let downloads = session.downloads().await;
        if let Some(record) = downloads
            .iter()
            .find(|record| record.state == "completed" && record.path.is_some())
        {
            settled = Some(record.clone());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let record = settled.expect("the download completes");
    assert_eq!(record.suggested_filename, "report.csv");
    let path = PathBuf::from(record.path.expect("a resolved path"));
    assert_eq!(
        path.file_name().map(|name| name.to_string_lossy().to_string()),
        Some("report.csv".to_string()),
        "a GUID is no use to a caller"
    );
    assert!(path.exists());
    assert!(std::fs::read_to_string(&path)
        .expect("read")
        .contains("quarter,revenue"));

    let profile_dir = path
        .parent()
        .and_then(|dir| dir.parent())
        .expect("the downloads root");
    assert!(
        !profile_dir.ends_with("profiles"),
        "downloads must not live inside a profile, which close deletes"
    );

    broker.close(session.browser_id()).await.expect("close");
    assert!(
        path.exists(),
        "closing the session takes the profile, never the downloaded file"
    );

    broker.shutdown_all().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a Chromium-based browser on the host"]
async fn the_console_can_be_filtered_and_emptied() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");

    session
        .eval("console.error('boom'); console.log('chatter'); true")
        .await
        .expect("eval");
    // Console entries arrive on the event pump, so this settles on the count.
    let mut entries = Vec::new();
    for _ in 0..100 {
        entries = session.console(None, false).await;
        if entries.len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(entries.len() >= 2, "got {entries:?}");

    let errors = session.console(Some("error"), false).await;
    assert!(errors.iter().all(|entry| entry.level == "error"));
    assert!(errors.iter().any(|entry| entry.text.contains("boom")));
    assert_eq!(
        session.summary().await.console_error_count,
        1,
        "a filtered read does not consume"
    );

    let drained = session.console(None, true).await;
    assert!(!drained.is_empty());
    assert!(session.console(None, false).await.is_empty());
    assert_eq!(session.summary().await.console_error_count, 0);

    broker.shutdown_all().await;
}
