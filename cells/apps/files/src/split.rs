// SPDX-License-Identifier: MIT
//! Split view: a second folder pane beside the first, Tab to move focus between them,
//! F5/F6 to copy/move the focused selection into the other pane's folder.
//!
//! The focused pane always lives in `FilesApp::{current_path, items, selected_index}`, so
//! every single-pane operation (keys, paste, Quick Look, Places) acts on it unchanged; the
//! other pane is parked in `secondary_pane`. `active_pane` records which side of the
//! window the focused pane is drawn on.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::app::FilesApp;
use crate::fs::{self, FileSystem};
use crate::layout;
use crate::model::{resolve_conflict_name, FileItem};

/// Side of the window a pane is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePane {
    #[default]
    Left,
    Right,
}

/// The unfocused pane's folder, listing and selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneState {
    pub current_path: String,
    pub items: Vec<FileItem>,
    pub selected_index: usize,
}

/// One pane as the renderer draws it.
#[derive(Debug, Clone, Copy)]
pub struct PaneView<'a> {
    pub path: &'a str,
    pub items: &'a [FileItem],
    pub selected_index: usize,
    pub focused: bool,
}

impl<F: FileSystem> FilesApp<F> {
    /// Whether the second pane is shown.
    pub fn is_split(&self) -> bool {
        self.secondary_pane.is_some()
    }

    /// Left pane and, when split, right pane, in window order.
    pub fn pane_views(&self) -> (PaneView<'_>, Option<PaneView<'_>>) {
        let focused = PaneView {
            path: &self.current_path,
            items: &self.items,
            selected_index: self.selected_index,
            focused: true,
        };
        let Some(other) = &self.secondary_pane else {
            return (focused, None);
        };
        let other = PaneView {
            path: &other.current_path,
            items: &other.items,
            selected_index: other.selected_index,
            focused: false,
        };
        match self.active_pane {
            ActivePane::Left => (focused, Some(other)),
            ActivePane::Right => (other, Some(focused)),
        }
    }

    /// F3 / Ctrl+E / the toolbar Split button: open a second pane, or close it.
    pub fn toggle_split_pane(&mut self) {
        if self.is_split() {
            self.detach_pane();
        } else {
            // A different folder than the current one, so the two panes are told apart.
            let path = if self.current_path != "/data" {
                "/data"
            } else {
                "/"
            };
            self.attach_pane(path);
        }
    }

    /// Show `path` in the second pane. Unsplit, it opens on the right with focus kept on
    /// the left (unsplit, `active_pane` is always `Left`); split, it replaces the
    /// unfocused pane. A refused listing leaves the view as it was and reports why.
    pub fn attach_pane(&mut self, path: &str) -> bool {
        let items = match self.read_listing(path) {
            Ok(items) => items,
            Err(e) => {
                self.status_message = format!("Cannot open {}: {}", path, e.describe());
                return false;
            }
        };
        self.secondary_pane = Some(PaneState {
            current_path: String::from(path),
            items,
            selected_index: 0,
        });
        self.status_message = format!(
            "Split view: {} (Tab: switch pane, F5: copy, F6: move across)",
            path
        );
        true
    }

    /// Close the unfocused pane; the focused one becomes the single view. Returns the
    /// closed pane's folder.
    pub fn detach_pane(&mut self) -> Option<String> {
        let closed = self.secondary_pane.take()?;
        self.active_pane = ActivePane::Left;
        self.status_message = format!("Closed split pane {}", closed.current_path);
        Some(closed.current_path)
    }

    /// Tab: move focus to the other pane.
    pub fn switch_active_pane(&mut self) {
        let Some(other) = self.secondary_pane.take() else {
            return;
        };
        let previous = PaneState {
            current_path: core::mem::replace(&mut self.current_path, other.current_path),
            items: core::mem::replace(&mut self.items, other.items),
            selected_index: core::mem::replace(&mut self.selected_index, other.selected_index),
        };
        self.secondary_pane = Some(previous);
        self.active_pane = match self.active_pane {
            ActivePane::Left => ActivePane::Right,
            ActivePane::Right => ActivePane::Left,
        };
        self.status_message = format!(
            "Active pane: {:?} ({})",
            self.active_pane, self.current_path
        );
    }

