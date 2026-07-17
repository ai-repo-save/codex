use super::*;
use futures::StreamExt;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

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
    let (mut stream, _) = listener.accept().expect("test server should accept a request");
    let mut buffer = [0_u8; 1024];
    stream
        .read(&mut buffer)
        .expect("test server should read a request");
    stream
}

#[tokio::test]
async fn stream_times_out_when_response_headers_do_not_arrive() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test server should bind");
    let request = request(&listener);
    let server = thread::spawn(move || {
        let _stream = accept_request(listener);
        thread::sleep(RESPONSE_BODY_DELAY);
    });

    let result = transport().stream(request).await;
    server.join().expect("test server should finish");

    assert!(matches!(result, Err(TransportError::Timeout)));
}

#[tokio::test]
async fn stream_continues_when_response_body_arrives_after_header_timeout() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test server should bind");
    let request = request(&listener);
    let server = thread::spawn(move || {
        let mut stream = accept_request(listener);
        stream
            .write_all(RESPONSE_HEADERS)
            .expect("test server should write response headers");
        stream
            .flush()
            .expect("test server should flush response headers");
        thread::sleep(RESPONSE_BODY_DELAY);
        stream
            .write_all(RESPONSE_BODY)
            .expect("test server should write response body");
    });

    let mut response = transport()
        .stream(request)
        .await
        .expect("response headers should arrive before the timeout");
    let body = response
        .bytes
        .next()
        .await
        .expect("response stream should yield a body chunk")
        .expect("response body should not time out");
    server.join().expect("test server should finish");

    assert_eq!(body.as_ref(), RESPONSE_BODY);
}
