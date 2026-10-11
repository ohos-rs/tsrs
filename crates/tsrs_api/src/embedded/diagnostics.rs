use super::ProgramSnapshot;
use crate::{ApiError, ApiResult};
use std::path::PathBuf;
use tsrs_ast::Diagnostic as CompilerDiagnostic;
use tsrs_core::{
    P,
    context::{CheckerLifetime, Context, with_checker_lifetime},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub file_name: Option<PathBuf>,
    pub pos: i64,
    pub end: i64,
    pub code: i32,
    pub category: u8,
    pub source: String,
    pub text: String,
    pub start_position: Option<DiagnosticPosition>,
    pub end_position: Option<DiagnosticPosition>,
    pub message_chain: Vec<Diagnostic>,
    pub related_information: Vec<Diagnostic>,
    pub reports_unnecessary: bool,
    pub reports_deprecated: bool,
}

impl Diagnostic {
    // Same UTF-8 -> UTF-16 conversion as proto.go NewDiagnosticResponse.
    pub(crate) fn from_compiler(d: &CompilerDiagnostic) -> Self {
        let (mut pos, mut end) = (d.pos(), d.end());
        let mut start_position = None;
        let mut end_position = None;
        if let Some(file) = d.file() {
            let len = file.text().len() as i32;
            pos = pos.clamp(0, len);
            end = end.clamp(pos, len);
            let position = |p| {
                let (line, character) =
                    tsrs_scanner::get_ecma_line_and_utf16_character_of_position(&*file, p);
                DiagnosticPosition {
                    line: line as u32,
                    character: character as u32,
                }
            };
            start_position = Some(position(pos));
            end_position = Some(position(end));
            let map = file.get_position_map();
            pos = map.utf8_to_utf16(pos);
            end = map.utf8_to_utf16(end);
        }
        Self {
            file_name: d.file().map(|f| PathBuf::from(f.file_name())),
            pos: i64::from(pos),
            end: i64::from(end),
            code: d.code(),
            category: d.category() as u8,
            source: d.source().to_string(),
            text: d.localize(),
            start_position,
            end_position,
            message_chain: d
                .message_chain()
                .iter()
                .map(|d| Self::from_compiler(d))
                .collect(),
            related_information: d
                .related_information()
                .iter()
                .map(|d| Self::from_compiler(d))
                .collect(),
            reports_unnecessary: d.reports_unnecessary(),
            reports_deprecated: d.reports_deprecated(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ArktsBuildDiagnostics {
    pub linter_diagnostics: Vec<Diagnostic>,
    pub syntactic_diagnostics: Vec<Diagnostic>,
    pub semantic_diagnostics: Vec<Diagnostic>,
}

impl ProgramSnapshot {
    pub fn arkts_build_diagnostics(&self) -> ApiResult<ArktsBuildDiagnostics> {
        self.run(|this| {
            let diagnostics = this
                .program()?
                .get_arkts_build_diagnostics(&Context::background());
            Ok(ArktsBuildDiagnostics {
                linter_diagnostics: owned(&diagnostics.linter),
                syntactic_diagnostics: owned(&diagnostics.syntactic),
                semantic_diagnostics: owned(&diagnostics.semantic),
            })
        })
    }
    pub fn syntactic_diagnostics(&self) -> ApiResult<Vec<Diagnostic>> {
        self.run(|this| {
            Ok(owned(
                &this
                    .program()?
                    .get_syntactic_diagnostics(&diagnostic_context(), None),
            ))
        })
    }
    pub fn semantic_diagnostics(&self) -> ApiResult<Vec<Diagnostic>> {
        self.run(|this| {
            Ok(owned(
                &this
                    .program()?
                    .get_semantic_diagnostics(&diagnostic_context(), None),
            ))
        })
    }
    pub fn arkts_linter_diagnostics(&self) -> ApiResult<Vec<Diagnostic>> {
        self.run(|this| {
            Ok(owned(
                &this
                    .program()?
                    .get_arkts_linter_diagnostics(&diagnostic_context(), None),
            ))
        })
    }
    pub fn declaration_diagnostics(&self, files: &[String]) -> ApiResult<Vec<Diagnostic>> {
        let files = files.to_vec();
        self.run(move |this| {
            let program = this.program()?;
            let mut result = Vec::new();
            for name in &files {
                let file = program
                    .get_source_file(name)
                    .ok_or_else(|| ApiError::client(format!("source file not found: {name}")))?;
                result.extend(owned(
                    &program.get_declaration_diagnostics(&diagnostic_context(), Some(file)),
                ));
            }
            Ok(result)
        })
    }
}

fn diagnostic_context() -> Context {
    with_checker_lifetime(&Context::background(), CheckerLifetime::Diagnostics)
}
fn owned(values: &[P<CompilerDiagnostic>]) -> Vec<Diagnostic> {
    values
        .iter()
        .map(|d| Diagnostic::from_compiler(d))
        .collect()
}
