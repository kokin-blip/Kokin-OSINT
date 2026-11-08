//! Shared bounded public-network admission, including cancellation while queued.
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};
#[derive(Default)]
struct State {
    active: usize,
    hosts: HashMap<String, (usize, Instant)>,
}
#[derive(Clone, Default)]
pub struct Scheduler {
    state: Arc<Mutex<State>>,
}
pub struct Permit {
    scheduler: Scheduler,
    host: String,
}
impl Scheduler {
    pub fn acquire(
        &self,
        host: &str,
        cancel: &AtomicBool,
        online: &AtomicBool,
    ) -> Result<Permit, String> {
        self.acquire_with_policy(host, cancel, online, || {
            std::env::var("KOKIN_NETWORK").as_deref() != Ok("deny")
        })
    }
    // Policy injection is private: deterministic admission tests never modify
    // process environment or enable a transport under the offline CI policy.
    fn acquire_with_policy(
        &self,
        host: &str,
        cancel: &AtomicBool,
        online: &AtomicBool,
        allowed: impl Fn() -> bool,
    ) -> Result<Permit, String> {
        loop {
            if cancel.load(Ordering::SeqCst) || !online.load(Ordering::SeqCst) || !allowed() {
                return Err("queued request cancelled or offline".into());
            }
            {
                let mut s = self
                    .state
                    .lock()
                    .map_err(|_| "network scheduler unavailable")?;
                let now = Instant::now();
                s.hosts.retain(|_, (n, next)| {
                    *n > 0 || now.saturating_duration_since(*next) < Duration::from_secs(300)
                });
                let allowed = s
                    .hosts
                    .get(host)
                    .is_none_or(|(n, next)| *n == 0 && now >= *next);
                if s.active < 4 && allowed {
                    s.active += 1;
                    s.hosts
                        .insert(host.into(), (1, now + Duration::from_secs(1)));
                    return Ok(Permit {
                        scheduler: self.clone(),
                        host: host.into(),
                    });
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut s) = self.scheduler.state.lock() {
            s.active = s.active.saturating_sub(1);
            if let Some((active, _)) = s.hosts.get_mut(&self.host) {
                *active = active.saturating_sub(1);
            }
        }
    }
}
pub fn shared() -> &'static Scheduler {
    static INSTANCE: OnceLock<Scheduler> = OnceLock::new();
    INSTANCE.get_or_init(Scheduler::default)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn four_destinations_run_but_a_fifth_or_same_host_waits_and_cancels() {
        let s = Scheduler::default();
        let online = AtomicBool::new(true);
        let cancel = AtomicBool::new(false);
        let permits = (0..4)
            .map(|i| {
                s.acquire_with_policy(&format!("host{i}.example"), &cancel, &online, || true)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(s.state.lock().unwrap().active, 4);
        let waiting = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let h =
                scope.spawn(|| s.acquire_with_policy("fifth.example", &waiting, &online, || true));
            std::thread::sleep(Duration::from_millis(30));
            waiting.store(true, Ordering::SeqCst);
            assert!(h.join().unwrap().is_err());
        });
        drop(permits);
        assert_eq!(s.state.lock().unwrap().active, 0);
        let _first = s
            .acquire_with_policy("same.example", &cancel, &online, || true)
            .unwrap();
        let stop = AtomicBool::new(true);
        assert!(s
            .acquire_with_policy("same.example", &stop, &online, || true)
            .is_err());
    }
}
