use super::*;
use futures::StreamExt;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tokio::sync::oneshot;
use serde_json::json;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

const RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_millis(50);
const RESPONSE_BODY_DELAY: Duration = Duration::from_millis(100);
const RESPONSE_HEADERS: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n";
const RESPONSE_BODY: &[u8] = b"hello";

fn transport() -> ReqwestTransport {
    ReqwestTransport::new(
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test HTTP client should build"),
    )
}

fn request(listener: &TcpListener) -> Request {
    let address = listener
        .local_addr()
        .expect("test listener should have an address");
    let mut request = Request::new(http::Method::GET, format!("http://{address}"));
    request.response_header_timeout = Some(RESPONSE_HEADER_TIMEOUT);
    request
}

fn accept_request(listener: TcpListener) -> TcpStream {
    let (mut stream, _) = listener
        .accept()
        .expect("test server should accept a request");
    let mut buffer = [0_u8; 1024];
    let bytes_read = stream
        .read(&mut buffer)
        .expect("test server should read a request");
    assert_ne!(bytes_read, 0, "test server should receive request bytes");
    stream
}

#[tokio::test]
async fn stream_times_out_when_response_headers_do_not_arrive() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test server should bind");
    let request = request(&listener);
    let (server_ready, server_ready_rx) = oneshot::channel();
    let (request_received, request_received_rx) = oneshot::channel();
    let (finish_server, finish_server_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        server_ready
            .send(())
            .expect("test should wait for the server to be ready");
        let _stream = accept_request(listener);
        request_received
            .send(())
            .expect("test should wait for the request");
        finish_server_rx
            .recv()
            .expect("test should release the server");
    });

    server_ready_rx
        .await
        .expect("test server should be ready to accept the request");
    let result = transport().stream(request).await;
    request_received_rx
        .await
        .expect("test server should receive the request");
    finish_server
        .send(())
        .expect("test server should still be waiting");
    server.join().expect("test server should finish");

    assert!(matches!(result, Err(TransportError::Timeout)));
}

#[tokio::test]
async fn stream_continues_when_response_body_arrives_after_header_timeout() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test server should bind");
    let request = request(&listener);
    let (server_ready, server_ready_rx) = oneshot::channel();
    let (headers_sent, headers_sent_rx) = oneshot::channel();
    let (send_body, send_body_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        server_ready
            .send(())
            .expect("test should wait for the server to be ready");
        let mut stream = accept_request(listener);
        stream
            .write_all(RESPONSE_HEADERS)
            .expect("test server should write response headers");
        stream
            .flush()
            .expect("test server should flush response headers");
        headers_sent
            .send(())
            .expect("test should wait for response headers");
        send_body_rx
            .recv()
            .expect("test should release the response body");
        stream
            .write_all(RESPONSE_BODY)
            .expect("test server should write response body");
    });

    server_ready_rx
        .await
        .expect("test server should be ready to accept the request");
    let mut response = transport()
        .stream(request)
        .await
        .expect("response headers should arrive before the timeout");
    headers_sent_rx
        .await
        .expect("test server should send response headers");
    tokio::time::sleep(RESPONSE_BODY_DELAY).await;
    send_body
        .send(())
        .expect("test server should be waiting to send the response body");
    let body = response
        .bytes
        .next()
        .await
        .expect("response stream should yield a body chunk")
        .expect("response body should not time out");
    server.join().expect("test server should finish");

    assert_eq!(body.as_ref(), RESPONSE_BODY);
}

#[tokio::test]
async fn enabled_request_logging_emits_transport_url_and_body() {
    let logs = capture_transport_logs(HttpClient::new(test_reqwest_client())).await;

    assert!(logs.contains("log capture sentinel"));
    assert!(logs.contains("url-secret"));
    assert!(logs.contains("body-secret"));
}

#[tokio::test]
async fn disabled_request_logging_suppresses_transport_url_and_body() {
    let logs = capture_transport_logs(HttpClient::new_without_request_logging(
        test_reqwest_client(),
    ))
    .await;

    assert!(logs.contains("log capture sentinel"));
    assert!(!logs.contains("url-secret"));
    assert!(!logs.contains("body-secret"));
}

fn test_reqwest_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("HTTP client should build")
}

async fn capture_transport_logs(client: HttpClient) -> String {
    let unavailable_server =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("server port should bind");
    let server_addr = unavailable_server
        .local_addr()
        .expect("server listener should have an address");
    drop(unavailable_server);
    let transport = ReqwestTransport::from_http_client(client);
    let log_buffer = Arc::new(Mutex::new(Vec::new()));
    let writer_buffer = Arc::clone(&log_buffer);
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(move || TestLogWriter(Arc::clone(&writer_buffer)))
            .with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("codex_http_client::transport", tracing::Level::TRACE),
            ),
    );
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::trace!(target: "codex_http_client::transport", "log capture sentinel");
    let mut request = Request::new(
        Method::POST,
        format!("http://{server_addr}/request?token=url-secret"),
    )
    .with_json(&json!({"token": "body-secret"}));
    request.timeout = Some(Duration::from_secs(1));

    let _ = transport.execute(request).await;

    String::from_utf8(
        log_buffer
            .lock()
            .expect("log buffer should not be poisoned")
            .clone(),
    )
    .expect("captured logs should be UTF-8")
}

#[derive(Clone)]
struct TestLogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for TestLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("log buffer should not be poisoned"))?
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
