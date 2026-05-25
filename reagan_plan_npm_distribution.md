# Plan: distribute `asm` as `@reaganhsu/asm` on npm

## Goal
Users can `npm i -g @reaganhsu/asm` and get the `asm` binary on PATH with
near-zero startup overhead (no Node wrapper — postinstall downloads the
native binary).

## Architecture
Standard "postinstall fetches a native binary" pattern (turbo / Tailwind
standalone / Vercel CLI use this shape):

1. `package.json` declares `bin: { "asm": "bin/asm" }`.
2. `install.js` runs on `npm install`:
   - Detects `process.platform` + `process.arch`.
   - Maps to a Rust target triple: `aarch64-apple-darwin`,
     `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
     `aarch64-unknown-linux-gnu`.
   - Downloads `asm-vX.Y.Z-<target>.tar.gz` from the matching GitHub
     Release, verifies SHA256 against a manifest checked into the
     package, extracts the binary to `bin/asm`, sets +x.
3. `bin/asm` ships as a tiny no-op shim that exits with a helpful
   message — overwritten by `install.js` on first install so npm's
   symlink machinery has something to link to either way.

This avoids the Node startup tax on every invocation (would 5–10× our
current 2–100ms latency).

## Foundation: GitHub Release pipeline
The npm package depends on prebuilt binaries existing at predictable
URLs. Set up GitHub Actions first.

`.github/workflows/release.yml`:
- Trigger: push of `v*` tags
- Matrix:
  | runner | rust target |
  |---|---|
  | `macos-latest` | `aarch64-apple-darwin` |
  | `macos-13` | `x86_64-apple-darwin` |
  | `ubuntu-latest` | `x86_64-unknown-linux-gnu` |
- Steps per job: checkout, install Rust + target, `cargo build --release --target <triple>`, tar the binary, upload to release.
- Final job: `softprops/action-gh-release` creates the release and attaches all artifacts + a `SHA256SUMS` file.

Aarch64 Linux is deferred (needs cross or QEMU); add later.

## Files
- `.github/workflows/release.yml`
- `npm/package.json` — name `@reaganhsu/asm`, version mirrors crate
- `npm/install.js` — postinstall download + verify + extract
- `npm/bin/asm` — shim placeholder
- `npm/README.md` — npm-facing docs
- `npm/.npmignore`
- Root `README.md` — install instructions (homebrew TBD, npm, from source)

## Versioning
The npm package version must match the crate version so `install.js` can
build the download URL. CI: a separate workflow on tag push could
auto-publish to npm using `NPM_TOKEN` (out of scope for first PR — manual
`npm publish --access public` is fine to start).

## Out of scope (for this round)
- Homebrew tap (separate repo, add after npm proves the release pipeline works)
- Linux arm64 binaries
- Windows
- Auto-publish to npm from CI
- Rename consideration (crates.io `asm` is taken — only matters if we
  ever publish to crates.io)

## Verification
- After implementing locally, push a `v0.1.0` tag to a test branch and
  verify GitHub Actions produces the 3 binaries.
- Run `node npm/install.js` locally with the release in place and
  confirm it places a working binary at `npm/bin/asm`.
- Manually `npm pack` and `npm i -g ./reaganhsu-asm-0.1.0.tgz`, run
  `asm --version`.
