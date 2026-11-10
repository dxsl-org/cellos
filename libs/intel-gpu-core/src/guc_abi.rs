//! GuC host-to-GuC messages the driver sends and the GuC-to-host events it reads.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/abi/guc_actions_abi.h` (action codes,
//! `HOST2GUC_SELF_CFG`, `HOST2GUC_CONTROL_CTB`, TLB invalidation operands),
//! `abi/guc_messages_abi.h` (HXG header fields), `abi/guc_klvs_abi.h` (self-config keys),
//! `abi/guc_scheduler_abi.h` (engine classes, `CONTEXT_REGISTRATION_FLAG_*`,
//! `GUC_CONTEXT_ENABLE`), `xe_guc_submit.c` (`__register_exec_queue`,
//! `submit_exec_queue`, `disable_scheduling_deregister`, `deregister_exec_queue`),
//! `xe_guc_tlb_inval.c` (`send_tlb_inval_all`), `xe_guc.c` (`guc_self_cfg`,
//! `xe_guc_mmio_send_recv`) and `xe_guc_ct.c` (CTB layout, fences).
//!
//! Every builder returns the action array exactly as upstream passes it to
//! `xe_guc_ct_send` or `xe_guc_mmio_send`: `action[0]` is the action code, the transport
//! adds its own header.

/// `XE_GUC_ACTION_SCHED_CONTEXT`.
pub const ACTION_SCHED_CONTEXT: u32 = 0x1000;
/// `XE_GUC_ACTION_SCHED_CONTEXT_MODE_SET`.
pub const ACTION_SCHED_CONTEXT_MODE_SET: u32 = 0x1001;
/// `XE_GUC_ACTION_SCHED_CONTEXT_MODE_DONE` (G2H).
pub const ACTION_SCHED_CONTEXT_MODE_DONE: u32 = 0x1002;
/// `XE_GUC_ACTION_CONTEXT_RESET_NOTIFICATION` (G2H).
pub const ACTION_CONTEXT_RESET_NOTIFICATION: u32 = 0x1008;
/// `XE_GUC_ACTION_ENGINE_FAILURE_NOTIFICATION` (G2H).
pub const ACTION_ENGINE_FAILURE_NOTIFICATION: u32 = 0x1009;
/// `XE_GUC_ACTION_REGISTER_CONTEXT`.
pub const ACTION_REGISTER_CONTEXT: u32 = 0x4502;
/// `XE_GUC_ACTION_DEREGISTER_CONTEXT`.
pub const ACTION_DEREGISTER_CONTEXT: u32 = 0x4503;
/// `XE_GUC_ACTION_DEREGISTER_CONTEXT_DONE` (G2H).
pub const ACTION_DEREGISTER_CONTEXT_DONE: u32 = 0x4600;
/// `XE_GUC_ACTION_NOTIFY_MEMORY_CAT_ERROR` (G2H).
pub const ACTION_NOTIFY_MEMORY_CAT_ERROR: u32 = 0x6000;
/// `XE_GUC_ACTION_TLB_INVALIDATION`.
pub const ACTION_TLB_INVALIDATION: u32 = 0x7000;
/// `XE_GUC_ACTION_TLB_INVALIDATION_DONE` (G2H).
pub const ACTION_TLB_INVALIDATION_DONE: u32 = 0x7001;
/// `GUC_ACTION_HOST2GUC_SELF_CFG` (MMIO only).
pub const ACTION_HOST2GUC_SELF_CFG: u32 = 0x0508;
/// `GUC_ACTION_HOST2GUC_CONTROL_CTB` (MMIO only).
pub const ACTION_HOST2GUC_CONTROL_CTB: u32 = 0x4509;

/// `GUC_RENDER_CLASS`.
pub const GUC_RENDER_CLASS: u32 = 0;
/// `GUC_COMPUTE_CLASS`.
pub const GUC_COMPUTE_CLASS: u32 = 4;
/// `GUC_MAX_ENGINE_CLASSES`.
pub const GUC_MAX_ENGINE_CLASSES: usize = 16;
/// `GUC_MAX_INSTANCES_PER_CLASS`.
pub const GUC_MAX_INSTANCES_PER_CLASS: usize = 32;

/// `CONTEXT_REGISTRATION_FLAG_KMD`.
pub const CONTEXT_REGISTRATION_FLAG_KMD: u32 = 1 << 0;
/// `GUC_CONTEXT_NORMAL` in `CONTEXT_REGISTRATION_FLAG_TYPE` (bits 2:1).
pub const GUC_CONTEXT_NORMAL: u32 = 0;
/// `GUC_CONTEXT_DISABLE` / `GUC_CONTEXT_ENABLE`.
pub const GUC_CONTEXT_DISABLE: u32 = 0;
pub const GUC_CONTEXT_ENABLE: u32 = 1;

