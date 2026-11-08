#![allow(clippy::unwrap_used)]
#[test]
fn source_and_lock_guards_are_part_of_the_default_suite() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let output = std::process::Command::new("python3")
        .arg(root.join("scripts/lint_dependency_patches.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn rfc2822_nested_comments_are_bounded_without_rejecting_normal_dates() {
    use time::{format_description::well_known::Rfc2822, OffsetDateTime};
    assert!(
        OffsetDateTime::parse("Fri, 21 Nov 1997 09:55:06 -0600 (normal comment)", &Rfc2822).is_ok()
    );
    let hostile = format!(
        "Fri, 21 Nov 1997 09:55:06 -0600 {}{}",
        "(".repeat(8192),
        ")".repeat(8192)
    );
    assert!(OffsetDateTime::parse(&hostile, &Rfc2822).is_err());
}
