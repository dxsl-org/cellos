// SPDX-License-Identifier: MIT
//! Files state and operations. Every listing and every change goes to the VFS through
//! [`FileSystem`]; after a change the folder is listed again, so the view shows what the
//! service holds, and a refusal is shown as the VFS reported it.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::{self, FileSystem, FsResult, PREVIEW_READ_MAX};
use crate::model::{resolve_conflict_name, FileItem, PlacesItem, ViewMode, DEFAULT_PLACES};
use crate::preview::{PreviewKind, QuickLookState};

/// Longest text the path/name prompt accepts (one FAT long name).
pub const MAX_INPUT_LEN: usize = 255;
/// Name of the synthetic parent entry at the top of every folder except `/`.
pub const PARENT_ENTRY: &str = "..";

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
        };
        app.load_directory(initial_path);
        app
    }

    fn read_listing(&mut self, path: &str) -> FsResult<Vec<FileItem>> {
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
        let count = self.items.iter().filter(|item| item.name != PARENT_ENTRY).count();
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
    /// index) and report `status`.
    fn refresh(&mut self, select: Option<&str>, status: String) {
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

    fn selected_entry(&self) -> Option<FileItem> {
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

    /// Enter a folder, or Quick Look a file.
    pub fn open_selected(&mut self) {
        let Some(item) = self.items.get(self.selected_index).cloned() else {
            return;
        };
        if item.is_dir {
            self.load_directory(&item.path);
        } else {
            self.preview_selected();
        }
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
        self.clipboard = Some(Clip { path: item.path, name: item.name, is_dir: item.is_dir, cut });
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
            Ok(()) => self.refresh(None, format!("Permanently deleted {} (irreversible)", item.name)),
            Err(e) => self.status_message = format!("Cannot delete {}: {}", item.name, e.describe()),
        }
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
        self.prompt = Some(Prompt { kind, text: String::new() });
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
        let what = if kind == PromptKind::NewFolder { "folder" } else { "file" };
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
            Err(e) => self.status_message = format!("Cannot create {} {}: {}", what, text, e.describe()),
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
        FakeFs::new().dir("/tmp").dir("/tmp/c5dir").file("/tmp/c5dir/a.txt", b"c5\n")
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
        let mut fs = c5_tree().dir("/srv").dir("/srv/cellos").file("/srv/cellos/key", b"k");
        fs.denied.push("/srv/cellos");
        let mut app = app_at(fs, "/tmp/c5dir");
        type_prompt(&mut app, PromptKind::GoTo, "/srv/cellos");
        assert_eq!(app.status_message, "Cannot open /srv/cellos: permission denied (VFS ERR_DENIED)");
        assert_eq!(app.current_path, "/tmp/c5dir");
        assert_eq!(names(&app), ["..", "a.txt"]);

        // A missing initial folder is an error with nothing listed, not a sample listing.
        let app = app_at(FakeFs::new(), "/data");
        assert!(app.items.is_empty());
        assert_eq!(app.status_message, "Cannot open /data: I/O error (VFS ERR_IO)");
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
        assert_eq!(app.fs.nodes["/tmp/c5dir/c5new/a.txt"].as_deref(), Some(&b"c5\n"[..]));

        app.delete_selected();
        assert_eq!(app.status_message, "Permanently deleted a.txt (irreversible)");
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
        assert_eq!(app.fs.nodes["/tmp/c5dir/a (1).txt"].as_deref(), Some(&b"c5\n"[..]));
        assert!(app.clipboard.is_some());

        app.selected_index = app.items.iter().position(|i| i.name == "a.txt").unwrap();
        app.copy_selected(true);
        type_prompt(&mut app, PromptKind::GoTo, "../other");
        app.paste_into_current();
        assert_eq!(app.status_message, "Moved a.txt -> a.txt");
        assert!(app.clipboard.is_none());
        assert!(!app.fs.nodes.contains_key("/tmp/c5dir/a.txt"));
        assert_eq!(app.fs.nodes["/tmp/other/a.txt"].as_deref(), Some(&b"c5\n"[..]));
    }

    #[test]
    fn refused_mutations_are_reported_and_change_nothing() {
        let mut fs = c5_tree().dir("/bin").file("/bin/tool", b"elf");
        fs.read_only.push("/bin");
        let mut app = app_at(fs, "/bin");
        type_prompt(&mut app, PromptKind::NewFolder, "x");
        assert_eq!(app.status_message, "Cannot create folder x: permission denied (VFS ERR_DENIED)");
        type_prompt(&mut app, PromptKind::NewFile, "y");
        assert_eq!(app.status_message, "Cannot create file y: permission denied (VFS ERR_DENIED)");
        app.selected_index = 1;
        app.delete_selected();
        assert_eq!(app.status_message, "Cannot delete tool: permission denied (VFS ERR_DENIED)");
        assert!(app.fs.nodes.contains_key("/bin/tool"));

        app.copy_selected(true);
        type_prompt(&mut app, PromptKind::GoTo, "/tmp");
        app.paste_into_current();
        // The copy landed, the source could not be removed: both are visible and reported.
        assert_eq!(app.status_message, "Paste of tool failed: permission denied (VFS ERR_DENIED)");
        assert!(app.fs.nodes.contains_key("/bin/tool"));
        assert_eq!(names(&app), ["..", "c5dir", "tool"]);
        assert!(app.clipboard.as_ref().is_some_and(|clip| clip.cut));

        // A non-empty folder outside /data is not removed recursively (`rmdir` scope).
        type_prompt(&mut app, PromptKind::GoTo, "/tmp");
        app.selected_index = 1;
        app.delete_selected();
        assert_eq!(app.status_message, "Cannot delete c5dir: I/O error (VFS ERR_IO)");
        assert!(app.fs.nodes.contains_key("/tmp/c5dir/a.txt"));
    }

    #[test]
    fn prompt_validates_names_and_bounds_input() {
        let mut app = app_at(c5_tree(), "/tmp/c5dir");
        type_prompt(&mut app, PromptKind::NewFolder, "a/b");
        assert_eq!(app.status_message, "Cannot create folder: a name cannot contain '/'");
        type_prompt(&mut app, PromptKind::NewFile, "a.txt");
        assert_eq!(app.status_message, "Cannot create file a.txt: already exists");
        type_prompt(&mut app, PromptKind::NewFile, "b.txt");
        assert_eq!(app.status_message, "Created file b.txt");
        assert_eq!(app.fs.nodes["/tmp/c5dir/b.txt"].as_deref(), Some(&b""[..]));

        app.begin_prompt(PromptKind::NewFolder);
        for _ in 0..MAX_INPUT_LEN + 10 {
            app.prompt_input('x');
        }
        assert_eq!(app.prompt.as_ref().unwrap().text.len(), MAX_INPUT_LEN);
        app.prompt_backspace();
        assert_eq!(app.status_message.len(), "New folder name: ".len() + MAX_INPUT_LEN - 1 + 1);
        app.prompt_cancel();
        assert!(app.prompt.is_none());
        assert_eq!(app.status_message, "/tmp/c5dir: 2 item(s)");
    }

    #[test]
    fn quick_look_reads_the_selected_file() {
        let mut app = app_at(c5_tree(), "/tmp/c5dir");
        app.selected_index = 1;
        app.open_selected();
        assert!(app.quick_look.is_open);
        assert_eq!(app.quick_look.lines, ["c5"]);
    }
}
