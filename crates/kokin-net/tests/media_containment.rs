//! Network probes belong to the broker crate, even for another worker's policy.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
#[test]
#[ignore = "explicit Mac containment proof using an owned loopback server; no live service"]
fn media_policy_denies_unrelated_files_and_network() {
    let root = std::env::temp_dir().join(format!("kokin-sandbox-proof-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let private = root.join("unrelated-synthetic-file");
    std::fs::write(&private, b"unrelated synthetic content").unwrap();
    let native = root.join("native");
    std::fs::create_dir(&native).unwrap();
    let profile =
        kokin_media::worker::sandbox_profile(std::path::Path::new("/bin/cat"), &native, None)
            .unwrap();
    let read = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, "/bin/cat"])
        .arg(&private)
        .output()
        .unwrap();
    assert!(!read.status.success());
    assert!(!read.stdout.windows(9).any(|w| w == b"unrelated"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let done = stop.clone();
    let requests = count.clone();
    let server = std::thread::spawn(move || {
        while !done.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    requests.fetch_add(1, Ordering::SeqCst);
                    socket
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut bytes = [0; 1024];
                    let _ = socket.read(&mut bytes);
                    let _ = socket.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("{e}"),
            }
        }
    });
    let url = format!("http://127.0.0.1:{port}/synthetic");
    let curl = ["--noproxy", "*", "--max-time", "2", "-fsS", &url];
    let outside = std::process::Command::new("/usr/bin/curl")
        .args(curl)
        .output()
        .unwrap();
    assert!(outside.status.success());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let profile =
        kokin_media::worker::sandbox_profile(std::path::Path::new("/usr/bin/curl"), &native, None)
            .unwrap();
    let inside = std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", &profile, "/usr/bin/curl"])
        .args(curl)
        .output()
        .unwrap();
    let denied = !inside.status.success();
    let attempts = count.load(Ordering::SeqCst);
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(denied, "sandboxed network unexpectedly succeeded");
    assert_eq!(attempts, 1, "sandboxed worker reached the listener");
}
