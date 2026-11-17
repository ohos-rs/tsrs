//! OH checker.ts resolveExternalModule / allowImportSendable.
use crate::*;
use tsrs_core::{ScriptKind, tspath};

impl Checker {
    pub(crate) fn report_unchecked_so_module(&mut self, node: P<Node>, name: &str) {
        self.error(Some(node), &diagnostics::CURRENTLY_MODULE_FOR_0_IS_NOT_VERIFIED_IF_YOU_RE_IMPORTING_NAPI_ITS_VERIFICATION_WILL_BE_ENABLED_IN_LATER_SDK_VERSION_PLEASE_MAKE_SURE_THE_CORRESPONDING_D_TS_FILE_IS_PROVIDED_AND_THE_NAPIS_ARE_CORRECTLY_DECLARED, &[&name]);
    }
    pub(crate) fn check_ts_import_ets(&mut self, source: P<SourceFile>, specifier: Option<P<Node>>, error_node: Option<P<Node>>) {
        let Some(error_node) = error_node else { return; };
        if source.script_kind() != ScriptKind::TS || self.compiler_options.need_do_ark_ts_linter != Tristate::True { return; }
        let mut allowed = self.compiler_options.ts_import_sendable_enable == Tristate::True && !source.is_declaration_file();
        let loader = &self.compiler_options.ets_loader_path;
        if !loader.is_empty() && source.is_declaration_file() {
            allowed = tspath::normalize_path(source.file_name()).starts_with(&tspath::get_normalized_absolute_path("../..", loader));
        }
        if specifier.is_some_and(|s| !s.parent().is_some_and(|p| matches!(p.kind(), Kind::ImportDeclaration | Kind::ExportDeclaration))) || !allowed {
            let message = if self.compiler_options.is_compatible_version == Tristate::True {
                &diagnostics::IMPORTING_ARK_TS_FILES_IN_JS_AND_TS_FILES_IS_ABOUT_TO_BE_FORBIDDEN
            } else { &diagnostics::IMPORTING_ARK_TS_FILES_IN_JS_AND_TS_FILES_IS_FORBIDDEN };
            self.error(Some(error_node), message, &[]);
        }
    }
}
