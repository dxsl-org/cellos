// SPDX-License-Identifier: Apache-2.0
//! File association catalog and MIME-to-application router.

extern crate alloc;
use alloc::string::String;

use super::types::{FileAction, LaunchIntent, MimeType};

/// Application descriptor registered as a launch candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppCandidate {
    pub app_id: &'static str,
    pub app_name: &'static str,
    pub app_binary: &'static str,
}

pub const APP_OCEL: AppCandidate = AppCandidate {
    app_id: "ocel",
    app_name: "Ocel Viewer",
    app_binary: "/bin/ocel",
};

pub const APP_TERMINAL: AppCandidate = AppCandidate {
    app_id: "terminal",
    app_name: "Terminal",
    app_binary: "/bin/terminal",
};

pub const APP_FILES: AppCandidate = AppCandidate {
    app_id: "files",
    app_name: "Files",
    app_binary: "/bin/files",
};

pub const APP_CHROME: AppCandidate = AppCandidate {
    app_id: "chrome",
    app_name: "Chrome (Tier 3)",
    app_binary: "/bin/chrome",
};

static CANDIDATES_MARKDOWN: &[AppCandidate] = &[APP_OCEL, APP_TERMINAL];
static CANDIDATES_HTML: &[AppCandidate] = &[APP_OCEL, APP_CHROME];
static CANDIDATES_PDF: &[AppCandidate] = &[APP_OCEL];
static CANDIDATES_IMAGE: &[AppCandidate] = &[APP_OCEL];
static CANDIDATES_SOURCE: &[AppCandidate] = &[APP_TERMINAL, APP_OCEL];
static CANDIDATES_PLAIN: &[AppCandidate] = &[APP_TERMINAL, APP_OCEL];
static CANDIDATES_ELF: &[AppCandidate] = &[APP_TERMINAL];
static CANDIDATES_DIR: &[AppCandidate] = &[APP_FILES];
static CANDIDATES_UNKNOWN: &[AppCandidate] = &[APP_TERMINAL];

/// Global static association registry and query functions.
pub struct AssociationRegistry;

impl AssociationRegistry {
    /// Return the list of candidate applications for a given MIME type.
    pub fn candidates_for_mime(mime: MimeType) -> &'static [AppCandidate] {
        match mime {
            MimeType::TextMarkdown => CANDIDATES_MARKDOWN,
            MimeType::TextHtml => CANDIDATES_HTML,
            MimeType::ApplicationPdf => CANDIDATES_PDF,
            MimeType::ImagePng | MimeType::ImageJpeg | MimeType::ImagePpm => CANDIDATES_IMAGE,
            MimeType::TextSource => CANDIDATES_SOURCE,
            MimeType::TextPlain => CANDIDATES_PLAIN,
            MimeType::ApplicationElf => CANDIDATES_ELF,
            MimeType::InodeDirectory => CANDIDATES_DIR,
            MimeType::Unknown => CANDIDATES_UNKNOWN,
        }
    }

    /// Return the primary default application for a given MIME type.
    pub fn default_app(mime: MimeType) -> Option<AppCandidate> {
        Self::candidates_for_mime(mime).first().copied()
    }

    /// Resolve a file path into a fully-bound [`LaunchIntent`].
    pub fn resolve_intent(path: &str, preferred_action: Option<FileAction>) -> LaunchIntent {
        let mime = MimeType::from_path(path);
        let action = match preferred_action {
            Some(act) => act,
            None => {
                if mime.is_executable() {
                    FileAction::Run
                } else {
                    FileAction::Open
                }
            }
        };

        let app = Self::default_app(mime).unwrap_or(APP_TERMINAL);
        LaunchIntent::new(action, app.app_id, app.app_binary, String::from(path), mime)
    }

    /// Resolve an intent for a specific candidate application ID.
    pub fn resolve_intent_with_app(
        path: &str,
        app_id: &str,
        preferred_action: Option<FileAction>,
    ) -> Option<LaunchIntent> {
        let mime = MimeType::from_path(path);
        let candidates = Self::candidates_for_mime(mime);
        let app = candidates.iter().find(|c| c.app_id == app_id).copied()?;
        let action = preferred_action.unwrap_or(if mime.is_executable() {
            FileAction::Run
        } else {
            FileAction::Open
        });

        Some(LaunchIntent::new(
            action,
            app.app_id,
            app.app_binary,
            String::from(path),
            mime,
        ))
    }
}
