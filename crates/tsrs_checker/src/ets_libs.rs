//! OH utilities.ts getEtsLibs and checker.ts isValidFromLibs.
use crate::*;
use tsrs_core::tspath;
impl Checker {
    pub(crate) fn initialize_ets_lib_files(&mut self) {
        let Some(libs) = &self.compiler_options.ets.libs else { return; };
        let directory = self.program.get_current_directory();
        self.ets_lib_files.extend(libs.iter().map(|name| tspath::get_normalized_absolute_path(name, &directory)));
        let mut references = Vec::new();
        for file in self.files {
            if !self.ets_lib_files.contains(&tspath::get_normalized_absolute_path(file.file_name(), &directory)) { continue; }
            references.extend(file.referenced_files().iter().map(|reference| tspath::get_normalized_absolute_path(&reference.file_name, &tspath::get_directory_path(file.file_name()))));
        }
        self.ets_lib_files.extend(references);
    }
    pub(crate) fn is_valid_from_ets_libs(&self, symbol: P<Symbol>, location: P<Node>) -> bool {
        if symbol.declarations().is_empty() || self.ets_lib_files.is_empty() { return true; }
        let file = ast::get_source_file_of_node(location).unwrap();
        let name = file.file_name().trim();
        if name.is_empty() { return true; }
        let directory = self.program.get_current_directory(); let name = tspath::get_normalized_absolute_path(name, &directory);
        if name.ends_with(tspath::EXTENSION_ETS) || self.ets_lib_files.contains(&name) { return true; }
        symbol.declarations().iter().any(|&declaration| {
            let file = ast::get_source_file_of_node(declaration).unwrap();
            let name = file.file_name();
            name.is_empty() || !self.ets_lib_files.contains(&tspath::get_normalized_absolute_path(name, &directory))
        })
    }
}
