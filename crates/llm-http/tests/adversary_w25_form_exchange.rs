//! Adversary case for the form exchange (wave 2026-11-04-w25, pass 1): whether the unit's
//! ambient-proxy test could fail at all. Its child process sends to a loopback endpoint with
//! every proxy variable set; if a client that honours those variables still goes straight to a
//! loopback address, that test stays green with `.no_proxy()` removed.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

const CHILD: &str = "LLM_HTTP_ADVERSARY_W25_PROXY_CHILD";
const MARKER: &str = "adversary-w25-proxy-probe-marker";

/// A client built without `.no_proxy()`, run with every proxy variable naming a local listener
/// and no `NO_PROXY`, sends its loopback request to that proxy. This is the mutant the unit's
/// `the_token_never_reaches_an_ambient_proxy` must be able to see.
#[test]
fn a_client_that_honours_proxy_variables_sends_a_loopback_request_to_the_proxy() {
    if let Some(target) = std::env::var_os(CHILD) {
        let target = target.into_string().expect("a UTF-8 target");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(3))
                .build()
                .expect("a client");
            let _ = client.post(&target).body(MARKER).send().await;
        });
        return;
    }
    let endpoint = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let target = format!("http://{}/", endpoint.local_addr().expect("its address"));
    std::thread::spawn(move || {
        while let Ok((mut socket, _)) = endpoint.accept() {
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
            let mut bytes = [0_u8; 4096];
            let _ = socket.read(&mut bytes);
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        }
    });
    let proxy = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let proxy_url = format!("http://{}", proxy.local_addr().expect("its address"));
    let (seen, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok((mut socket, _)) = proxy.accept() {
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
            let mut bytes = vec![0; 64 * 1024];
            let length = socket.read(&mut bytes).unwrap_or(0);
            bytes.truncate(length);
            if seen.send(bytes).is_err() {
                break;
            }
        }
    });
    let mut child = std::process::Command::new(std::env::current_exe().expect("this binary"));
    child
        .args([
            "--exact",
            "a_client_that_honours_proxy_variables_sends_a_loopback_request_to_the_proxy",
        ])
        .env(CHILD, &target)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .env_remove("REQUEST_METHOD");
    for variable in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        child.env(variable, &proxy_url);
    }
    let status = child.status().expect("the child runs");
    assert!(status.success(), "the child failed: {status}");
    let mut proxied = false;
    while let Ok(bytes) = heard.recv_timeout(Duration::from_millis(500)) {
        proxied |= String::from_utf8_lossy(&bytes).contains(&target);
    }
    assert!(
        proxied,
        "a client honouring HTTP_PROXY went straight to {target}: a proxy test on a loopback \
         endpoint cannot observe a missing `.no_proxy()`"
    );
}
