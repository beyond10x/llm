//! Integrity conformance check: the transport carries caller-supplied credential headers to the
//! caller-selected endpoint only. Ambient process proxy configuration must not redirect them.
//!
//! The parent test re-executes this binary with `HTTP_PROXY` naming a local listener, so the
//! environment change never leaks into another test in this process.
use llm_core::Cancel;
use llm_http::{Framing, HeaderMap, HeaderValue, HttpClient, Limits};
use std::{io::Read, net::TcpListener, process::Command, sync::mpsc, thread, time::Duration};

const CHILD: &str = "LLM_HTTP_AMBIENT_PROXY_CHILD";
const CREDENTIAL: &str = "probe-credential-value";

#[test]
fn ambient_proxy_environment_does_not_receive_caller_credentials() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(false).unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let (seen_tx, seen_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Ok((mut socket, _)) = proxy.accept() {
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = vec![0; 8192];
            let length = socket.read(&mut bytes).unwrap_or(0);
            bytes.truncate(length);
            seen_tx.send(bytes).ok();
        }
    });
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_sends_one_authenticated_request",
            "--include-ignored",
        ])
        .env(CHILD, "1")
        .env("HTTP_PROXY", &proxy_url)
        .env("http_proxy", &proxy_url)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .status()
        .unwrap();
    assert!(status.success(), "child fixture failed to run");
    if let Ok(bytes) = seen_rx.recv_timeout(Duration::from_secs(3)) {
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        panic!(
            "ambient HTTP_PROXY received the attempt (credential forwarded: {}); first line: {:?}",
            text.contains(CREDENTIAL),
            text.lines().next().unwrap_or_default()
        );
    }
}

#[test]
#[ignore = "subprocess fixture entry point; run by the parent test with a proxy environment"]
fn child_sends_one_authenticated_request() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        // The caller-selected endpoint: a listener that accepts and stays silent.
        let origin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/stream", origin.local_addr().unwrap());
        let _accept = tokio::spawn(async move {
            let _held = origin.accept().await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let client = HttpClient::new(Limits {
            response_headers: Duration::from_millis(500),
            ..Limits::default()
        })
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {CREDENTIAL}")).unwrap(),
        );
        let _ = client
            .post_sse(
                &url,
                headers,
                b"{}".to_vec(),
                Framing::PayloadsOnly,
                &Cancel::new(),
            )
            .await;
    });
}
