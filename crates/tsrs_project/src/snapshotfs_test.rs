// OH snapshot cache sharing: the snapshot builder must keep the Arc<str> text of the overlay files it
// carries forward, so a snapshot and its replacement share one buffer per file.

use std::sync::Arc;

use tsrs_core::tspath::Path;
use tsrs_lsproto as lsproto;
use tsrs_vfs::vfstest;

use super::*;
use crate::filechange::{FileChange, FileChangeKind};
use crate::overlayfs::*;

fn create_overlay_fs() -> OverlayFS {
    let test_fs = vfstest::from_map(
        [("/test1.ts", "// existing content"), ("/test2.ts", "// existing content")],
        false, /* useCaseSensitiveFileNames */
    );
    new_overlay_fs(
        FsRef::Host(Arc::new(test_fs)),
        Arc::default(),
        lsproto::PositionEncodingKind::UTF16,
        Arc::new(|file_name: &str| Path::from(file_name)),
    )
}

fn test_uri1() -> lsproto::DocumentUri {
    lsproto::DocumentUri("file:///test1.ts".to_string())
}

fn open(uri: lsproto::DocumentUri, version: i32, content: &str, language_kind: lsproto::LanguageKind) -> FileChange {
    FileChange {
        kind: FileChangeKind::Open,
        uri,
        version,
        content: content.to_string(),
        language_kind,
        changes: Vec::new(),
    }
}

#[test]
fn snapshot_cache_shares_text_and_preserves_previous_file_versions() {
    let to_path: ToPath = Arc::new(|name: &str| Path::from(name));
    let fs = create_overlay_fs();
    fs.process_changes(&[open(test_uri1(), 1, "export const value: number = 1;", lsproto::LanguageKind::TYPE_SCRIPT)]);
    let text = fs.get_file("/test1.ts").unwrap().shared_content();
    let handle = crate::overlayfs::new_cached_file_handle("/test1.ts", Arc::clone(&text));
    assert!(Arc::ptr_eq(&handle.shared_content(), &text));
    let builder = new_snapshot_fs_builder_from_source(Arc::new(fs), Arc::default(), Arc::default(), Arc::default(), Arc::clone(&to_path));
    let cached = builder.get_file("/test1.ts").unwrap();
    assert!(Arc::ptr_eq(&cached.shared_content(), &text));
    let (old, _) = builder.finalize();
    assert!(Arc::ptr_eq(&old.get_file("/test1.ts").unwrap().shared_content(), &text));
    let replacement: Arc<str> = Arc::from("export const value: number = 2;");
    let next_fs = create_overlay_fs();
    next_fs.process_changes(&[open(test_uri1(), 2, &replacement, lsproto::LanguageKind::TYPE_SCRIPT)]);
    let next = new_snapshot_fs_builder_from_source(Arc::new(next_fs), Arc::clone(&old.cache_files), Arc::default(), Arc::default(), to_path);
    next.invalidate_cache();
    let new = next.get_file("/test1.ts").unwrap();
    assert_ne!(new.hash(), cached.hash());
    assert_eq!(new.content(), replacement.as_ref());
}
