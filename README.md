# asm — Agent Session Manager

Fuzzy-search and resume your Claude Code and Codex sessions from one TUI.

## Install

### npm (recommended)

```sh
npm install -g @reaganhsu/asm
```

Downloads a prebuilt binary for macOS (Intel + Apple Silicon) or Linux x86_64. No Rust toolchain required.

### From source

Requires Rust (`rustup` works fine).

```sh
git clone https://github.com/Cheggin/agent-session-management
cd agent-session-management
task install        # or: cargo install --path . && asm install
```

`asm install` writes a zsh hook to `~/.asm/shell-hooks.zsh` and sources it from `~/.zshrc`, so that bare `claude --resume` / `codex resume` open the picker.

## Use

```sh
asm                  # picker scoped to the current directory
asm --global         # all sessions
asm ls --filter "claude branch:main"
```

| key | action |
|---|---|
| Enter | resume the selected session |
| ctrl-f | fork at a chosen turn |
| ctrl-e | export to markdown |
| @tag | filter chips inside the search bar (agent, branch, repo, …) |
| ctrl-r / F5 | re-index |
| esc / ctrl-c | quit |

## Release process

Tag-driven. Pushing a `v*` tag triggers `.github/workflows/release.yml`, which:

1. Cross-compiles `asm` for `aarch64-apple-darwin`, `x86_64-apple-darwin`, and `x86_64-unknown-linux-gnu`.
2. Attaches the three tarballs + `SHA256SUMS` to a GitHub Release.
3. Publishes `@reaganhsu/asm@<version>` to npm using `secrets.NPM_TOKEN`.

To cut a release:

```sh
git tag v0.1.0
git push origin v0.1.0
```