    /// F5 (copy) / F6 (`cut`: move) the focused selection into the other pane's folder.
    /// Refused while unsplit: the target folder must be on screen.
    pub fn copy_to_other_pane(&mut self, cut: bool) {
        let Some(target_dir) = self.secondary_pane.as_ref().map(|s| s.current_path.clone()) else {
            self.status_message = String::from("No second pane: press F3 to split the view");
            return;
        };
        let Some(item) = self.selected_entry() else {
            return;
        };
        let existing = match self.names_in(&target_dir) {
            Ok(names) => names,
            Err(e) => {
                self.status_message = format!("Operation failed: {}", e.describe());
                return;
            }
        };
        let final_name = resolve_conflict_name(&item.name, &existing);
        let dst = fs::join(&target_dir, &final_name);
        let result = if cut {
            fs::move_entry(&mut self.fs, &item.path, &dst, item.is_dir)
        } else {
            fs::copy_entry(&mut self.fs, &item.path, &dst, item.is_dir)
        };
        match result {
            Ok(()) => {
                let verb = if cut { "Moved" } else { "Copied" };
                self.refresh(None, format!("{} {} -> {}", verb, item.name, dst));
            }
            Err(e) => {
                let status = format!("Operation failed: {}", e.describe());
                // A partial copy may exist; show what the VFS now holds.
                self.refresh(None, status);
            }
        }
    }

    /// List the unfocused pane again after a change made through the focused one (both
    /// may show the same folder). If its folder itself was moved or deleted, it shows the
    /// nearest ancestor that still lists rather than the vanished folder's old entries.
    pub(crate) fn reload_secondary_pane(&mut self) {
        let Some(mut path) = self.secondary_pane.as_ref().map(|p| p.current_path.clone()) else {
            return;
        };
        let listing = loop {
            match self.read_listing(&path) {
                Ok(items) => break items,
                Err(_) if path != "/" => path = String::from(fs::parent(&path)),
                Err(_) => break Vec::new(),
            }
        };
        if let Some(pane) = &mut self.secondary_pane {
            if pane.current_path != path {
                pane.current_path = path;
                pane.selected_index = 0;
            }
            pane.selected_index = pane.selected_index.min(listing.len().saturating_sub(1));
            pane.items = listing;
        }
    }

