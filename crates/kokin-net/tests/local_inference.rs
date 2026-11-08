//! Explicit, owned loopback transport checks; no external service or model.
#![allow(clippy::unwrap_used)]
use kokin_net::local::{Call, Error, InferenceCapability, LoopbackInference};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[test]
#[ignore = "owned loopback server; run explicitly with KOKIN_NETWORK=allow"]
fn loopback_json_redirect_denial_and_prompt_cancellation() {
    for redirect in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..count]);
            assert!(request.starts_with("GET /api/tags HTTP/1.1"));
            if redirect {
                socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            } else {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"models\":[]}").unwrap();
            }
        });
        let response = LoopbackInference::new(&endpoint)
            .unwrap()
            .send(Call::Tags, &AtomicBool::new(false));
        if redirect {
            assert!(matches!(response, Err(Error::Unavailable)));
        } else {
            assert_eq!(response.unwrap()["models"], serde_json::json!([]));
        }
        thread.join().unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap());
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 1024];
        assert!(socket.read(&mut bytes).unwrap() > 0);
        flag.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
    });
    let start = Instant::now();
    let response = LoopbackInference::new(&endpoint)
        .unwrap()
        .send(Call::Tags, &cancel);
    assert!(matches!(response, Err(Error::Cancelled)));
    assert!(start.elapsed() < Duration::from_secs(2));
    thread.join().unwrap();
}
