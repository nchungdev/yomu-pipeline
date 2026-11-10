#![cfg(feature = "llm")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;
use yomu_pipeline::llm::{
    Header, HttpRequest, HttpTransport, Method, ReqwestTransport, TransportError,
};

/// One-shot loopback server: reads the request head, then runs `serve` on the stream.
fn serve_once(serve: impl FnOnce(std::net::TcpStream) + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}/x", listener.local_addr().expect("addr"));
    thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let mut head = Vec::new();
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => head.extend_from_slice(&buf[..n]),
                }
            }
            serve(s);
        }
    });
    url
}

fn respond(status: &str, body: Vec<u8>) -> impl FnOnce(std::net::TcpStream) + Send + 'static {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\nX-Test: yes\r\n\r\n",
        body.len()
    );
    move |mut s| {
        let _ = s.write_all(head.as_bytes());
        let _ = s.write_all(&body);
    }
}

fn req(url: String, timeout: Duration) -> HttpRequest {
    HttpRequest {
        method: Method::Post,
        url,
        headers: vec![
            Header::secret("authorization", "Bearer k"),
            Header::new("content-type", "application/json"),
        ],
        body: b"{}".to_vec(),
        timeout,
    }
}

#[tokio::test]
async fn ok_response_with_body_and_headers() {
    let url = serve_once(respond("200 OK", b"hello".to_vec()));
    let t = ReqwestTransport::new().expect("client");
    let r = t.send(&req(url, Duration::from_secs(2))).await.expect("ok");
    assert_eq!((r.status, r.body.as_slice()), (200, &b"hello"[..]));
    assert!(r.headers.iter().any(|(k, v)| k == "x-test" && v == "yes"));
}

#[tokio::test]
async fn error_status_keeps_body() {
    let url = serve_once(respond("500 Internal Server Error", b"boom".to_vec()));
    let t = ReqwestTransport::new().expect("client");
    let r = t
        .send(&req(url, Duration::from_secs(2)))
        .await
        .expect("resp");
    assert_eq!((r.status, r.body.as_slice()), (500, &b"boom"[..]));
}

#[tokio::test]
async fn stalled_server_times_out() {
    let url = serve_once(|_s| thread::sleep(Duration::from_secs(3)));
    let t = ReqwestTransport::new().expect("client");
    let e = t
        .send(&req(url, Duration::from_millis(300)))
        .await
        .expect_err("timeout");
    assert_eq!(e, TransportError::Timeout);
}

#[tokio::test]
async fn refused_connection_is_connect_error() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    let t = ReqwestTransport::new().expect("client");
    let e = t
        .send(&req(
            format!("http://127.0.0.1:{port}/"),
            Duration::from_secs(2),
        ))
        .await
        .expect_err("err");
    assert_eq!(e, TransportError::Connect);
}

#[tokio::test]
async fn oversized_success_body_is_rejected() {
    let url = serve_once(respond("200 OK", vec![b'a'; 2 * 1024 * 1024 + 1024]));
    let t = ReqwestTransport::new().expect("client");
    let e = t
        .send(&req(url, Duration::from_secs(2)))
        .await
        .expect_err("too large");
    assert_eq!(e, TransportError::BodyTooLarge);
}

#[tokio::test]
async fn error_body_is_truncated_at_cap() {
    let url = serve_once(respond("502 Bad Gateway", vec![b'e'; 100 * 1024]));
    let t = ReqwestTransport::new().expect("client");
    let r = t
        .send(&req(url, Duration::from_secs(2)))
        .await
        .expect("resp");
    assert_eq!((r.status, r.body.len()), (502, 64 * 1024));
}
