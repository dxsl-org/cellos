//! DTO definitions matching Haily Personal Assistant wire protocol.

extern crate alloc;

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Request incoming from Haily desktop or HTTP gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HailyRequest {
    pub session_id: String,
    pub adapter_id: String,
    pub message: String,
    #[serde(default)]
    pub user_ref: Option<String>,
}

/// Structured response chunk returned to Haily client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HailyResponse {
    pub session_id: String,
    pub status: String,
    pub reply: String,
    pub tool_invocations: usize,
}

/// Explicit approval request for mutating tools.
#[allow(dead_code)]
pub struct ToolApprovalRequest {
    pub tool_name: String,
    pub args_json: String,
    pub risk_level: String,
}
