//! Sandboxed Tool Routing and Execution for Haily Agent.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use agent_proto::{AgentToolRequest, AgentToolResponse};
use ostd::syscall::{sys_recv, sys_send, SyscallResult};

/// Registry of tool cell TIDs.
#[derive(Debug, Default, Clone, Copy)]
pub struct ToolRouter {
    pub fs_cell: usize,
    pub sys_cell: usize,
    pub spawn_cell: usize,
}

impl ToolRouter {
    /// Route a tool name to the responsible tool cell TID.
    pub fn route(&self, tool_name: &str) -> usize {
        match tool_name {
            "read_file" | "write_file" | "list_dir" => self.fs_cell,
            "list_cells" | "sys_info" | "lookup_service" => self.sys_cell,
            "spawn_cell" | "kill_cell" => self.spawn_cell,
            _ => 0,
        }
    }

    /// Check if this tool performs mutating side effects requiring approval.
    pub fn requires_approval(tool_name: &str) -> bool {
        matches!(tool_name, "write_file" | "spawn_cell" | "kill_cell")
    }

    /// Execute a tool invocation via IPC to the target tool cell.
    pub fn execute(&self, name: &str, args_json: &str) -> Result<String, String> {
        let cell_tid = self.route(name);
        if cell_tid == 0 {
            return Err(format!("tool '{}' unavailable (cell not bound)", name));
        }

        let req = AgentToolRequest::Invoke {
            name,
            args_json,
        };
        let mut out = [0u8; 4096];
        let encoded = postcard::to_slice(&req, &mut out)
            .map_err(|_| String::from("tool request payload exceeded IPC buffer"))?;

        match sys_send(cell_tid, encoded) {
            SyscallResult::Ok(_) => {}
            _ => return Err(format!("IPC send to tool cell '{}' failed", name)),
        }

        let mut buf = [0u8; 4096];
        loop {
            match sys_recv(0, &mut buf) {
                SyscallResult::Ok(sender) if sender == cell_tid => {
                    return match postcard::take_from_bytes::<AgentToolResponse>(&buf) {
                        Ok((AgentToolResponse::Ok { result_json }, _)) => Ok(result_json),
                        Ok((AgentToolResponse::Err { message }, _)) => Err(message),
                        Err(_) => Err(String::from("corrupted AgentToolResponse")),
                    };
                }
                SyscallResult::Ok(_) => continue,
                _ => return Err(format!("no reply received from tool cell '{}'", name)),
            }
        }
    }
}
