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

## Apple Silicon macOS verification

An actual Mac is available in this workspace. On 2026-10-08 the debug app bundle
launched and the embedded WDIO lifecycle test passed: create, recovery challenge,
offline navigation, close and reopen. The case gate and shared observation detail were visually inspected. A native
file picker imported synthetic HTML, explicit extraction recorded two observations,
and the observation opened its verified source-byte lineage.
This is partial acceptance. Additional dialogs, keyboard
navigation, scaling, installation and integrated recovery still need the bounded
manual pass. Windows manual verification has not occurred in this implementation
slice; the existing CI matrix remains the automated platform build path.

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
