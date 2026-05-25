# asm — Agent Session Manager

Fuzzy-search and resume your Claude Code and Codex sessions from one TUI.

## Install

### npm

```sh
npm install -g @reaganhsu/asm
```
## Use

```sh
asm                  # picker scoped to the current directory
asm --global         # all sessions
claude -- resume     # auto detected, proxies to asm"
codex resume         # auto detected, proxies to asm"
```

| key | action |
|---|---|
| Enter | resume the selected session |
| ctrl-f | fork at a chosen turn |
| ctrl-e | export to markdown |
| @tag | filter chips inside the search bar (agent, branch, repo, …) |
| ctrl-r / F5 | re-index |
| esc / ctrl-c | quit |

