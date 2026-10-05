//! Harness parity H21 (`harness-http/src/transport.rs:54`-`87`, `Settings::streaming`, test
//! `:696`): a connect bound of its own, and an idle limit long enough for a model that thinks
//! silently between two events. Local sockets only.
use llm_core::{Cancel, Dispatch, ErrorCode};
use llm_http::{CONNECT_TIMEOUT, Framing, HeaderMap, HttpClient, Limits};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpSocket, TcpStream};

/// A listener whose accept queue is full and which never accepts: the kernel drops every further
/// connection attempt, so a connection neither completes nor is refused until the client gives up.
///
/// The queue is filled by connecting until one attempt stops completing; the fixture asserts it
/// got there, so a kernel that answered instead cannot turn this into a refusal test.
async fn never_established() -> (TcpListener, Vec<TcpStream>, String) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(0).unwrap();
    let address = listener.local_addr().unwrap();
    let mut fillers = Vec::new();
    let mut full = false;
    for _ in 0..64 {
        match tokio::time::timeout(Duration::from_millis(200), TcpStream::connect(address)).await {
            Ok(Ok(stream)) => fillers.push(stream),
            Ok(Err(error)) => panic!("the fixture listener refused a connection: {error}"),
            Err(_) => {
                full = true;
                break;
            }
        }
    }
    assert!(full, "the accept queue never filled");
    (listener, fillers, format!("http://{address}/v1"))
}

fn patient() -> Limits {
    Limits {
        response_headers: Duration::from_secs(10),
        idle: Duration::from_secs(10),
        total: Duration::from_secs(10),
    }
}

/// The values Harness streams with: connect 15 s, per-read 180 s. llm's response-header (60 s)
/// and total (600 s) bounds have no Harness counterpart and keep their values.
#[test]
fn the_default_bounds_are_the_ones_harness_streams_with() {
    assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(15));
    let limits = Limits::default();
    assert_eq!(limits.idle, Duration::from_secs(180), "a silent long think");
    assert_eq!(limits.response_headers, Duration::from_secs(60));
    assert_eq!(limits.total, Duration::from_secs(600));
    let client = HttpClient::new(limits).unwrap();
    assert_eq!(client.connect_timeout(), CONNECT_TIMEOUT);
    let chosen = HttpClient::with_connect_timeout(limits, Duration::from_secs(3)).unwrap();
    assert_eq!(
        chosen.connect_timeout(),
        Duration::from_secs(3),
        "configurable"
    );
}

#[test]
fn a_connect_bound_must_be_positive_and_at_most_one_day() {
    let day = Duration::from_hours(24);
    for refused in [Duration::ZERO, day + Duration::from_millis(1)] {
        let error = HttpClient::with_connect_timeout(Limits::default(), refused)
            .err()
            .unwrap_or_else(|| panic!("a connect bound of {refused:?} was admitted"));
        assert_eq!(error.code, ErrorCode::InvalidRequest, "{refused:?}");
    }
    HttpClient::with_connect_timeout(Limits::default(), day).expect("exactly one day is admitted");
}

#[tokio::test]
async fn a_connection_never_established_ends_at_the_connect_bound() {
    let (_listener, _fillers, url) = never_established().await;
    let client = HttpClient::with_connect_timeout(patient(), Duration::from_millis(200)).unwrap();
    let started = Instant::now();
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        client.post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        ),
    )
    .await
    .expect("the connect bound did not end the attempt")
    .err()
    .expect("no connection, no stream");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(error.code, ErrorCode::Transport, "{error}");
    assert_eq!(error.dispatch, Dispatch::Unknown, "{error}");
    assert!(
        error.retriable,
        "a connection never made is worth another attempt"
    );
}

/// The control: with the default 15 s connect bound, the same fixture is ended by the shorter
/// response-header deadline instead. A fixture whose connection were refused would end with
/// `transport` here at once, so this is what makes the case above a connect-bound case.
#[tokio::test]
async fn without_a_short_connect_bound_the_header_deadline_ends_the_same_attempt() {
    let (_listener, _fillers, url) = never_established().await;
    let client = HttpClient::new(Limits {
        response_headers: Duration::from_millis(300),
        ..patient()
    })
    .unwrap();
    let started = Instant::now();
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        client.post_sse(
            &url,
            HeaderMap::new(),
            b"{}".to_vec(),
            Framing::PayloadsOnly,
            &Cancel::new(),
        ),
    )
    .await
    .expect("the response-header deadline did not end the attempt")
    .err()
    .expect("no connection, no stream");
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(error.code, ErrorCode::Deadline, "{error}");
    assert_eq!(error.dispatch, Dispatch::Unknown, "{error}");
}
