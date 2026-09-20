// SPDX-License-Identifier: MIT
//! Application catalog and state for CellOS Desktop.

extern crate alloc;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AppInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub path: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    pub is_pinned: bool,
}

pub const DEFAULT_CATALOG: &[AppInfo] = &[
    AppInfo {
        id: "terminal",
        name: "Terminal",
        path: "/bin/fb-console",
        icon: ">_",
        description: "Interactive Console",
        is_pinned: true,
    },
    AppInfo {
        id: "ocel",
        name: "Ocel Viewer",
        path: "/bin/ocel",
        icon: "(O)",
        description: "Document & Web Viewer",
        is_pinned: true,
    },
    AppInfo {
        id: "dashboard",
        name: "Robot Dash",
        path: "/bin/robot-dashboard",
        icon: "[#]",
        description: "Telemetry & Controls",
        is_pinned: true,
    },
    AppInfo {
        id: "counter",
        name: "ViUI Counter",
        path: "/bin/viui-demo",
        icon: "+1",
        description: "Reactive UI Demo",
        is_pinned: true,
    },
    AppInfo {
        id: "tetris",
        name: "Tetris",
        path: "/bin/tetris",
        icon: "[T]",
        description: "Block Stacking Game",
        is_pinned: true,
    },
    AppInfo {
        id: "systools",
        name: "Sys Tools",
        path: "/bin/app-sys-tools",
        icon: "$_",
        description: "System Memory & Tasks",
        is_pinned: false,
    },
    AppInfo {
        id: "sensors",
        name: "Sensors",
        path: "/bin/sensor-demo",
        icon: "(s)",
        description: "Hardware Peripheral Prober",
        is_pinned: false,
    },
    AppInfo {
        id: "httpd",
        name: "HTTP Server",
        path: "/bin/service-httpd",
        icon: "://",
        description: "Web Server Daemon",
        is_pinned: false,
    },
    AppInfo {
        id: "calc",
        name: "Calculator",
        path: "/bin/app-c-math-smoke",
        icon: "[%]",
        description: "Math Engine Smoke",
        is_pinned: false,
    },
];

pub struct AppRegistry {
    pub items: Vec<AppInfo>,
}

impl AppRegistry {
    pub fn new() -> Self {
        Self {
            items: DEFAULT_CATALOG.to_vec(),
        }
    }

    pub fn pinned_apps(&self) -> Vec<AppInfo> {
        self.items.iter().copied().filter(|a| a.is_pinned).collect()
    }

    pub fn toggle_pin(&mut self, id: &str) {
        if let Some(app) = self.items.iter_mut().find(|a| a.id == id) {
            app.is_pinned = !app.is_pinned;
        }
    }

    pub fn filtered(&self, query: &str) -> Vec<AppInfo> {
        if query.is_empty() {
            return self.items.clone();
        }
        let q_bytes = query.as_bytes();
        self.items
            .iter()
            .copied()
            .filter(|a| {
                contains_case_insensitive(a.name.as_bytes(), q_bytes)
                    || contains_case_insensitive(a.description.as_bytes(), q_bytes)
                    || contains_case_insensitive(a.id.as_bytes(), q_bytes)
            })
            .collect()
    }
}

fn contains_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle.iter())
            .all(|(&h, &n)| h.eq_ignore_ascii_case(&n))
    })
}
