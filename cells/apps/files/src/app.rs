// SPDX-License-Identifier: MIT
//! Files state and operations. Every listing and every change goes to the VFS through
//! [`FileSystem`]; after a change the folder is listed again, so the view shows what the
//! service holds, and a refusal is shown as the VFS reported it.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::{self, FileSystem, FsResult, PREVIEW_READ_MAX};
use crate::model::{
    resolve_conflict_name, FileItem, PlacesItem, TrashRecord, ViewMode, DEFAULT_PLACES,
};
use crate::preview::{PreviewKind, QuickLookState};
use crate::split::{ActivePane, PaneState};

/// Longest text the path/name prompt accepts (one FAT long name).
pub const MAX_INPUT_LEN: usize = 255;
/// Name of the synthetic parent entry at the top of every folder except `/`.
pub const PARENT_ENTRY: &str = "..";
/// Folder that Delete moves entries into and Undo restores them from.
pub const TRASH_DIR: &str = "/data/.trash";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub cut: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    GoTo,
    NewFolder,
    NewFile,
}

impl PromptKind {
    const fn label(self) -> &'static str {
        match self {
            Self::GoTo => "Go to",
            Self::NewFolder => "New folder name",
            Self::NewFile => "New file name",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub text: String,
}

pub struct FilesApp<F: FileSystem> {
    pub fs: F,
    pub current_path: String,
    pub view_mode: ViewMode,
    pub items: Vec<FileItem>,
    pub selected_index: usize,
    pub places: Vec<PlacesItem>,
    pub clipboard: Option<Clip>,
    pub prompt: Option<Prompt>,
    pub status_message: String,
    pub width: u32,
    pub height: u32,
    pub quick_look: QuickLookState,
    pub pending_intent: Option<api::services::mime::LaunchIntent>,
    pub trash_stack: Vec<TrashRecord>,
    pub active_pane: ActivePane,
    pub secondary_pane: Option<PaneState>,
}

impl<F: FileSystem> FilesApp<F> {
    pub fn new(fs: F, initial_path: &str, width: u32, height: u32) -> Self {
        let mut app = Self {
            fs,
            current_path: String::from(initial_path),
            view_mode: ViewMode::List,
            items: Vec::new(),
            selected_index: 0,
            places: DEFAULT_PLACES.to_vec(),
            clipboard: None,
            prompt: None,
            status_message: String::new(),
            width,
            height,
            quick_look: QuickLookState::new(),
            pending_intent: None,
            trash_stack: Vec::new(),
            active_pane: ActivePane::Left,
            secondary_pane: None,
        };
        app.load_directory(initial_path);
        app
    }

    pub(crate) fn read_listing(&mut self, path: &str) -> FsResult<Vec<FileItem>> {
        let mut entries = self.fs.list_dir(path)?;
        // Folders first; the VFS order is kept within each group.
        entries.sort_by_key(|entry| !entry.is_dir);
        let mut items = Vec::with_capacity(entries.len() + 1);
        if path != "/" {
            items.push(FileItem::new(PARENT_ENTRY, fs::parent(path), true, None));
        }
        for entry in entries {
            let entry_path = fs::join(path, &entry.name);
            let size = if entry.is_dir {
                None
            } else {
                self.fs.stat(&entry_path).ok().map(|(size, _)| size)
            };
            items.push(FileItem::new(entry.name, entry_path, entry.is_dir, size));
        }
        Ok(items)
    }

    fn summary(&self) -> String {
        let count = self
            .items
            .iter()
            .filter(|item| item.name != PARENT_ENTRY)
            .count();
        format!("{}: {} item(s)", self.current_path, count)
    }

