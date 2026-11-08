//! Native capability reads, without model downloads or case data.
use std::path::Path;
#[derive(Debug, Clone)]
pub struct Hardware {
    pub architecture: String,
    pub cpu_threads: usize,
    pub total_memory_bytes: Option<u64>,
    pub available_memory_bytes: Option<u64>,
    pub free_storage_bytes: Option<u64>,
    pub memory_kind: String,
    pub accelerator_status: String,
}
fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.len() > 64 * 1024 {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}
pub fn detect(resource_directory: &Path) -> Hardware {
    let architecture = std::env::consts::ARCH.to_owned();
    let mut result = Hardware {
        cpu_threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        architecture: architecture.clone(),
        total_memory_bytes: None,
        available_memory_bytes: None,
        free_storage_bytes: None,
        memory_kind: "unknown".into(),
        accelerator_status: "CPU fallback available; accelerator runtime unverified".into(),
    };
    #[cfg(target_os = "macos")]
    {
        result.total_memory_bytes =
            output("/usr/sbin/sysctl", &["-n", "hw.memsize"]).and_then(|s| s.trim().parse().ok());
        result.available_memory_bytes =
            output("/usr/bin/vm_stat", &[]).and_then(|s| available_pages(&s));
        result.memory_kind = if architecture == "aarch64" {
            "Apple unified memory; GPU allocations share system RAM"
        } else {
            "system RAM; dedicated accelerator memory unknown"
        }
        .into();
        if let Some(displays) = output(
            "/usr/sbin/system_profiler",
            &["SPDisplaysDataType", "-json"],
        ) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&displays) {
                let rows = value["SPDisplaysDataType"].as_array();
                if let Some(gpu) = rows.and_then(|rows| rows.first()) {
                    let name = gpu["sppci_model"].as_str().unwrap_or("Reported GPU");
                    let metal = gpu["spdisplays_metal"]
                        .as_str()
                        .or_else(|| gpu["spdisplays_metal_supported"].as_str())
                        .unwrap_or("support unknown");
                    result.accelerator_status=format!("{name}: {metal}; CPU fallback available. Ollama acceleration requires runtime validation.");
                }
            }
        }
        if let Some(path) = resource_directory.to_str() {
            result.free_storage_bytes = output("/bin/df", &["-Pk", path]).and_then(|s| {
                s.lines()
                    .nth(1)?
                    .split_whitespace()
                    .nth(3)?
                    .parse::<u64>()
                    .ok()?
                    .checked_mul(1024)
            });
        }
    }
    result
}
/// Conservative reclaimable estimate: free plus inactive pages, not total RAM.
fn available_pages(text: &str) -> Option<u64> {
    let first = text.lines().next()?;
    let page = first
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let value = |name: &str| {
        text.lines()
            .find(|line| line.starts_with(name))?
            .split(':')
            .nth(1)?
            .trim()
            .trim_end_matches('.')
            .parse::<u64>()
            .ok()
    };
    value("Pages free")?
        .checked_add(value("Pages inactive")?)?
        .checked_mul(page)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_fields_are_unknown_not_zero() {
        assert_eq!(available_pages("missing"), None);
        assert_eq!(available_pages("Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 10.\nPages inactive: 20.\nPages wired down: 100."),Some(30*16384));
    }
}
