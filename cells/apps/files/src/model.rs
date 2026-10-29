// SPDX-License-Identifier: MIT
//! View models for Files: entries as listed by the VFS, places, and paste-name conflicts.

extern crate alloc;
use alloc::format;
use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    List = 0,
    Grid = 1,
    Columns = 2,
}

impl ViewMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Grid => "Grid",
            Self::Columns => "Columns",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// `Stat` size; `None` for folders and when the VFS refused the `Stat`.
    pub size: Option<u64>,
    pub icon: &'static str,
}

impl FileItem {
    pub fn new(name: impl Into<String>, path: impl Into<String>, is_dir: bool, size: Option<u64>) -> Self {
        let name = name.into();
        let path = path.into();
        let icon = if is_dir {
            "[D]"
        } else if name.ends_with(".md") || name.ends_with(".txt") {
            "[M]"
        } else if name.ends_with(".pdf") {
            "[P]"
        } else if name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".ppm") {
            "[I]"
        } else if name.ends_with(".rs") || name.ends_with(".c") || name.ends_with(".toml") {
            "[T]"
        } else {
            "[?]"
        };

        Self {
            name,
            path,
            is_dir,
            size,
            icon,
        }
    }

    pub fn formatted_size(&self) -> String {
        match self.size {
            _ if self.is_dir => String::from("--"),
            None => String::from("?"),
            Some(size) if size < 1024 => format!("{} B", size),
            Some(size) if size < 1024 * 1024 => format!("{} KB", size / 1024),
            Some(size) => format!("{} MB", size / (1024 * 1024)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacesItem {
    pub name: &'static str,
    pub path: &'static str,
    pub icon: &'static str,
    pub is_pinned: bool,
}

pub const DEFAULT_PLACES: &[PlacesItem] = &[
    PlacesItem { name: "Root", path: "/", icon: "(/)", is_pinned: false },
    PlacesItem { name: "Applications", path: "/bin", icon: "[A]", is_pinned: false },
    PlacesItem { name: "Data", path: "/data", icon: "[*]", is_pinned: true },
    PlacesItem { name: "Storage", path: "/mnt", icon: "[S]", is_pinned: true },
    PlacesItem { name: "Services", path: "/srv", icon: "[=]", is_pinned: false },
];

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    Overwrite,
    RenameNew,
    Cancel,
}

/// Calculate a non-conflicting filename by appending ` (1)`, ` (2)`, etc.
pub fn resolve_conflict_name(name: &str, existing_names: &[String]) -> String {
    if !existing_names.iter().any(|n| n == name) {
        return String::from(name);
    }

    let (stem, ext) = match name.rfind('.') {
        Some(idx) if idx > 0 => (&name[..idx], &name[idx..]),
        _ => (name, ""),
    };

    for i in 1..100 {
        let candidate = format!("{} ({}){}", stem, i, ext);
        if !existing_names.iter().any(|n| n == &candidate) {
            return candidate;
        }
    }
    format!("{} (copy){}", stem, ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conflict_name_resolution() {
        let existing = alloc::vec![
            String::from("notes.md"),
            String::from("notes (1).md"),
            String::from("document.txt"),
        ];

        // Unique name remains unchanged
        assert_eq!(resolve_conflict_name("report.pdf", &existing), "report.pdf");

        // Existing name gets (2) since (1) already exists
        assert_eq!(resolve_conflict_name("notes.md", &existing), "notes (2).md");

        // Existing name without (1) gets (1)
        assert_eq!(resolve_conflict_name("document.txt", &existing), "document (1).txt");
    }

    #[test]
    fn test_file_item_icons_and_sizes() {
        let dir = FileItem::new("docs", "/docs", true, None);
        assert_eq!(dir.icon, "[D]");
        assert_eq!(dir.formatted_size(), "--");

        let doc = FileItem::new("readme.md", "/readme.md", false, Some(2048));
        assert_eq!(doc.icon, "[M]");
        assert_eq!(doc.formatted_size(), "2 KB");

        let pdf = FileItem::new("manual.pdf", "/manual.pdf", false, Some(5 * 1024 * 1024));
        assert_eq!(pdf.icon, "[P]");
        assert_eq!(pdf.formatted_size(), "5 MB");

        let refused = FileItem::new("secret", "/secret", false, None);
        assert_eq!(refused.formatted_size(), "?");
    }
}
