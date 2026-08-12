# Platform limitations

Stated plainly, because the acceptance criteria require known limitations to be
documented honestly rather than discovered by users.

## macOS builds are unsigned and unnotarised

**What this means for you:** macOS will refuse to open the application on first
launch, showing a message that it "cannot be opened because the developer cannot
be verified" or that the app "is damaged and can't be opened".

**Workaround:** right-click the application and choose *Open*, then confirm. Or
run `xattr -dr com.apple.quarantine /Applications/Kokin-OSINT.app`.

**Why:** notarisation requires a paid Apple Developer Program membership
($99/year). The owner decided on 2026-08-12 not to purchase one at this stage.
This is a deliberate, revisitable trade-off, not an oversight (risk R-002).

## macOS is built and tested only in CI

There is no Mac available to the developer. Every macOS build and test runs on
GitHub Actions `macos-14` (Apple Silicon) runners.

**Consequence:** automated tests cover macOS, but *interactive* behaviour —
window chrome, WKWebView rendering differences, drag-and-drop, file dialogs,
keyboard shortcuts, display scaling — is **not manually verified**. Bugs in
those areas will reach users before they reach the developer.

The CI matrix exists from the first commit specifically so that this gap stays
as small as possible. It is still a real gap. macOS bug reports with screenshots
are disproportionately valuable.

**Not covered at all:** Intel Macs. CI builds `aarch64-apple-darwin` only. An
`x86_64-apple-darwin` leg can be added, but it would be even less verified than
the Apple Silicon one.

## Windows requires a native perl to build (developers only)

Not a limitation for end users — only for anyone building from source. See the
README prerequisites and ADR-0004.

## No Linux build

Not currently a target. The Rust core is portable and the shell is
cross-platform, so this is a packaging and testing question rather than an
architectural one, but nothing is verified and nothing is shipped.
