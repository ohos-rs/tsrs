//! OH checker.ts resolveExternalModule / allowImportSendable.
use crate::*;
use tsrs_core::{ScriptKind, tspath};

impl Checker {
    pub(crate) fn report_unchecked_so_module(&mut self, node: P<Node>, name: &str) {
        self.error(Some(node), &diagnostics::Currently_module_for_0_is_not_verified_If_you_re_importing_napi_its_verification_will_be_enabled_in_later_SDK_version_Please_make_sure_the_corresponding_d_ts_file_is_provided_and_the_napis_are_correctly_declared, &[&name]);
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
                &diagnostics::Importing_ArkTS_files_in_JS_and_TS_files_is_about_to_be_forbidden
            } else { &diagnostics::Importing_ArkTS_files_in_JS_and_TS_files_is_forbidden };
            self.error(Some(error_node), message, &[]);
        }
    }
}
