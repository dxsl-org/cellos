// SPDX-License-Identifier: Apache-2.0
//! File Association and MIME Intent Routing service contract.

pub mod registry;
pub mod types;

pub use registry::{
    AppCandidate, AssociationRegistry, APP_CHROME, APP_FILES, APP_OCEL, APP_TERMINAL,
};
pub use types::{FileAction, LaunchIntent, MimeType};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_detection_from_path() {
        assert_eq!(
            MimeType::from_path("/data/notes.md"),
            MimeType::TextMarkdown
        );
        assert_eq!(MimeType::from_path("/var/log/sys.log"), MimeType::TextPlain);
        assert_eq!(MimeType::from_path("/doc/index.html"), MimeType::TextHtml);
        assert_eq!(MimeType::from_path("/paper.pdf"), MimeType::ApplicationPdf);
        assert_eq!(MimeType::from_path("/src/main.rs"), MimeType::TextSource);
        assert_eq!(MimeType::from_path("/img/cat.png"), MimeType::ImagePng);
        assert_eq!(MimeType::from_path("/img/cat.jpg"), MimeType::ImageJpeg);
        assert_eq!(MimeType::from_path("/bin/editor"), MimeType::ApplicationElf);
        assert_eq!(MimeType::from_path("/unknown.xyz"), MimeType::Unknown);
    }

    #[test]
    fn test_mime_magic_detection() {
        assert_eq!(
            MimeType::from_magic(b"\x7fELF\x02\x01\x01\x00"),
            Some(MimeType::ApplicationElf)
        );
        assert_eq!(
            MimeType::from_magic(b"%PDF-1.4\n"),
            Some(MimeType::ApplicationPdf)
        );
        assert_eq!(
            MimeType::from_magic(b"\x89PNG\r\n\x1a\n"),
            Some(MimeType::ImagePng)
        );
        assert_eq!(
            MimeType::from_magic(b"\xff\xd8\xff\xe0"),
            Some(MimeType::ImageJpeg)
        );
        assert_eq!(
            MimeType::from_magic(b"P6 800 600"),
            Some(MimeType::ImagePpm)
        );
        assert_eq!(MimeType::from_magic(b"hello world"), None);
    }

    #[test]
    fn test_association_resolution_and_open_with() {
        let md_intent = AssociationRegistry::resolve_intent("/data/readme.md", None);
        assert_eq!(md_intent.mime, MimeType::TextMarkdown);
        assert_eq!(md_intent.action, FileAction::Open);
        assert_eq!(md_intent.app_id, "ocel");
        assert_eq!(md_intent.app_binary, "/bin/ocel");

        let candidates = AssociationRegistry::candidates_for_mime(MimeType::TextMarkdown);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].app_id, "ocel");
        assert_eq!(candidates[1].app_id, "terminal");

        // Specific Open-With selection
        let term_intent =
            AssociationRegistry::resolve_intent_with_app("/data/readme.md", "terminal", None)
                .expect("candidate exists");
        assert_eq!(term_intent.app_id, "terminal");
        assert_eq!(term_intent.app_binary, "/bin/terminal");

        // Executable program action separation
        let elf_intent = AssociationRegistry::resolve_intent("/bin/test-app", None);
        assert_eq!(elf_intent.mime, MimeType::ApplicationElf);
        assert_eq!(elf_intent.action, FileAction::Run);
    }
}
