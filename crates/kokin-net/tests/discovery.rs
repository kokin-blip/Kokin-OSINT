//! Explicit local-server acceptance. Never contacts an upstream search engine.
#![allow(clippy::unwrap_used)]
use std::{
    io::{Read, Write},
    sync::atomic::AtomicBool,
};
#[test]
#[ignore = "binds the fixed local SearXNG port to an owned synthetic server"]
fn loopback_discovery_refuses_offline_redirects_and_oversized_responses() {
    let listener = std::net::TcpListener::bind("127.0.0.1:8088").unwrap();
    let original = std::env::var_os("KOKIN_NETWORK");
    std::env::set_var("KOKIN_NETWORK", "allow");
    let server = std::thread::spawn(move || {
        for i in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 8192];
            let n = socket.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]);
            assert!(request.starts_with("GET /search?"));
            assert!(!request.contains("multipart"));
            let body = r#"{"results":[{"title":"Synthetic lead","url":"https://example.org/page","content":"fixture"}]}"#;
            let response=match i{0=>format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()),1=>"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),2=>"HTTP/1.1 200 OK\r\nContent-Length: 5000000\r\nConnection: close\r\n\r\n".into(),_=>"HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nSet-Cookie: must-not-be-captured\r\nContent-Length: 9\r\nConnection: close\r\n\r\nchallenge".into()};
            socket.write_all(response.as_bytes()).unwrap();
        }
    });
    let no = AtomicBool::new(false);
    let yes = AtomicBool::new(true);
    assert!(kokin_net::discovery::search("fixture", 1, &no, &no).is_err());
    assert!(kokin_net::discovery::search("fixture", 1, &yes, &yes).is_err());
    let (url, response) =
        kokin_net::discovery::search("synthetic topic & name", 1, &no, &yes).unwrap();
    assert!(url.starts_with("http://127.0.0.1:8088/search?"));
    assert!(response.body.contains("Synthetic lead"));
    assert!(kokin_net::discovery::search("redirect", 1, &no, &yes).is_err());
    assert!(kokin_net::discovery::search("oversized", 1, &no, &yes).is_err());
    let (_, raw) = kokin_net::discovery::search_response("blocked", 1, &no, &yes).unwrap();
    assert_eq!(raw.status, 403);
    assert_eq!(raw.body_bytes.unwrap(), b"challenge");
    assert!(!raw.headers.contains_key("set-cookie"));
    server.join().unwrap();
    if let Some(v) = original {
        std::env::set_var("KOKIN_NETWORK", v)
    } else {
        std::env::remove_var("KOKIN_NETWORK")
    }
}
