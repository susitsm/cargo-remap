# AGENTS.md

<!-- cargo-remap:begin -->
## Building and worktrees (cargo-remap)

This repo maps every git worktree to one fixed path with cargo-remap. Read
`.agents/skills/cargo-remap/SKILL.md` before running cargo or any path-dependent command (debuggers,
formatters, linters, scripts, test runners), and before creating worktrees. In short:

- Run cargo as `cargo remap cargo <args>` (or plain `cargo` if `install` was done).
- Run other path-sensitive commands as `cargo remap exec <command>`; run git normally.
- Create worktrees with `cargo remap worktree add <path> [-b <branch>]`, not `git worktree add`.
- Paths in output under the mapped path are files of your current worktree.
<!-- cargo-remap:end -->