/// `XE_GUC_TLB_INVAL_FULL`, `XE_GUC_TLB_INVAL_MODE_HEAVY`, `XE_GUC_TLB_INVAL_FLUSH_CACHE`.
pub const TLB_INVAL_FULL: u32 = 0;
pub const TLB_INVAL_MODE_HEAVY: u32 = 0;
pub const TLB_INVAL_FLUSH_CACHE: u32 = 1 << 31;

/// `GUC_KLV_SELF_CFG_*_KEY`.
pub const KLV_H2G_CTB_ADDR: u16 = 0x0902;
pub const KLV_H2G_CTB_DESCRIPTOR_ADDR: u16 = 0x0903;
pub const KLV_H2G_CTB_SIZE: u16 = 0x0904;
pub const KLV_G2H_CTB_ADDR: u16 = 0x0905;
pub const KLV_G2H_CTB_DESCRIPTOR_ADDR: u16 = 0x0906;
pub const KLV_G2H_CTB_SIZE: u16 = 0x0907;

/// `GUC_CTB_CONTROL_DISABLE` / `GUC_CTB_CONTROL_ENABLE`.
pub const CTB_CONTROL_DISABLE: u32 = 0;
pub const CTB_CONTROL_ENABLE: u32 = 1;

/// `CTB_DESC_SIZE`: each descriptor is padded to 2 KiB.
pub const CTB_DESC_SIZE: u32 = 2048;
/// `CTB_H2G_BUFFER_OFFSET` / `CTB_G2H_BUFFER_OFFSET` inside their buffer objects.
pub const CTB_BUFFER_OFFSET: u32 = 2 * CTB_DESC_SIZE;
/// Offset of the G2H descriptor inside the G2H object (`guc_ct_ctb_g2h_init`).
pub const CTB_G2H_DESC_OFFSET: u32 = CTB_DESC_SIZE;
/// `CTB_H2G_BUFFER_SIZE` / `CTB_G2H_BUFFER_SIZE` in bytes.
pub const CTB_H2G_BUFFER_SIZE: u32 = 16 * 1024;
pub const CTB_G2H_BUFFER_SIZE: u32 = 128 * 1024;

/// `CT_SEQNO_UNTRACKED`: fence bit 15 of a message no response is expected for.
pub const CT_SEQNO_UNTRACKED: u16 = 1 << 15;
/// `CT_SEQNO_MASK`.
pub const CT_SEQNO_MASK: u16 = 0x7fff;

/// `GUC_HXG_ORIGIN_GUC` in bit 31 of a message header.
pub const HXG_ORIGIN_GUC: u32 = 1 << 31;

/// `__register_exec_queue` for a single-LRC queue: no work queue, so the WQ fields are 0.
pub fn register_context(guc_id: u32, guc_class: u32, submit_mask: u32, lrc_desc: u64) -> [u32; 12] {
    [
        ACTION_REGISTER_CONTEXT,
        CONTEXT_REGISTRATION_FLAG_KMD | GUC_CONTEXT_NORMAL << 1,
        guc_id,
        guc_class,
        submit_mask,
        0, // wq_desc_lo
        0, // wq_desc_hi
        0, // wq_base_lo
        0, // wq_base_hi
        0, // wq_size
        lrc_desc as u32,
        (lrc_desc >> 32) as u32,
    ]
}

/// `submit_exec_queue` on a queue whose scheduling is not enabled yet.
pub fn sched_context_mode_set(guc_id: u32, enable: bool) -> [u32; 3] {
    let mode = if enable {
        GUC_CONTEXT_ENABLE
    } else {
        GUC_CONTEXT_DISABLE
    };
    [ACTION_SCHED_CONTEXT_MODE_SET, guc_id, mode]
}

/// `submit_exec_queue` on an enabled queue: tell the GuC its ring tail moved.
pub fn sched_context(guc_id: u32) -> [u32; 2] {
    [ACTION_SCHED_CONTEXT, guc_id]
}

/// `deregister_exec_queue`.
pub fn deregister_context(guc_id: u32) -> [u32; 2] {
    [ACTION_DEREGISTER_CONTEXT, guc_id]
}

/// `send_tlb_inval_ppgtt` without range invalidation: a full, heavy invalidation that also
/// flushes the caches (`MAKE_INVAL_OP(XE_GUC_TLB_INVAL_FULL)`).
pub fn tlb_invalidation_full(seqno: u32) -> [u32; 3] {
    [
        ACTION_TLB_INVALIDATION,
        seqno,
        TLB_INVAL_FULL | TLB_INVAL_MODE_HEAVY << 8 | TLB_INVAL_FLUSH_CACHE,
    ]
}