    /// Show `path` as the VFS lists it. A refused or failed listing keeps the current view
    /// and puts the reason in the status bar; it never shows an empty stand-in.
    pub fn load_directory(&mut self, path: &str) -> bool {
        match self.read_listing(path) {
            Ok(items) => {
                self.current_path = String::from(path);
                self.items = items;
                self.selected_index = 0;
                self.status_message = self.summary();
                true
            }
            Err(e) => {
                self.status_message = format!("Cannot open {}: {}", path, e.describe());
                false
            }
        }
    }

    /// List the current folder again after a change, select `select` (or keep the old row
    /// index) and report `status`. The other split pane is listed again too: the change
    /// may have touched the folder it shows.
    pub(crate) fn refresh(&mut self, select: Option<&str>, status: String) {
        self.reload_secondary_pane();
        let previous = self.selected_index;
        let path = self.current_path.clone();
        if !self.load_directory(&path) {
            self.status_message = format!("{}. {}", status, self.status_message);
            return;
        }
        self.selected_index = select
            .and_then(|name| self.items.iter().position(|item| item.name == name))
            .unwrap_or_else(|| previous.min(self.items.len().saturating_sub(1)));
        self.status_message = status;
    }

    pub(crate) fn selected_entry(&self) -> Option<FileItem> {
        self.items
            .get(self.selected_index)
            .filter(|item| item.name != PARENT_ENTRY)
            .cloned()
    }

    pub fn navigate_up(&mut self) {
        if self.current_path != "/" {
            let parent = String::from(fs::parent(&self.current_path));
            self.load_directory(&parent);
        }
    }

    /// Enter a folder, or stage a LaunchIntent for a file.
    pub fn open_selected(&mut self) {
        let Some(item) = self.items.get(self.selected_index).cloned() else {
            return;
        };
        if item.is_dir {
            self.load_directory(&item.path);
        } else {
            let intent = api::services::mime::AssociationRegistry::resolve_intent(&item.path, None);
            self.status_message = format!(
                "{} {} with {}",
                intent.action.as_str(),
                item.name,
                intent.app_id
            );
            self.pending_intent = Some(intent);
        }
    }

    /// Open selected file with a specific candidate application ID.
    pub fn open_selected_with(&mut self, app_id: &str) {
        let Some(item) = self.items.get(self.selected_index).cloned() else {
            return;
        };
        if !item.is_dir {
            if let Some(intent) = api::services::mime::AssociationRegistry::resolve_intent_with_app(
                &item.path, app_id, None,
            ) {
                self.status_message = format!(
                    "{} {} with {}",
                    intent.action.as_str(),
                    item.name,
                    intent.app_id
                );
                self.pending_intent = Some(intent);
            }
        }
    }

    pub fn take_launch_intent(&mut self) -> Option<api::services::mime::LaunchIntent> {
        self.pending_intent.take()
    }

    /// Open Quick Look on the selection; text kinds are read through the VFS (bounded).
    pub fn preview_selected(&mut self) {
        let Some(item) = self.items.get(self.selected_index).cloned() else {
            return;
        };
        let text = PreviewKind::of(&item)
            .is_text()
            .then(|| fs::read_file(&mut self.fs, &item.path, PREVIEW_READ_MAX));
        self.quick_look.show(&item, text);
    }

    pub fn copy_selected(&mut self, cut: bool) {
        let Some(item) = self.selected_entry() else {
            return;
        };
        self.status_message = format!("{} {}", if cut { "Cut" } else { "Copied" }, item.name);
        self.clipboard = Some(Clip {
            path: item.path,
            name: item.name,
            is_dir: item.is_dir,
            cut,
        });
    }

