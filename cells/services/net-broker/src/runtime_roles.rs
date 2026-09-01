/// Maximum scheduler turns for one network-role IPC exchange.
///
/// At the kernel's 10 ms scheduler quantum this is two seconds. A timeout
/// poisons the current beacon connection, so no late reply can be reused.
pub const NETWORK_IPC_TIMEOUT_TICKS: u64 = 200;

/// Maximum elapsed time for all runtime roles to drain during restart.
///
/// This exceeds [`NETWORK_IPC_TIMEOUT_TICKS`] by one second, leaving a bounded
/// scheduling window for a timed-out network role to observe shutdown and exit.
pub const RESTART_DRAIN_TIMEOUT_MS: u64 = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeRole {
    LocalWorker,
    ReplyPump,
    NetworkPoller,
}

impl RuntimeRole {
    pub const ALL: [Self; 3] = [Self::LocalWorker, Self::ReplyPump, Self::NetworkPoller];

    pub const fn name(self) -> &'static str {
        match self {
            Self::LocalWorker => "local-worker",
            Self::ReplyPump => "reply-pump",
            Self::NetworkPoller => "network-poller",
        }
    }
}

pub fn start_runtime_roles<F, E>(mut spawn: F) -> Result<(), RuntimeRole>
where
    F: FnMut(RuntimeRole) -> Result<(), E>,
{
    for role in RuntimeRole::ALL {
        if spawn(role).is_err() {
            return Err(role);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_roles_in_order() {
        let mut seen = [None; 3];
        let mut idx = 0usize;
        start_runtime_roles(|role| {
            seen[idx] = Some(role);
            idx += 1;
            Ok::<(), ()>(())
        })
        .expect("all roles start");
        assert_eq!(
            seen,
            [
                Some(RuntimeRole::LocalWorker),
                Some(RuntimeRole::ReplyPump),
                Some(RuntimeRole::NetworkPoller),
            ]
        );
    }

    #[test]
    fn stops_on_first_failed_role() {
        let mut seen = [None; 2];
        let mut idx = 0usize;
        let failed = start_runtime_roles(|role| {
            if idx < seen.len() {
                seen[idx] = Some(role);
            }
            idx += 1;
            if role == RuntimeRole::ReplyPump {
                Err(())
            } else {
                Ok(())
            }
        });
        assert_eq!(failed, Err(RuntimeRole::ReplyPump));
        assert_eq!(
            seen,
            [Some(RuntimeRole::LocalWorker), Some(RuntimeRole::ReplyPump)]
        );
        assert_eq!(idx, 2);
    }
}
