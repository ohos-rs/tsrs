mod common;
use common::*;
use std::sync::Arc;
use tsrs_api::{SessionOptions, embedded::Compiler};
use tsrs_core::{CompilerOptions, Tristate};
use tsrs_vfs::{FS, bundled, osvfs};

#[test]
fn oh49_numeric_enum_equality_keeps_the_remaining_numeric_domain() {
    // OH checker.ts::getEnumKind/narrowTypeByEquality: a computed numeric
    // member selects Numeric, whose TypeFlags.Unit does not include Enum.
    let dir = TempDir::new("oh49-enum-flow");
    let file = dir.write("input.ets", "enum Channel { None = 0, Custom = 1 << 18, All = ~0 }\nfunction label(channel: Channel): string { if (channel == Channel.Custom) { return 'custom'; } else { return channel.toString(); } }\nfunction strictLabel(channel: Channel): string { if (channel !== Channel.Custom) { return channel.toString(); } return 'custom'; }");
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = Compiler::new(SessionOptions::new(
        dir.dir(),
        bundled::lib_path(),
        Arc::clone(&fs),
        false,
    ))
    .unwrap();
    let program = service
        .create_program(
            &[file],
            CompilerOptions {
                no_emit: Tristate::True,
                skip_lib_check: Tristate::True,
                ets_loader_path: dir.path("sdk/ets-loader"),
                ..Default::default()
            },
            fs,
        )
        .unwrap();
    let diagnostics = program.semantic_diagnostics().unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn oh49_regexp_scanning_keeps_legacy_syntax_and_still_rejects_unterminated_literals() {
    // OH scanner.ts::reScanSlashToken never invokes the modern RegExp parser.
    let dir = TempDir::new("oh49-regexp");
    let file = dir.write(
        "input.ts",
        r"const ansi = /\033\[/; const flags = /a/gg; const escaped = /[\/]/;",
    );
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
    let service = Compiler::new(SessionOptions::new(
        dir.dir(),
        bundled::lib_path(),
        Arc::clone(&fs),
        false,
    ))
    .unwrap();
    let create = |roots: &[String], harmony: bool| {
        service
            .create_program(
                roots,
                CompilerOptions {
                    no_emit: Tristate::True,
                    skip_lib_check: Tristate::True,
                    ets_loader_path: if harmony {
                        dir.path("sdk/ets-loader")
                    } else {
                        String::new()
                    },
                    ..Default::default()
                },
                Arc::clone(&fs),
            )
            .unwrap()
    };
    let harmony = create(&[file.clone()], true);
    assert!(harmony.syntactic_diagnostics().unwrap().is_empty());
    assert!(harmony.semantic_diagnostics().unwrap().is_empty());
    let modern_service = Compiler::new(SessionOptions::new(
        dir.dir(),
        bundled::lib_path(),
        Arc::clone(&fs),
        false,
    ))
    .unwrap();
    let modern = modern_service
        .create_program(
            &[file],
            CompilerOptions {
                no_emit: Tristate::True,
                skip_lib_check: Tristate::True,
                ..Default::default()
            },
            Arc::clone(&fs),
        )
        .unwrap();
    assert!(
        modern
            .semantic_diagnostics()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic.code == 1487)
    );
    let unterminated = dir.write("broken.ets", "const broken = /unterminated\n");
    let broken = create(&[unterminated], true);
    let diagnostics = broken.syntactic_diagnostics().unwrap();
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == 1161
            && diagnostic.pos == 16
            && diagnostic.end == 16),
        "{diagnostics:?}"
    );
}