    /// Copy (or move, after Cut) the clipboard entry into the current folder under a free name.
    pub fn paste_into_current(&mut self) {
        let Some(clip) = self.clipboard.take() else {
            return;
        };
        let existing: Vec<String> = self
            .items
            .iter()
            .filter(|item| item.name != PARENT_ENTRY)
            .map(|item| item.name.clone())
            .collect();
        let final_name = resolve_conflict_name(&clip.name, &existing);
        let dst = fs::join(&self.current_path, &final_name);
        let result = if clip.cut {
            fs::move_entry(&mut self.fs, &clip.path, &dst, clip.is_dir)
        } else {
            fs::copy_entry(&mut self.fs, &clip.path, &dst, clip.is_dir)
        };
        match result {
            Ok(()) => {
                let verb = if clip.cut { "Moved" } else { "Pasted" };
                let status = format!("{} {} -> {}", verb, clip.name, final_name);
                if !clip.cut {
                    self.clipboard = Some(clip);
                }
                self.refresh(Some(&final_name), status);
            }
            Err(e) => {
                let status = format!("Paste of {} failed: {}", clip.name, e.describe());
                self.clipboard = Some(clip);
                // A partial copy may exist; show what the VFS now holds.
                self.refresh(None, status);
            }
        }
    }

    pub fn delete_selected(&mut self) {
        let Some(item) = self.selected_entry() else {
            return;
        };
        match fs::remove_entry(&mut self.fs, &item.path, item.is_dir) {
            Ok(()) => self.refresh(
                None,
                format!("Permanently deleted {} (irreversible)", item.name),
            ),
            Err(e) => {
                self.status_message = format!("Cannot delete {}: {}", item.name, e.describe())
            }
        }
    }

    /// Names in `dir`, straight from the VFS. `write` replaces an existing file, so a
    /// free name must never be picked from a stale or failed listing.
    pub(crate) fn names_in(&mut self, dir: &str) -> FsResult<Vec<String>> {
        Ok(self
            .fs
            .list_dir(dir)?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    /// Move the selected entry to Trash (`/data/.trash`), preserving it for Undo.
    pub fn trash_selected(&mut self) {
        let Some(item) = self.selected_entry() else {
            return;
        };
        // Inside the trash (not merely a sibling such as `.trash-old`) Delete is permanent.
        if self.current_path == TRASH_DIR
            || self
                .current_path
                .strip_prefix(TRASH_DIR)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            self.delete_selected();
            return;
        }
        let _ = self.fs.mkdir(TRASH_DIR);
        // No Trash (e.g. `/data` not mounted): say so and name the permanent path
        // instead of reporting a bare VFS error for the item.
        let trash_existing = match self.names_in(TRASH_DIR) {
            Ok(names) => names,
            Err(e) => {
                self.status_message = format!(
                    "Trash unavailable ({}): {} kept; Shift+Delete deletes permanently",
                    e.describe(),
                    item.name
                );
                return;
            }
        };
        let target_name = resolve_conflict_name(&item.name, &trash_existing);
        let trashed_path = fs::join(TRASH_DIR, &target_name);
        match fs::move_entry(&mut self.fs, &item.path, &trashed_path, item.is_dir) {
            Ok(()) => {
                let status = format!("Moved {} to Trash (Ctrl+Z to Undo)", item.name);
                self.trash_stack.push(TrashRecord {
                    name: item.name,
                    original_path: item.path,
                    trashed_path,
                    is_dir: item.is_dir,
                });
                self.refresh(None, status);
            }
            Err(e) => {
                self.status_message =
                    format!("Cannot move {} to Trash: {}", item.name, e.describe());
            }
        }
    }

    /// Restore the most recently trashed item to its original location.
    pub fn undo_last_trash(&mut self) {
        let Some(record) = self.trash_stack.pop() else {
            self.status_message = String::from("Nothing to undo in Trash");
            return;
        };
        // Purged or renamed in Trash since: this record can never be restored and must
        // not block every older one.
        if self.fs.stat(&record.trashed_path).is_err() {
            self.status_message = format!("{} is no longer in Trash", record.name);
            return;
        }
        let parent = fs::parent(&record.original_path);
        let dest_existing = match self.names_in(parent) {
            Ok(names) => names,
            Err(e) => {
                self.status_message = format!("Cannot restore {}: {}", record.name, e.describe());
                self.trash_stack.push(record);
                return;
            }
        };
        let restored_name = resolve_conflict_name(&record.name, &dest_existing);
        let restored_path = fs::join(parent, &restored_name);
        match fs::move_entry(
            &mut self.fs,
            &record.trashed_path,
            &restored_path,
            record.is_dir,
        ) {
            Ok(()) => {
                self.refresh(
                    Some(&restored_name),
                    format!("Restored {} from Trash", record.name),
                );
            }
            Err(e) => {
                let status = format!("Cannot restore {}: {}", record.name, e.describe());
                self.trash_stack.push(record);
                self.status_message = status;
            }
        }
    }

    #[allow(dead_code)]
    pub fn can_undo_trash(&self) -> bool {
        !self.trash_stack.is_empty()
    }

    pub fn switch_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.status_message = format!("View mode: {}", mode.name());
    }

