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
                let long_label = fixture_with_long_label();
                let body: &str = if request.starts_with("GET /second") {
                    SECOND_PAGE
                } else {
                    &long_label
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
    let profile_root = std::env::temp_dir().join(format!("wardian-profile-{}", uuid::Uuid::new_v4()));
    let broker = BrowserSessionBroker::new(profile_root.clone());
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let profile = profile_root.join(session.browser_id());
    assert!(profile.is_dir(), "the session should own a profile directory");

    broker.close(session.browser_id()).await.expect("close");
    assert!(
        !profile.exists(),
        "a closed session must not leave its profile behind at {}",
        profile.display()
    );
    let _ = std::fs::remove_dir_all(&profile_root);
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
async fn a_surface_open_queued_before_the_frontend_listens_is_drained_once() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");
    let summary = session.summary().await;
    broker.queue_surface_open(summary.clone()).await;

    let drained = broker.take_pending_surface_opens().await;
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].browser_id, summary.browser_id);
    assert!(
        broker.take_pending_surface_opens().await.is_empty(),
        "draining must not replay the same request"
    );

    // Once the listener exists the event is delivery enough. Continuing to
    // queue would grow the list for the app's lifetime under normal use.
    broker.queue_surface_open(summary.clone()).await;
    assert!(
        broker.take_pending_surface_opens().await.is_empty(),
        "opens after the listener is ready must not be queued"
    );

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

    // Draining must not resurrect a surface for a browser that is gone.
    assert!(broker.take_pending_surface_opens().await.is_empty());
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
async fn re_attaching_the_same_presentation_does_not_release_the_newer_attachment() {
    let broker = broker();
    let session = broker.open(OpenBrowserRequest::default()).await.expect("open");

    // A hidden/shown race produces two attachments for one presentation. The
    // stale cleanup must release only its own.
    let first = session.attach_screencast("pane-a").await.expect("first");
    let second = session.attach_screencast("pane-a").await.expect("second");
    assert_eq!(session.attachment_count().await, 2);

    session.detach_screencast(&first.token).await.expect("stale detach");
    assert_eq!(
        session.attachment_count().await,
        1,
        "the newer attachment must survive a stale cleanup"
    );
    assert!(
        session.token_may_drive(&second.token).await,
        "the lease must pass to the surviving attachment"
    );

    session.detach_screencast(&second.token).await.expect("detach");
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