/// `guc_self_cfg`: an MMIO `HOST2GUC_SELF_CFG` request carrying one KLV of `len` dwords
/// (1 or 2).
pub fn self_cfg(key: u16, len: u16, value: u64) -> [u32; 4] {
    [
        ACTION_HOST2GUC_SELF_CFG,
        u32::from(key) << 16 | u32::from(len),
        value as u32,
        (value >> 32) as u32,
    ]
}

/// `guc_ct_control_toggle`.
pub fn control_ctb(enable: bool) -> [u32; 2] {
    let control = if enable {
        CTB_CONTROL_ENABLE
    } else {
        CTB_CONTROL_DISABLE
    };
    [ACTION_HOST2GUC_CONTROL_CTB, control]
}

/// The GuC's answer in `VF_SW_FLAG(0)` to an MMIO request (`xe_guc_mmio_send_recv`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmioReply {
    /// Bit 31 still clear: the GuC has not answered.
    Pending,
    /// `RESPONSE_SUCCESS` with `DATA0` (27:0).
    Success(u32),
    /// `NO_RESPONSE_BUSY`: keep waiting for the final answer.
    Busy,
    /// `NO_RESPONSE_RETRY`: send the request again.
    Retry,
    /// `RESPONSE_FAILURE`: `HINT` 27:16, `ERROR` 15:0.
    Failure { hint: u32, error: u32 },
    /// A type a reply never carries.
    Unexpected(u32),
}

/// Decode a reply header (`GUC_HXG_MSG_0_*`).
pub fn decode_mmio_reply(header: u32) -> MmioReply {
    if header & HXG_ORIGIN_GUC == 0 {
        return MmioReply::Pending;
    }
    match header >> 28 & 7 {
        7 => MmioReply::Success(header & 0x0fff_ffff),
        3 => MmioReply::Busy,
        5 => MmioReply::Retry,
        6 => MmioReply::Failure {
            hint: header >> 16 & 0xfff,
            error: header & 0xffff,
        },
        other => MmioReply::Unexpected(other),
    }
}

/// A G2H event the driver acts on (`process_g2h_msg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum G2hEvent {
    /// `SCHED_CONTEXT_MODE_DONE`: `{guc_id, runnable_state}`.
    SchedDone { guc_id: u32, enabled: bool },
    /// `DEREGISTER_CONTEXT_DONE`: `{guc_id}`.
    DeregisterDone { guc_id: u32 },
    /// `CONTEXT_RESET_NOTIFICATION`: the GuC reset this context after a hang.
    ContextReset { guc_id: u32 },
    /// `NOTIFY_MEMORY_CAT_ERROR`: the context faulted on an unmapped address.
    CatError { guc_id: u32 },
    /// `ENGINE_FAILURE_NOTIFICATION`: `{guc_class, instance, reason}`.
    EngineFailure { guc_class: u32, instance: u32 },
    /// `TLB_INVALIDATION_DONE`: `{seqno}`.
    TlbInvalidationDone { seqno: u32 },
    /// Any other event or a malformed one; `xe` logs these.
    Other { action: u32 },
}

/// Decode one G2H message's HXG part (`hxg[0]` header, then payload). Non-event messages
/// (responses to requests the driver never sends) decode as `Other`.
pub fn decode_g2h(hxg: &[u32]) -> G2hEvent {
    let Some(&header) = hxg.first() else {
        return G2hEvent::Other { action: 0 };
    };
    let action = header & 0xffff;
    let payload = &hxg[1..];
    // `GUC_HXG_TYPE_EVENT` = 1.
    if header >> 28 & 7 != 1 {
        return G2hEvent::Other { action };
    }
    let at = |i: usize| payload.get(i).copied();
    let event = match action {
        ACTION_SCHED_CONTEXT_MODE_DONE => {
            at(0).zip(at(1)).map(|(guc_id, state)| G2hEvent::SchedDone {
                guc_id,
                enabled: state == GUC_CONTEXT_ENABLE,
            })
        }
        ACTION_DEREGISTER_CONTEXT_DONE => at(0).map(|guc_id| G2hEvent::DeregisterDone { guc_id }),
        ACTION_CONTEXT_RESET_NOTIFICATION => at(0).map(|guc_id| G2hEvent::ContextReset { guc_id }),
        ACTION_NOTIFY_MEMORY_CAT_ERROR => at(0).map(|guc_id| G2hEvent::CatError { guc_id }),
        ACTION_ENGINE_FAILURE_NOTIFICATION => {
            at(0)
                .zip(at(1))
                .map(|(guc_class, instance)| G2hEvent::EngineFailure {
                    guc_class,
                    instance,
                })
        }
        ACTION_TLB_INVALIDATION_DONE => at(0).map(|seqno| G2hEvent::TlbInvalidationDone { seqno }),
        _ => None,
    };
    event.unwrap_or(G2hEvent::Other { action })
}

#[cfg(test)]
#[path = "guc_abi/tests.rs"]
mod tests;
