//! Measure how long the Argon2id profiles take on this machine.
//!
//! The cost parameters in `KdfProfile::V1` are a trade-off between resisting
//! offline cracking and keeping case-open latency tolerable. That trade-off is
//! machine-dependent, so it should be measured rather than asserted — this is
//! the tool that produces the numbers quoted in ADR-0015.
//!
//! Run with optimisations, or the numbers are meaningless:
//!
//! ```text
//! cargo run -p kokin-keys --example kdf_timing --release
//! ```

// A benchmark harness legitimately unwraps: a failure here should stop the run
// loudly, not be threaded through a Result for a developer tool.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use kokin_keys::{derive_kek, generate_salt, KdfProfile};

const ROUNDS: u32 = 5;

fn main() {
    let salt = generate_salt().unwrap();

    let profiles = [
        ("V1 (default)", KdfProfile::V1),
        (
            "test profile",
            KdfProfile {
                id: 1,
                m_cost_kib: 1024,
                t_cost: 1,
                p_cost: 1,
            },
        ),
    ];

    println!("Argon2id derivation, mean of {ROUNDS} runs\n");
    println!(
        "{:<16} {:>10} {:>4} {:>4} {:>12}",
        "profile", "memory", "t", "p", "mean"
    );

    for (name, profile) in profiles {
        // One untimed run first, so the measurement is not dominated by the
        // first large allocation.
        let _ = derive_kek("warmup", &salt, profile).unwrap();

        let start = Instant::now();
        for i in 0..ROUNDS {
            let passphrase = format!("benchmark passphrase {i}");
            let _ = derive_kek(&passphrase, &salt, profile).unwrap();
        }
        let mean = start.elapsed() / ROUNDS;

        println!(
            "{:<16} {:>7} MiB {:>4} {:>4} {:>9.1?}",
            name,
            profile.m_cost_kib / 1024,
            profile.t_cost,
            profile.p_cost,
            mean
        );
    }
}
