// SPDX-License-Identifier: Apache-2.0
//! Core MIME types, actions, and launch intent primitives.

extern crate alloc;
use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Canonical MIME content types recognized across CellOS desktop and apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MimeType {
    TextPlain,
    TextMarkdown,
    TextHtml,
    TextSource,
    ApplicationPdf,
    ImagePng,
    ImageJpeg,
    ImagePpm,
    ApplicationElf,
    InodeDirectory,
    Unknown,
}

impl MimeType {
    /// Canonical media-type string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TextPlain => "text/plain",
            Self::TextMarkdown => "text/markdown",
            Self::TextHtml => "text/html",
            Self::TextSource => "text/x-source",
            Self::ApplicationPdf => "application/pdf",
            Self::ImagePng => "image/png",
            Self::ImageJpeg => "image/jpeg",
            Self::ImagePpm => "image/x-portable-pixmap",
            Self::ApplicationElf => "application/x-executable",
            Self::InodeDirectory => "inode/directory",
            Self::Unknown => "application/octet-stream",
        }
    }

    /// Parse a media-type string into a [`MimeType`].
    pub fn parse(s: &str) -> Self {
        match s {
            "text/plain" => Self::TextPlain,
            "text/markdown" => Self::TextMarkdown,
            "text/html" => Self::TextHtml,
            "text/x-source" => Self::TextSource,
            "application/pdf" => Self::ApplicationPdf,
            "image/png" => Self::ImagePng,
            "image/jpeg" => Self::ImageJpeg,
            "image/x-portable-pixmap" => Self::ImagePpm,
            "application/x-executable" => Self::ApplicationElf,
            "inode/directory" => Self::InodeDirectory,
            _ => Self::Unknown,
        }
    }

    /// Identify MIME type from file extension or path conventions.
    pub fn from_path(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let dot_pos = name.rfind('.');
        if let Some(pos) = dot_pos {
            let ext = &name[pos + 1..];
            return match ext {
                s if s.eq_ignore_ascii_case("md") => Self::TextMarkdown,
                s if s.eq_ignore_ascii_case("html") || s.eq_ignore_ascii_case("htm") => {
                    Self::TextHtml
                }
                s if s.eq_ignore_ascii_case("pdf") => Self::ApplicationPdf,
                s if s.eq_ignore_ascii_case("png") => Self::ImagePng,
                s if s.eq_ignore_ascii_case("jpg") || s.eq_ignore_ascii_case("jpeg") => {
                    Self::ImageJpeg
                }
                s if s.eq_ignore_ascii_case("ppm") => Self::ImagePpm,
                s if s.eq_ignore_ascii_case("txt") || s.eq_ignore_ascii_case("log") => {
                    Self::TextPlain
                }
                s if s.eq_ignore_ascii_case("rs")
                    || s.eq_ignore_ascii_case("c")
                    || s.eq_ignore_ascii_case("h")
                    || s.eq_ignore_ascii_case("toml")
                    || s.eq_ignore_ascii_case("json")
                    || s.eq_ignore_ascii_case("lua")
                    || s.eq_ignore_ascii_case("sh") =>
                {
                    Self::TextSource
                }
                s if s.eq_ignore_ascii_case("elf") => Self::ApplicationElf,
                _ => {
                    if path.starts_with("/bin/") {
                        Self::ApplicationElf
                    } else {
                        Self::Unknown
                    }
                }
            };
        }
        if path.starts_with("/bin/") {
            Self::ApplicationElf
        } else {
            Self::Unknown
        }
    }

    /// Identify MIME type from magic headers in the initial bytes.
    pub fn from_magic(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 4 && &bytes[0..4] == b"\x7fELF" {
            Some(Self::ApplicationElf)
        } else if bytes.len() >= 5 && &bytes[0..5] == b"%PDF-" {
            Some(Self::ApplicationPdf)
        } else if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
            Some(Self::ImagePng)
        } else if bytes.len() >= 3 && &bytes[0..3] == b"\xff\xd8\xff" {
            Some(Self::ImageJpeg)
        } else if bytes.len() >= 2 && (bytes[0] == b'P' && (bytes[1] == b'6' || bytes[1] == b'3')) {
            Some(Self::ImagePpm)
        } else {
            None
        }
    }

    /// Human-friendly display label.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::TextPlain => "Plain Text",
            Self::TextMarkdown => "Markdown Document",
            Self::TextHtml => "HTML Web Page",
            Self::TextSource => "Source Code",
            Self::ApplicationPdf => "PDF Document",
            Self::ImagePng => "PNG Image",
            Self::ImageJpeg => "JPEG Image",
            Self::ImagePpm => "PPM Image",
            Self::ApplicationElf => "Executable Program",
            Self::InodeDirectory => "Folder",
            Self::Unknown => "Binary / Unknown",
        }
    }

    /// Compact icon tag for visual listings.
    pub const fn icon_tag(self) -> &'static str {
        match self {
            Self::TextPlain => "[T]",
            Self::TextMarkdown => "[M]",
            Self::TextHtml => "[H]",
            Self::TextSource => "[S]",
            Self::ApplicationPdf => "[P]",
            Self::ImagePng | Self::ImageJpeg | Self::ImagePpm => "[I]",
            Self::ApplicationElf => "[X]",
            Self::InodeDirectory => "[D]",
            Self::Unknown => "[?]",
        }
    }

    /// Whether the content is human-readable text suitable for inline preview.
    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Self::TextPlain | Self::TextMarkdown | Self::TextHtml | Self::TextSource
        )
    }

    /// Whether this type represents an executable binary or runnable payload.
    pub const fn is_executable(self) -> bool {
        matches!(self, Self::ApplicationElf)
    }
}

/// Action intent when handling a file. Separates passive preview from execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileAction {
    /// Open file in an associated viewer or editor application.
    Open,
    /// Passive Quick Look preview without execution or modification rights.
    Preview,
    /// Execute program or script in an isolated spawn domain.
    Run,
}

impl FileAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Preview => "Preview",
            Self::Run => "Run",
        }
    }
}

/// Fully-resolved launch intent binding action, target app, path, and MIME type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchIntent {
    pub action: FileAction,
    pub app_id: &'static str,
    pub app_binary: &'static str,
    pub target_path: String,
    pub mime: MimeType,
}

impl LaunchIntent {
    pub fn new(
        action: FileAction,
        app_id: &'static str,
        app_binary: &'static str,
        target_path: String,
        mime: MimeType,
    ) -> Self {
        Self {
            action,
            app_id,
            app_binary,
            target_path,
            mime,
        }
    }
}
