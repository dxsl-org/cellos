use ostd::io::println;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClosePolicy {
    Never,
    RejectThenAccept,
}

pub(crate) struct ProbeRole {
    pub(crate) name: &'static str,
    pub(crate) title: &'static str,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) color: [u8; 4],
    pub(crate) background: bool,
    pub(crate) restore_after_minimize: bool,
    pub(crate) apply_configures: bool,
    pub(crate) close_policy: ClosePolicy,
}

pub(crate) fn parse_role() -> Option<ProbeRole> {
    let role = match ostd::args().first().map(|arg| arg.as_str()) {
        Some("back") => (
            "back",
            "Back",
            80,
            80,
            [0x00, 0x00, 0xFF, 0xFF],
            false,
            false,
            true,
            ClosePolicy::Never,
        ),
        Some("front") => (
            "front",
            "Front",
            160,
            120,
            [0xFF, 0x00, 0x00, 0xFF],
            false,
            false,
            true,
            ClosePolicy::Never,
        ),
        Some("background") => (
            "background",
            "",
            0,
            0,
            [0x00, 0xFF, 0x00, 0xFF],
            true,
            false,
            false,
            ClosePolicy::Never,
        ),
        Some("wm-primary") => (
            "wm-primary",
            "Primary",
            400,
            100,
            [0xFF, 0x00, 0xFF, 0xFF],
            false,
            true,
            true,
            ClosePolicy::Never,
        ),
        Some("wm-silent") => (
            "wm-silent",
            "Silent",
            400,
            300,
            [0xFF, 0xFF, 0x00, 0xFF],
            false,
            false,
            false,
            ClosePolicy::Never,
        ),
        Some("wm-close") => (
            "wm-close",
            "Close",
            600,
            100,
            [0x00, 0xFF, 0xFF, 0xFF],
            false,
            false,
            true,
            ClosePolicy::RejectThenAccept,
        ),
        _ => return None,
    };
    Some(ProbeRole {
        name: role.0,
        title: role.1,
        x: role.2,
        y: role.3,
        color: role.4,
        background: role.5,
        restore_after_minimize: role.6,
        apply_configures: role.7,
        close_policy: role.8,
    })
}

pub(crate) fn print_usage() {
    println("[window-policy-probe] usage: window-policy-probe back|front|background|wm-primary|wm-silent|wm-close");
}
