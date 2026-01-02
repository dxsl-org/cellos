# ViOS Shell Design (viSH) - Phase 18

## 1. Objectives
- Provide a CLI interface to interact with the Kernel and Filesystem.
- Verify User-Mode System Calls (`read`, `write`, `spawn`, `dir`).
- Test Keyboard Driver buffering.

## 2. Why Not "Dash"?
- **Dash (Debian Almquist Shell)** is a POSIX-compliant shell.
- **Requirements**: `fork`, `execve`, `pipe`, `dup2`, Signals (`SIGINT`, `SIGTSTP`), Process Groups, and a full C Standard Library (`libc`).
- **ViOS State**:
    - Supports `spawn` (different from `fork/exec`).
    - No Signals yet (Planned Phase 20).
    - No Pipes yet.
    - `MiniFat` is Read-Only (mostly).
- **Conclusion**: Porting Dash now is premature. We need a custom, lightweight Rust shell first.

## 3. Architecture: `viSH` (The ViOS Shell)
A custom Rust application running in User Mode.

### Features
1.  **Line Editor**:
    - Support Backspace, Left/Right navigation.
    - Basic History (Up/Down arrow).
    - *Implementation*: `LineReader` struct buffering chars until `\n`.
2.  **Tokenizer**:
    - Split input by spaces (respecting quotes `'` `"`).
3.  **Built-in Commands**:
    - `help`: List commands.
    - `clear`: Clear screen (ANSI codes).
    - `ls` / `dir`: List files in ROOT_FS.
    - `cat`: Read file content.
    - `echo`: Print args.
    - `reboot`: Shutdown/Restart.
4.  **External Commands**:
    - If input matches `app_name`, call `sys_spawn("app_name")`.
    - Wait for child to exit (`sys_wait`).

## 4. Implementation Plan
1.  **`libs/ostd/src/io`**: Add `Stdin` / `LineReader` helper.
2.  **`apps/shell`**: Create new crate.
3.  **Command Loop**:
    - Print `viOS$ `.
    - Read line.
    - Parse & Execute.
