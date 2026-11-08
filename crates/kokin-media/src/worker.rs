//! The parent owns paths and cancellation; the worker receives only verified bytes.
use crate::{Error, Request, Result};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
fn error(e: impl std::fmt::Display) -> Error {
    Error::Processing(e.to_string())
}
#[cfg(target_os = "macos")]
fn literal(path: &Path) -> std::result::Result<String, Error> {
    let s = path
        .canonicalize()
        .map_err(error)?
        .to_string_lossy()
        .into_owned();
    if s.contains(['"', '\\', '\n', '\r']) {
        return Err(error("unsupported engine path"));
    }
    Ok(format!("\"{s}\""))
}
#[cfg(target_os = "macos")]
pub fn sandbox_profile(
    binary: &Path,
    native: &Path,
    ocr: Option<&Path>,
) -> std::result::Result<String, Error> {
    let binary = literal(binary)?;
    let native = literal(native)?;
    let ocr = ocr
        .map(literal)
        .transpose()?
        .map(|p| format!("(allow file-read* (subpath {p}))"))
        .unwrap_or_default();
    Ok(format!(
        r#"(version 1) (deny default)
 (allow process-exec (literal {binary}))
 (allow signal (target self)) (allow sysctl-read)
 (allow file-read* (require-all (literal "/") (vnode-type DIRECTORY)) (literal {binary}) (subpath {native})
   (subpath "/usr/lib") (subpath "/System/Library")
   (subpath "/System/Volumes/Preboot/Cryptexes/OS/usr/lib")
   (literal "/dev/null") (literal "/dev/random") (literal "/dev/urandom"))
 (allow file-write* (literal "/dev/null"))
 (allow mach-lookup (global-name "com.apple.FontObjectsServer"))
 {ocr}"#
    ))
}
pub fn run(
    binary: &Path,
    native: &Path,
    ocr: Option<&Path>,
    bytes: Vec<u8>,
    request: &Request,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<Result, Error> {
    if bytes.len() > crate::MAX_INPUT || cancel.load(Ordering::SeqCst) {
        return Err(error("media cancelled or too large"));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (binary, native, ocr, request);
        return Err(error(
            "tested media containment is unavailable on this platform",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let binary_path = binary.canonicalize().map_err(error)?;
        let native_path = native.canonicalize().map_err(error)?;
        let ocr_path = ocr.map(|p| p.canonicalize()).transpose().map_err(error)?;
        let binary = binary_path.as_path();
        let native = native_path.as_path();
        let ocr = ocr_path.as_deref();
        let profile = sandbox_profile(binary, native, ocr)?;
        let mut command = std::process::Command::new("/usr/bin/sandbox-exec");
        command.args(["-p", &profile]).arg(binary);
        #[cfg(test)]
        if binary == Path::new("/bin/sleep") {
            command.arg("2");
        }
        command.env_clear().env("KOKIN_MEDIA_NATIVE", native);
        if let Some(ocr) = ocr {
            command.env("KOKIN_OCR_DATA", ocr);
        }
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn().map_err(error)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| error("worker input missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| error("worker output missing"))?;
        let json = serde_json::to_vec(request).map_err(error)?;
        let writer = std::thread::spawn(move || -> std::io::Result<()> {
            stdin.write_all(&(json.len() as u32).to_le_bytes())?;
            stdin.write_all(&json)?;
            stdin.write_all(&bytes)?;
            Ok(())
        });
        let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut out = Vec::new();
            stdout
                .take(crate::MAX_OUTPUT as u64 + 1)
                .read_to_end(&mut out)?;
            Ok(out)
        });
        let started = Instant::now();
        let mut cancelled = false;
        let status = loop {
            if cancel.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(90) {
                cancelled = true;
                let _ = child.kill();
                break child.wait().map_err(error)?;
            }
            if let Some(status) = child.try_wait().map_err(error)? {
                break status;
            }
            // Fail closed if the native process cannot be monitored. A race
            // with normal exit is checked before treating absent RSS as failure.
            let rss = std::process::Command::new("/bin/ps")
                .args(["-o", "rss=", "-p", &child.id().to_string()])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<u64>()
                        .ok()
                });
            if rss.is_none() || rss.is_some_and(|rss| rss > 512 * 1024) {
                if let Some(status) = child.try_wait().map_err(error)? {
                    break status;
                }
                cancelled = true;
                let _ = child.kill();
                break child.wait().map_err(error)?;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let written = writer.join().map_err(|_| error("worker input failed"))?;
        let output = reader
            .join()
            .map_err(|_| error("worker output failed"))?
            .map_err(error)?;
        if cancelled {
            return Err(error(
                "media cancelled, timed out, or its memory budget could not be enforced",
            ));
        }
        written.map_err(error)?;
        if !status.success() {
            return Err(error("worker refused the document, crashed, or containment denied access; raw evidence is preserved"));
        }
        if output.len() > crate::MAX_OUTPUT {
            return Err(Error::Limit);
        }
        let result: Result = serde_json::from_slice(&output).map_err(error)?;
        if result.version != 1
            || result.observations.len() > 10000
            || result.thumbnail_png.len() > 8 * 1024 * 1024
            || result.observations.iter().any(|o| o.value.len() > 8192)
        {
            return Err(Error::Limit);
        }
        Ok(result)
    }
}

#[cfg(all(test, target_os = "macos"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit sandboxed process fault fixtures on Mac"]
    fn crashes_and_mid_processing_cancellation_never_return_a_result() {
        let native =
            std::env::temp_dir().join(format!("kokin-worker-fault-{}", std::process::id()));
        std::fs::create_dir(&native).unwrap();
        let request = Request {
            version: 1,
            media_type: "image/png".into(),
            page: 0,
            ocr: false,
            image_recipe: None,
        };
        // A fixture executable deliberately exits before producing the protocol.
        assert!(run(
            Path::new("/usr/bin/false"),
            &native,
            None,
            vec![0],
            &request,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        // A test-only sleep argument keeps the fixture process alive while cancellation
        // stops it; neither the fixture argument nor its command is compiled into production.
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            stop.store(true, Ordering::SeqCst);
        });
        assert!(run(
            Path::new("/bin/sleep"),
            &native,
            None,
            vec![0; 100],
            &request,
            cancel
        )
        .is_err());
        timer.join().unwrap();
        std::fs::remove_dir_all(native).unwrap();
    }
}