    /// A left click in the content area: focus the pane under it and select the entry
    /// clicked there. Returns whether the click landed in a pane.
    pub fn click_listing(&mut self, x: i32, y: i32) -> bool {
        let split = self.is_split();
        let Some((side, frame)) = layout::pane_at(self.width, self.height, split, x, y) else {
            return false;
        };
        if side != self.active_pane {
            self.switch_active_pane();
        }
        let rect = layout::listing_rect(frame, split);
        if let Some(idx) = layout::listing_hit(self.view_mode, rect, self.items.len(), x, y) {
            self.selected_index = idx;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fake::FakeFs;
    use crate::model::ViewMode;

    fn app_at(fs: FakeFs, path: &str) -> FilesApp<FakeFs> {
        FilesApp::new(fs, path, 800, 600)
    }

    fn two_folders() -> FakeFs {
        FakeFs::new()
            .dir("/data")
            .file("/data/item.txt", b"payload\n")
            .dir("/mnt")
    }

    fn names(items: &[FileItem]) -> Vec<&str> {
        items.iter().map(|item| item.name.as_str()).collect()
    }

    #[test]
    fn split_pane_toggles_and_copies_across() {
        let mut app = app_at(two_folders(), "/data");
        assert!(!app.is_split());

        app.toggle_split_pane();
        assert!(app.is_split());
        assert_eq!(app.active_pane, ActivePane::Left);
        let (left, right) = app.pane_views();
        assert_eq!((left.path, left.focused), ("/data", true));
        let right = right.expect("right pane shown");
        assert_eq!((right.path, right.focused), ("/", false));

        app.switch_active_pane();
        assert_eq!(app.active_pane, ActivePane::Right);
        assert_eq!(app.current_path, "/");
        // Focus moved; the panes stay on their sides.
        let (left, right) = app.pane_views();
        assert_eq!((left.path, left.focused), ("/data", false));
        assert_eq!(right.map(|r| (r.path, r.focused)), Some(("/", true)));
        app.switch_active_pane();
        assert_eq!(app.active_pane, ActivePane::Left);

        app.selected_index = 1;
        app.copy_to_other_pane(false);
        assert_eq!(app.status_message, "Copied item.txt -> /item.txt");
        assert_eq!(
            app.fs.nodes["/item.txt"].as_deref(),
            Some(&b"payload\n"[..])
        );
        let other = app.secondary_pane.as_ref().unwrap();
        assert_eq!(names(&other.items), ["data", "mnt", "item.txt"]);

        app.toggle_split_pane();
        assert!(!app.is_split());
        assert_eq!(app.status_message, "Closed split pane /");
    }

    #[test]
    fn copy_and_move_are_refused_without_a_visible_second_pane() {
        let mut app = app_at(two_folders(), "/data");
        app.selected_index = 1;
        let before = app.fs.nodes.clone();
        for cut in [false, true] {
            app.copy_to_other_pane(cut);
            assert_eq!(
                app.status_message,
                "No second pane: press F3 to split the view"
            );
            assert_eq!(app.fs.nodes, before, "cut={cut}");
        }
        // Tab without a second pane changes nothing either.
        app.switch_active_pane();
        assert_eq!(app.active_pane, ActivePane::Left);
        assert_eq!(app.current_path, "/data");

        // After closing the split the same keys are refused again.
        app.toggle_split_pane();
        app.toggle_split_pane();
        app.copy_to_other_pane(true);
        assert_eq!(app.fs.nodes, before);
    }

    #[test]
    fn move_across_updates_both_panes() {
        let mut app = app_at(two_folders(), "/data");
        assert!(app.attach_pane("/mnt"));
        app.selected_index = 1;
        app.copy_to_other_pane(true);
        assert_eq!(app.status_message, "Moved item.txt -> /mnt/item.txt");
        assert_eq!(names(&app.items), [".."]);
        let other = app.secondary_pane.as_ref().unwrap();
        assert_eq!(names(&other.items), ["..", "item.txt"]);
    }

    #[test]
    fn attach_and_detach_keep_the_focused_pane() {
        let fs = FakeFs::new().dir("/data").dir("/mnt").dir("/docs");
        let mut app = app_at(fs, "/data");

        assert!(!app.attach_pane("/missing"));
        assert!(!app.is_split());
        assert!(app.status_message.starts_with("Cannot open /missing"));

        assert!(app.attach_pane("/docs"));
        app.switch_active_pane();
        // Split already: attaching replaces the unfocused pane, focus stays right.
        assert!(app.attach_pane("/mnt"));
        assert_eq!(app.active_pane, ActivePane::Right);
        assert_eq!(app.current_path, "/docs");
        assert_eq!(app.secondary_pane.as_ref().unwrap().current_path, "/mnt");

        // Closing keeps the focused folder as the single view.
        assert_eq!(app.detach_pane().as_deref(), Some("/mnt"));
        assert!(!app.is_split());
        assert_eq!(app.active_pane, ActivePane::Left);
        assert_eq!(app.current_path, "/docs");
        assert_eq!(app.detach_pane(), None);
    }

    #[test]
    fn unfocused_pane_follows_changes_and_vanished_folders() {
        let fs = FakeFs::new()
            .dir("/data")
            .dir("/data/sub")
            .file("/data/sub/a.txt", b"a");
        let mut app = app_at(fs, "/data");
        assert!(app.attach_pane("/data/sub"));
        // Trash the folder the right pane shows: it falls back to /data, which now
        // holds .trash instead of sub.
        app.selected_index = 1;
        app.trash_selected();
        let other = app.secondary_pane.as_ref().unwrap();
        assert_eq!(other.current_path, "/data");
        assert_eq!(names(&other.items), ["..", ".trash"]);
    }

    #[test]
    fn clicks_focus_the_pane_under_the_pointer_and_select_in_it() {
        let fs = two_folders().file("/mnt/m1", b"1").file("/mnt/m2", b"2");
        let mut app = app_at(fs, "/data");
        assert!(app.attach_pane("/mnt"));
        // Right pane listing starts at y = 44 + 24 (header) + 28 (column header).
        app.click_listing(600, 96 + 32 * 2 + 5);
        assert_eq!(app.active_pane, ActivePane::Right);
        assert_eq!(app.current_path, "/mnt");
        assert_eq!(app.selected_index, 2);
        assert_eq!(app.items[2].name, "m2");

        // Click into the left pane: focus returns with its own selection untouched…
        assert!(app.click_listing(300, 50));
        assert_eq!(app.active_pane, ActivePane::Left);
        assert_eq!(app.current_path, "/data");
        assert_eq!(app.selected_index, 0);
        // …and the right pane keeps its selection.
        assert_eq!(app.secondary_pane.as_ref().unwrap().selected_index, 2);

        app.click_listing(300, 96 + 32 + 5);
        assert_eq!(app.selected_index, 1);
        assert!(!app.click_listing(479, 200), "divider");
        assert!(!app.click_listing(80, 200), "places");

        // Grid view hit-tests the narrower pane too.
        app.switch_view_mode(ViewMode::Grid);
        app.click_listing(480 + 12 + 116 + 5, 68 + 12 + 5);
        assert_eq!(
            (app.active_pane, app.selected_index),
            (ActivePane::Right, 1)
        );

        // Unsplit, the whole content area is the one listing again.
        app.toggle_split_pane();
        app.switch_view_mode(ViewMode::List);
        app.click_listing(700, 44 + 28 + 5);
        assert_eq!(app.selected_index, 0);
        assert_eq!(app.current_path, "/mnt");
    }
}
