//! Parent-side face worker containment and bounded protocol validation.
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
pub fn run(
    binary: &Path,
    native: &Path,
    models: &Path,
    bytes: Vec<u8>,
    request: &kokin_faces::Request,
    cancel: Arc<AtomicBool>,
) -> std::result::Result<kokin_faces::Output, crate::Error> {
    let error = |v: &str| crate::Error::Processing(v.into());
    request.validate().map_err(|e| error(&e.to_string()))?;
    let max = if request.media_type.starts_with("video/") {
        kokin_faces::MAX_VIDEO
    } else {
        crate::MAX_INPUT
    };
    if bytes.len() > max || cancel.load(Ordering::SeqCst) {
        return Err(crate::Error::Limit);
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        let _ = (binary, native, models, bytes, request, cancel);
        Err(error(
            "contained face processing is unavailable on this platform",
        ))
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        use std::{
            io::{Read, Write},
            os::unix::process::CommandExt,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let binary = binary
            .canonicalize()
            .map_err(|_| error("face worker is not installed; prepare the native bundle"))?;
        let native = native
            .canonicalize()
            .map_err(|_| error("native engines unavailable"))?;
        let models = models
            .canonicalize()
            .map_err(|_| error("face models unavailable"))?;
        let quote = |p: &Path| -> std::result::Result<String, crate::Error> {
            let s = p.to_string_lossy();
            if s.contains(['"', '\\', '\n', '\r']) {
                return Err(error("unsupported resource path"));
            }
            Ok(format!("\"{s}\""))
        };
        let helper = native.join("video/kokin-video-frames");
        let video_rule = if request.media_type.starts_with("video/") {
            format!(
                "(allow process-fork) (allow process-exec (literal {}))",
                quote(&helper)?
            )
        } else {
            String::new()
        };
        let profile = format!(
            "{}\n(allow file-read* (subpath {}))\n{}",
            crate::worker::sandbox_profile(&binary, &native, None)?,
            quote(&models)?,
            video_rule
        );
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command
            .args(["-p", &profile])
            .arg(&binary)
            .env_clear()
            .env("KOKIN_MEDIA_NATIVE", &native)
            .env("KOKIN_FACE_MODELS", &models)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|_| error("cannot start contained face worker"))?;
        let group = format!("-{}", child.id());
        let mut stdin = child.stdin.take().ok_or_else(|| error("worker input"))?;
        let stdout = child.stdout.take().ok_or_else(|| error("worker output"))?;
        let json = serde_json::to_vec(request).map_err(|_| error("face request"))?;
        let writer = std::thread::spawn(move || -> std::io::Result<()> {
            stdin.write_all(&(json.len() as u32).to_le_bytes())?;
            stdin.write_all(&json)?;
            stdin.write_all(&bytes)
        });
        let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut out = Vec::new();
            stdout
                .take(kokin_faces::MAX_OUTPUT as u64 + 1)
                .read_to_end(&mut out)?;
            Ok(out)
        });
        let started = Instant::now();
        let mut refused = false;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|_| error("face worker status"))? {
                break status;
            }
            let rss = Command::new("/bin/ps")
                .args(["-o", "rss=", "-g", &child.id().to_string()])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    let rows: String = String::from_utf8_lossy(&o.stdout).into();
                    let numbers: Option<Vec<u64>> =
                        rows.split_whitespace().map(|v| v.parse().ok()).collect();
                    numbers
                        .filter(|n| !n.is_empty())
                        .map(|n| n.iter().sum::<u64>())
                });
            if cancel.load(Ordering::SeqCst)
                || started.elapsed() > Duration::from_secs(1800)
                || rss.is_none_or(|v| v > 1024 * 1024)
            {
                refused = true;
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", "--", &group])
                    .status();
                let _ = child.kill();
                break child.wait().map_err(|_| error("face worker stop"))?;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        // Reap descendants even if a failed worker exited before cleaning up its decoder.
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &group])
            .stderr(Stdio::null())
            .status();
        let written = writer.join().map_err(|_| error("face input failed"))?;
        let output = reader
            .join()
            .map_err(|_| error("face output failed"))?
            .map_err(|_| error("face output unreadable"))?;
        if refused {
            return Err(error(
                "face job cancelled, timed out or exceeded the monitored memory bound",
            ));
        }
        written.map_err(|_| error("worker refused input"))?;
        if !status.success() || output.len() > kokin_faces::MAX_OUTPUT {
            return Err(error(
                "face worker refused media or resources; original evidence is preserved",
            ));
        }
        let result: kokin_faces::Output =
            serde_json::from_slice(&output).map_err(|_| error("invalid face output"))?;
        if result.version != 1
            || result.detections.len() > kokin_faces::MAX_FACES
            || result.sampled_frames > 1800
        {
            return Err(crate::Error::Limit);
        }
        let times = &result.sampled_timestamps_ms;
        if request.media_type.starts_with("video/")
            && (times.len() != result.sampled_frames
                || times.windows(2).any(|p| p[0] >= p[1])
                || times
                    .iter()
                    .any(|t| *t < request.start_ms || *t >= request.end_ms.unwrap_or(86_400_000)))
        {
            return Err(error("invalid sampled-frame coverage"));
        }
        for d in &result.detections {
            d.validate().map_err(|_| error("invalid face detection"))?;
        }
        Ok(result)
    }
}
