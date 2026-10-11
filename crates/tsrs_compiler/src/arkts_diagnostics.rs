//! OH ets2bundle's normal / syntactic / strict-linter build phases.
use super::*;
use rustc_hash::FxHashSet;
use tsrs_core::{ScriptKind, Tristate};

pub struct ArkTsBuildDiagnostics {
    pub linter: Vec<P<Diagnostic>>,
    pub syntactic: Vec<P<Diagnostic>>,
    pub semantic: Vec<P<Diagnostic>>,
}
impl Program {
    fn arkts_build_pool(&'static self) -> &'static checkerPool {
        self.compiler_checker_pool().unwrap_or_else(|| self.arkts_build_checker_pool.get_or_init(|| checkerPool::new(self)))
    }
    fn arkts_linter_pool(&'static self) -> &'static checkerPool {
        self.arkts_linter_checker_pool.get_or_init(|| checkerPool::new_arkts_linter(self))
    }
    /// Value-only transform facts share the normal build checkers and their SDK-use caches.
    pub fn for_each_arkts_build_checker_group(&'static self, files: &[P<SourceFile>], cb: impl Fn(&mut Checker, usize, P<SourceFile>) + Sync) {
        self.arkts_build_pool().for_each_checker_group_do(files, self.single_threaded(), cb);
    }
    pub fn get_arkts_build_checker_for_file(&'static self, file: P<SourceFile>) -> CheckerHandle {
        self.arkts_build_pool().get_checker_for_file_exclusive(file)
    }
    pub fn get_arkts_build_diagnostics(&'static self, ctx: &Context) -> ArkTsBuildDiagnostics {
        let semantic_files: Vec<_> = self.files.iter().copied().filter(|f| self.options().strict_checker_only != Tristate::True || f.script_kind() != ScriptKind::ETS).collect();
        let normal_pool = self.arkts_build_pool();
        let groups: Vec<Mutex<Vec<P<Diagnostic>>>> = semantic_files.iter().map(|_| Mutex::new(Vec::new())).collect();
        normal_pool.for_each_checker_group_do(&semantic_files, self.single_threaded(), |checker, index, file| {
            if !self.skip_type_checking(file, false) { *groups[index].lock().unwrap() = self.get_semantic_diagnostics_with_checker(ctx, checker, file); }
        });
        let groups: Vec<_> = groups.into_iter().map(|g| g.into_inner().unwrap()).collect();
        let semantic = filter_and_sort_diagnostics(&groups.concat());
        let normal: FxHashMap<_, _> = semantic_files.into_iter().zip(groups).collect();
        let syntactic = self.get_syntactic_diagnostics(ctx, None);
        let linter = if self.options().need_do_ark_ts_linter == Tristate::True { self.collect_arkts_linter_diagnostics(ctx, self.files, &normal, normal_pool) } else { Vec::new() };
        ArkTsBuildDiagnostics { linter, syntactic, semantic }
    }
    pub fn get_arkts_linter_diagnostics(&'static self, ctx: &Context, source_file: Option<P<SourceFile>>) -> Vec<P<Diagnostic>> {
        if self.options().need_do_ark_ts_linter != Tristate::True { return Vec::new(); }
        let one_file = source_file.map(|f| [f]); let files = one_file.as_ref().map_or(self.files, |f| f.as_slice());
        let mut normal = FxHashMap::default();
        if self.options().strict_checker_only != Tristate::True {
            let ets_files: Vec<_> = files.iter().copied().filter(|f| f.script_kind() == ScriptKind::ETS && self.should_run_arkts_linter(*f)).collect();
            let groups = self.collect_checker_diagnostics_from_files(ctx, &ets_files, &|ctx, checker, file| self.get_semantic_diagnostics_with_checker(ctx, checker, file));
            normal.extend(ets_files.into_iter().zip(groups));
        }
        self.collect_arkts_linter_diagnostics(ctx, files, &normal, self.arkts_build_pool())
    }
    fn collect_arkts_linter_diagnostics(&'static self, ctx: &Context, files: &[P<SourceFile>], normal: &FxHashMap<P<SourceFile>, Vec<P<Diagnostic>>>, normal_pool: &checkerPool) -> Vec<P<Diagnostic>> {
        let strict_pool = self.arkts_linter_pool();
        let groups: Vec<Mutex<Vec<P<Diagnostic>>>> = files.iter().map(|_| Mutex::new(Vec::new())).collect();
        strict_pool.for_each_checker_group_do(files, self.single_threaded(), |checker, index, file| {
            if !self.should_run_arkts_linter(file) || file.script_kind() == ScriptKind::TS { return; }
            let mut strict = self.get_semantic_diagnostics_with_checker(ctx, checker, file);
            let existing: FxHashSet<_> = normal.get(&file).into_iter().flatten().map(|d| (d.code(), d.pos(), d.len())).collect();
            strict.retain(|d| (d.pos() != 0 || d.len() != 0) && (self.options().strict_checker_only == Tristate::True || !existing.contains(&(d.code(), d.pos(), d.len()))));
            *groups[index].lock().unwrap() = strict;
        });
        normal_pool.for_each_checker_parallel(|_, checker| checker.reset_oh_sdk_jsdoc_check());
        strict_pool.for_each_checker_parallel(|_, checker| checker.reset_oh_sdk_jsdoc_check());
        strict_pool.for_each_checker_group_do(files, self.single_threaded(), |checker, index, file| {
            if !self.should_run_arkts_linter(file) { return; }
            let mut result = groups[index].lock().unwrap();
            let strict = std::mem::take(&mut *result);
            *result = checker.get_arkts_linter_diagnostics(file, strict);
        });
        groups.into_iter().flat_map(|g| g.into_inner().unwrap()).collect()
    }
    fn should_run_arkts_linter(&self, file: P<SourceFile>) -> bool {
        if !matches!(file.script_kind(), ScriptKind::ETS | ScriptKind::TS) { return false; }
        let name = tspath::normalize_path(file.file_name()); let options = self.options();
        if options.skip_oh_modules_lint == Tristate::True && name.contains("/oh_modules/") { return false; }
        if file.script_kind() != ScriptKind::TS { return true; }
        let in_sdk = !options.ets_loader_path.is_empty() && name.starts_with(&tspath::normalize_path(&tspath::resolve_path(&options.ets_loader_path, &["../.."])));
        !tspath::get_base_file_name(&name).starts_with("@kit.") && !name.contains("/oh_modules/") && (options.ts_import_sendable_enable == Tristate::True || in_sdk)
    }
}
