# @reaganhsu/asm

Agent Session Manager — fuzzy-search and resume Claude Code / Codex sessions from a single TUI.

## Install

```sh
npm install -g @reaganhsu/asm
```

This downloads a prebuilt native binary for your platform from the matching GitHub Release and places it on your `$PATH` as `asm`. No Rust toolchain required.

On zsh the postinstall also runs `asm install`, writing `~/.asm/shell-hooks.zsh` and a source line to `~/.zshrc` so bare `claude --resume` / `codex resume` open the asm picker. Opt out with `ASM_SKIP_SHELL_HOOK=1 npm i -g @reaganhsu/asm`.

Supported platforms: macOS (Intel + Apple Silicon), Linux x86_64. On other platforms the install fails with a pointer to the from-source instructions.

## Use

```sh
asm                  # interactive list of sessions in the current directory
asm --global         # all sessions across all directories
asm install          # set up the zsh hook so 'claude --resume' / 'codex resume' open asm
```

Press Enter on a row to resume, `ctrl-f` to fork, `ctrl-e` to export, `@tag` in the search bar to filter.

## Source

Code, issues, and from-source install instructions:
https://github.com/Cheggin/agent-session-management