    fn show_prompt(&mut self) {
        if let Some(prompt) = &self.prompt {
            self.status_message = format!("{}: {}_", prompt.kind.label(), prompt.text);
        }
    }

    pub fn begin_prompt(&mut self, kind: PromptKind) {
        self.prompt = Some(Prompt {
            kind,
            text: String::new(),
        });
        self.show_prompt();
    }

    pub fn prompt_input(&mut self, ch: char) {
        if let Some(prompt) = &mut self.prompt {
            if !ch.is_control() && prompt.text.len() + ch.len_utf8() <= MAX_INPUT_LEN {
                prompt.text.push(ch);
            }
        }
        self.show_prompt();
    }

    pub fn prompt_backspace(&mut self) {
        if let Some(prompt) = &mut self.prompt {
            prompt.text.pop();
        }
        self.show_prompt();
    }

    pub fn prompt_cancel(&mut self) {
        if self.prompt.take().is_some() {
            self.status_message = self.summary();
        }
    }

    pub fn prompt_commit(&mut self) {
        let Some(Prompt { kind, text }) = self.prompt.take() else {
            return;
        };
        if kind == PromptKind::GoTo {
            let target = fs::resolve(&self.current_path, &text);
            self.load_directory(&target);
            return;
        }
        let what = if kind == PromptKind::NewFolder {
            "folder"
        } else {
            "file"
        };
        if let Err(why) = fs::validate_name(&text) {
            self.status_message = format!("Cannot create {}: {}", what, why);
            return;
        }
        let path = fs::join(&self.current_path, &text);
        let result = if kind == PromptKind::NewFolder {
            self.fs.mkdir(&path).map_err(Into::into)
        } else {
            fs::create_file(&mut self.fs, &path)
        };
        match result {
            Ok(()) => self.refresh(Some(&text), format!("Created {} {}", what, text)),
            Err(e) => {
                self.status_message = format!("Cannot create {} {}: {}", what, text, e.describe())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fake::FakeFs;

    fn app_at(fs: FakeFs, path: &str) -> FilesApp<FakeFs> {
        FilesApp::new(fs, path, 800, 600)
    }

    fn names(app: &FilesApp<FakeFs>) -> Vec<&str> {
        app.items.iter().map(|item| item.name.as_str()).collect()
    }

    fn type_prompt(app: &mut FilesApp<FakeFs>, kind: PromptKind, text: &str) {
        app.begin_prompt(kind);
        for ch in text.chars() {
            app.prompt_input(ch);
        }
        app.prompt_commit();
    }

    fn c5_tree() -> FakeFs {
        FakeFs::new()
            .dir("/tmp")
            .dir("/tmp/c5dir")
            .file("/tmp/c5dir/a.txt", b"c5\n")
    }

    #[test]
    fn listing_comes_from_the_vfs_with_sizes_and_parent_entry() {
        let fs = c5_tree().dir("/tmp/c5dir/sub");
        let app = app_at(fs, "/tmp/c5dir");
        assert_eq!(names(&app), ["..", "sub", "a.txt"]);
        assert_eq!(app.items[0].path, "/tmp");
        assert_eq!(app.items[2].size, Some(3));
        assert_eq!(app.status_message, "/tmp/c5dir: 2 item(s)");
        assert_eq!(names(&app_at(c5_tree(), "/")), ["tmp"]);
    }

    #[test]
    fn refused_listing_keeps_the_view_and_shows_the_denial() {
        let mut fs = c5_tree()
            .dir("/srv")
            .dir("/srv/cellos")
            .file("/srv/cellos/key", b"k");
        fs.denied.push("/srv/cellos");
        let mut app = app_at(fs, "/tmp/c5dir");
        type_prompt(&mut app, PromptKind::GoTo, "/srv/cellos");
        assert_eq!(
            app.status_message,
            "Cannot open /srv/cellos: permission denied (VFS ERR_DENIED)"
        );
        assert_eq!(app.current_path, "/tmp/c5dir");
        assert_eq!(names(&app), ["..", "a.txt"]);

        // A missing initial folder is an error with nothing listed, not a sample listing.
        let app = app_at(FakeFs::new(), "/data");
        assert!(app.items.is_empty());
        assert_eq!(
            app.status_message,
            "Cannot open /data: I/O error (VFS ERR_IO)"
        );
    }

    #[test]
    fn c5_flow_create_copy_delete_round_trips_through_the_vfs() {
        let mut app = app_at(FakeFs::new().dir("/data"), "/data");
        type_prompt(&mut app, PromptKind::GoTo, "/tmp/c5dir");
        assert!(app.status_message.starts_with("Cannot open /tmp/c5dir"));
        app.fs = c5_tree();
        type_prompt(&mut app, PromptKind::GoTo, "/tmp/c5dir");
        assert_eq!(app.status_message, "/tmp/c5dir: 1 item(s)");

        type_prompt(&mut app, PromptKind::NewFolder, "c5new");
        assert_eq!(app.status_message, "Created folder c5new");
        assert_eq!(names(&app), ["..", "c5new", "a.txt"]);
        assert_eq!(app.selected_index, 1);
        assert_eq!(app.fs.nodes.get("/tmp/c5dir/c5new"), Some(&None));

        app.selected_index = 2;
        app.copy_selected(false);
        assert_eq!(app.status_message, "Copied a.txt");
        app.selected_index = 1;
        app.open_selected();
        assert_eq!(app.current_path, "/tmp/c5dir/c5new");
        assert_eq!(names(&app), [".."]);
        app.paste_into_current();
        assert_eq!(app.status_message, "Pasted a.txt -> a.txt");
        assert_eq!(names(&app), ["..", "a.txt"]);
        assert_eq!(app.selected_index, 1);
        assert_eq!(
            app.fs.nodes["/tmp/c5dir/c5new/a.txt"].as_deref(),
            Some(&b"c5\n"[..])
        );

        app.delete_selected();
        assert_eq!(
            app.status_message,
            "Permanently deleted a.txt (irreversible)"
        );
        assert_eq!(names(&app), [".."]);
        assert!(!app.fs.nodes.contains_key("/tmp/c5dir/c5new/a.txt"));
        app.navigate_up();
        app.selected_index = 1;
        app.delete_selected();
        assert_eq!(names(&app), ["..", "a.txt"]);
        assert!(!app.fs.nodes.contains_key("/tmp/c5dir/c5new"));
    }

    #[test]
    fn paste_resolves_conflicts_and_cut_moves() {
        let mut app = app_at(c5_tree().dir("/tmp/other"), "/tmp/c5dir");
        app.selected_index = 1;
        app.copy_selected(false);
        app.paste_into_current();
        assert_eq!(app.status_message, "Pasted a.txt -> a (1).txt");
        assert_eq!(
            app.fs.nodes["/tmp/c5dir/a (1).txt"].as_deref(),
            Some(&b"c5\n"[..])
        );
        assert!(app.clipboard.is_some());

        app.selected_index = app.items.iter().position(|i| i.name == "a.txt").unwrap();
        app.copy_selected(true);
        type_prompt(&mut app, PromptKind::GoTo, "../other");
        app.paste_into_current();
        assert_eq!(app.status_message, "Moved a.txt -> a.txt");
        assert!(app.clipboard.is_none());
        assert!(!app.fs.nodes.contains_key("/tmp/c5dir/a.txt"));
        assert_eq!(
            app.fs.nodes["/tmp/other/a.txt"].as_deref(),
            Some(&b"c5\n"[..])
        );
    }

    #[test]
    fn refused_mutations_are_reported_and_change_nothing() {
        let mut fs = c5_tree().dir("/bin").file("/bin/tool", b"elf");
        fs.read_only.push("/bin");
        let mut app = app_at(fs, "/bin");
        type_prompt(&mut app, PromptKind::NewFolder, "x");
        assert_eq!(
            app.status_message,
            "Cannot create folder x: permission denied (VFS ERR_DENIED)"
        );
        type_prompt(&mut app, PromptKind::NewFile, "y");
        assert_eq!(
            app.status_message,
            "Cannot create file y: permission denied (VFS ERR_DENIED)"
        );
        app.selected_index = 1;
        app.delete_selected();
        assert_eq!(
            app.status_message,
            "Cannot delete tool: permission denied (VFS ERR_DENIED)"
        );
        assert!(app.fs.nodes.contains_key("/bin/tool"));

        app.copy_selected(true);
        type_prompt(&mut app, PromptKind::GoTo, "/tmp");
        app.paste_into_current();
        // The copy landed, the source could not be removed: both are visible and reported.
        assert_eq!(
            app.status_message,
            "Paste of tool failed: permission denied (VFS ERR_DENIED)"
        );
        assert!(app.fs.nodes.contains_key("/bin/tool"));
        assert_eq!(names(&app), ["..", "c5dir", "tool"]);
        assert!(app.clipboard.as_ref().is_some_and(|clip| clip.cut));

        // A non-empty folder outside /data is not removed recursively (`rmdir` scope).
        type_prompt(&mut app, PromptKind::GoTo, "/tmp");
        app.selected_index = 1;
        app.delete_selected();
        assert_eq!(
            app.status_message,
            "Cannot delete c5dir: I/O error (VFS ERR_IO)"
        );
        assert!(app.fs.nodes.contains_key("/tmp/c5dir/a.txt"));
    }

    #[test]
    fn prompt_validates_names_and_bounds_input() {
        let mut app = app_at(c5_tree(), "/tmp/c5dir");
        type_prompt(&mut app, PromptKind::NewFolder, "a/b");
        assert_eq!(
            app.status_message,
            "Cannot create folder: a name cannot contain '/'"
        );
        type_prompt(&mut app, PromptKind::NewFile, "a.txt");
        assert_eq!(
            app.status_message,
            "Cannot create file a.txt: already exists"
        );
        type_prompt(&mut app, PromptKind::NewFile, "b.txt");
        assert_eq!(app.status_message, "Created file b.txt");
        assert_eq!(app.fs.nodes["/tmp/c5dir/b.txt"].as_deref(), Some(&b""[..]));

        app.begin_prompt(PromptKind::NewFolder);
        for _ in 0..MAX_INPUT_LEN + 10 {
            app.prompt_input('x');
        }
        assert_eq!(app.prompt.as_ref().unwrap().text.len(), MAX_INPUT_LEN);
        app.prompt_backspace();
        assert_eq!(
            app.status_message.len(),
            "New folder name: ".len() + MAX_INPUT_LEN - 1 + 1
        );
        app.prompt_cancel();
        assert!(app.prompt.is_none());
        assert_eq!(app.status_message, "/tmp/c5dir: 2 item(s)");
    }

    #[test]
    fn quick_look_reads_the_selected_file() {
        let mut app = app_at(c5_tree(), "/tmp/c5dir");
        app.selected_index = 1;
        app.preview_selected();
        assert!(app.quick_look.is_open);
        assert_eq!(app.quick_look.lines, ["c5"]);
    }

    #[test]
    fn open_selected_stages_launch_intent() {
        let mut app = app_at(c5_tree(), "/tmp/c5dir");
        app.selected_index = 1;
        app.open_selected();
        let intent = app.take_launch_intent().expect("staged launch intent");
        assert_eq!(intent.target_path, "/tmp/c5dir/a.txt");
        assert_eq!(intent.action, api::services::mime::FileAction::Open);
        assert_eq!(intent.app_id, "terminal");
    }
    #[test]
    fn delete_without_a_trash_volume_keeps_the_file_and_names_shift_delete() {
        // c5_tree has /tmp but no /data, like an image without a data disk.
        let mut app = app_at(c5_tree(), "/tmp/c5dir");
        app.selected_index = 1;
        app.trash_selected();
        assert!(app.fs.stat("/tmp/c5dir/a.txt").is_ok());
        assert!(!app.can_undo_trash());
        assert!(app.status_message.starts_with("Trash unavailable"), "{}", app.status_message);
        assert!(app.status_message.contains("Shift+Delete"));
    }

    #[test]
    fn trash_and_undo_restores_file() {
        let fs = FakeFs::new()
            .dir("/data")
            .file("/data/memo.txt", b"important\n");
        let mut app = app_at(fs, "/data");
        assert_eq!(names(&app), ["..", "memo.txt"]);
        app.selected_index = 1;
        assert!(!app.can_undo_trash());

        // Trash the file
        app.trash_selected();
        assert!(app.can_undo_trash());
        assert_eq!(names(&app), ["..", ".trash"]);
        assert!(app.status_message.contains("Moved memo.txt to Trash"));

        // Verify it was moved to /data/.trash/memo.txt
        assert!(app.fs.stat("/data/.trash/memo.txt").is_ok());
        assert!(app.fs.stat("/data/memo.txt").is_err());

        // Undo trash
        app.undo_last_trash();
        assert!(!app.can_undo_trash());
        assert_eq!(names(&app), ["..", ".trash", "memo.txt"]);
        assert!(app.status_message.contains("Restored memo.txt from Trash"));
        assert!(app.fs.stat("/data/memo.txt").is_ok());
        assert!(app.fs.stat("/data/.trash/memo.txt").is_err());
    }

    #[test]
    fn undo_skips_entry_purged_from_trash() {
        let fs = FakeFs::new()
            .dir("/data")
            .file("/data/old.txt", b"old\n")
            .file("/data/new.txt", b"new\n");
        let mut app = app_at(fs, "/data");
        app.selected_index = 1;
        app.trash_selected();
        // Folders first: "..", ".trash", "old.txt".
        app.selected_index = 2;
        app.trash_selected();
        assert_eq!(app.trash_stack.len(), 2);
        let purged = app.trash_stack[1].trashed_path.clone();
        app.fs.unlink(&purged).unwrap();

        app.undo_last_trash();
        assert!(app.status_message.contains("no longer in Trash"));
        app.undo_last_trash();
        assert!(app.status_message.contains("Restored"));
        assert!(!app.can_undo_trash());
    }

    #[test]
    fn delete_in_trash_lookalike_folder_still_trashes() {
        let fs = FakeFs::new()
            .dir("/data")
            .dir("/data/.trash-old")
            .file("/data/.trash-old/keep.txt", b"keep\n");
        let mut app = app_at(fs, "/data/.trash-old");
        app.selected_index = 1;
        app.trash_selected();
        assert!(app.can_undo_trash());
        assert!(app.fs.stat("/data/.trash/keep.txt").is_ok());
    }
}
