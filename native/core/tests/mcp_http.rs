#![cfg(unix)]
#[path = "fixtures/http.rs"]
mod http_peer;
use http_peer::Fixture;
use serde_json::json;
use shadowcode_core::mcp::Client;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn modern_http_negotiates_metadata_and_headers_without_sessions_and_preserves_sse_utf8() {
    let f = Fixture::new("modern").await;
    let mut client = f.connect(CancellationToken::new()).await.unwrap();
    assert_eq!(client.pid(), None);
    assert_eq!(
        client.tools().len(),
        3,
        "Invalid header annotation must be excluded"
    );
    let value = client
        .call("雪", json!({"region":"東","action":"sse"}))
        .await
        .unwrap();
    assert_eq!(value["content"][0]["text"], "HTTP fixture 雪");
    assert_eq!(
        value["structuredContent"]["headers"]["mcp-name"],
        "=?base64?6Zuq?="
    );
    assert_eq!(
        value["structuredContent"]["headers"]["mcp-param-region"],
        "=?base64?5p2x?="
    );
    let failure = client.call("failure", json!({})).await.unwrap();
    assert_eq!(failure["isError"], true);
    client.close().await.unwrap();
    f.closed_streams().await;
    let requests = f.requests();
    assert_eq!(requests[0]["message"]["method"], "server/discover");
    assert!(requests.iter().all(|r| r["method"] == "POST"
        && r["message"]["method"] != "initialize"
        && r["headers"]["mcp-session-id"].is_null()));
    let call = requests
        .iter()
        .find(|r| r["message"]["method"] == "tools/call")
        .unwrap();
    assert_eq!(
        call["message"]["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
        "2026-07-28"
    );
    assert_eq!(call["headers"]["mcp-protocol-version"], "2026-07-28");
    assert_eq!(call["headers"]["mcp-method"], "tools/call");
}
#[tokio::test]
async fn legacy_http_negotiates_session_and_deletes_it_without_repeating_expired_tool_calls() {
    for mode in ["legacy", "legacy-no-sse"] {
        let f = Fixture::new(mode).await;
        let mut client = f.connect(CancellationToken::new()).await.unwrap();
        assert_eq!(
            client.call("echo", json!({})).await.unwrap()["isError"],
            false
        );
        assert!(client
            .call("echo", json!({"action":"expired"}))
            .await
            .is_err());
        assert!(client.is_closed());
        client.close().await.unwrap();
        f.closed_streams().await;
        let requests = f.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["message"]["method"] == "initialize")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r["message"]["method"] == "tools/call")
                .count(),
            2
        );
        assert_eq!(
            requests.iter().filter(|r| r["method"] == "DELETE").count(),
            1
        );
        for request in requests
            .iter()
            .filter(|r| r["message"]["method"] == "tools/call")
        {
            assert_eq!(request["headers"]["mcp-session-id"], "fixture-session");
        }
    }
}
#[tokio::test]
async fn http_credentials_are_explicit_and_never_redirected_or_exposed_in_errors() {
    let mut f = Fixture::new("auth").await;
    let error = f.connect(CancellationToken::new()).await.err().unwrap();
    assert!(!format!("{error:#}").contains("private-"));
    assert_eq!(
        f.requests().len(),
        1,
        "Authentication must not trigger initialize fallback"
    );
    f.spec.bearer_token = Some("http-private-fixture-key".into());
    let mut client = f.connect(CancellationToken::new()).await.unwrap();
    assert!(client
        .call("echo", json!({"action":"redirect"}))
        .await
        .is_err());
    assert!(f.requests().iter().all(|r| r["path"] != "/stolen"));
    client.close().await.unwrap();
    let redirect = Fixture::new("redirect").await;
    assert!(redirect.connect(CancellationToken::new()).await.is_err());
    assert_eq!(redirect.requests().len(), 1);
}
#[tokio::test]
async fn http_rejects_oversized_malformed_truncated_and_flooded_peer_responses() {
    for mode in [
        "init_json_large",
        "init_malformed",
        "init_truncated",
        "catalog_error",
    ] {
        let f = Fixture::new(mode).await;
        let error = f.connect(CancellationToken::new()).await.err().expect(mode);
        assert!(!format!("{error:#}").contains("private-"));
        f.closed_streams().await;
    }
    for action in [
        "json_large",
        "chunked_large",
        "sse_large",
        "sse_invalid",
        "sse_flood",
        "sse_incomplete",
    ] {
        let f = Fixture::new("modern").await;
        let mut client = f.connect(CancellationToken::new()).await.unwrap();
        let error = client
            .call("echo", json!({"action":action}))
            .await
            .expect_err(action);
        assert!(!format!("{error:#}").contains("private-"));
        assert!(client.is_closed());
        f.closed_streams().await;
    }
}
#[tokio::test]
async fn http_initialization_and_calls_cancel_close_streams_and_never_resubmit() {
    let mut f = Fixture::new("init_hang").await;
    f.spec.timeout = Duration::from_millis(150);
    assert!(f.connect(CancellationToken::new()).await.is_err());
    f.closed_streams().await;
    let f = Fixture::new("init_hang").await;
    let spec = f.spec.clone();
    let connecting =
        tokio::spawn(async move { Client::connect_http(&spec, CancellationToken::new()).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while f.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    connecting.abort();
    assert!(connecting.await.is_err_and(|error| error.is_cancelled()));
    f.closed_streams().await;
    for cancelled in [false, true] {
        let mut f = Fixture::new("modern").await;
        f.spec.timeout = Duration::from_millis(200);
        let cancel = CancellationToken::new();
        let mut client = f.connect(cancel.clone()).await.unwrap();
        if cancelled {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                cancel.cancel();
            });
        }
        assert!(client.call("echo", json!({"action":"hang"})).await.is_err());
        client.close().await.unwrap();
        f.closed_streams().await;
        assert_eq!(
            f.requests()
                .iter()
                .filter(|r| r["message"]["method"] == "tools/call")
                .count(),
            1
        );
    }
    let f = Fixture::new("legacy").await;
    let client = f.connect(CancellationToken::new()).await.unwrap();
    drop(client);
    f.closed_streams().await;
}
#[tokio::test]
async fn invalid_http_specs_and_precancelled_connections_send_nothing() {
    let f = Fixture::new("modern").await;
    for url in [
        "file:///etc/passwd",
        "http://user:password@127.0.0.1/mcp",
        "https://example.com/mcp#fragment",
    ] {
        let mut spec = f.spec.clone();
        spec.url = url.into();
        assert!(Client::connect_http(&spec, CancellationToken::new())
            .await
            .is_err());
    }
    let mut spec = f.spec.clone();
    spec.url = "http://example.com/mcp".into();
    spec.bearer_token = Some("secret".into());
    assert!(Client::connect_http(&spec, CancellationToken::new())
        .await
        .is_err());
    spec = f.spec.clone();
    spec.bearer_token = Some("injected\r\nHeader:value".into());
    assert!(Client::connect_http(&spec, CancellationToken::new())
        .await
        .is_err());
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(f.connect(cancel).await.is_err());
    assert!(f.requests().is_empty());
}
