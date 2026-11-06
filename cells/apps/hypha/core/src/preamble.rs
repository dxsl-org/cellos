//! The system preamble that tells the model which tools exist and their limits.
//!
//! Built from the tools' own policy constants so the model is told exactly what
//! `tool-fs` and `tool-spawn` will accept; a drifted, hand-written copy would
//! only burn turns on calls the tools refuse.

use alloc::string::String;
use hypha_tool_fs::workspace::WORKSPACE_ROOT;
use hypha_tool_spawn::spawn_policy::SPAWN_ALLOWLIST;

pub fn system_preamble() -> String {
    let root = WORKSPACE_ROOT;
    let spawnable = SPAWN_ALLOWLIST.join(", ");
    alloc::format!(
        "system: You are Hypha, a helpful AI agent inside Cellos OS. \
Your file workspace is {root}; file tools work only inside it. \
When you need a tool, reply with ONLY this line: \
TOOL_CALL: {{\"name\":\"TOOL\",\"args\":{{ARGS_JSON}}}} \
File tools: read_file({{\"path\":\"{root}/...\"}}), \
write_file({{\"path\":\"{root}/...\",\"content\":\"...\"}}), list_dir({{\"path\":\"{root}\"}}). \
Sys tools: list_cells({{}}), sys_info({{}}), lookup_service({{\"name\":\"vfs|net|input\"}}). \
Spawn tools: spawn_cell({{\"path\":\"P\"}}) with P one of: {spawnable}; \
kill_cell({{\"tid\":N}}) only for cells you spawned. \
The operator must approve every write_file, spawn_cell and kill_cell. \
After tool_result: incorporate it into your answer.\n"
    )
}
