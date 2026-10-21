//! In-process compiler API. Uses the same snapshot and checker owners as the wire API,
//! with typed calls and RAII leases instead of transport messages or raw arena pointers.
mod diagnostics;
mod executor;
mod facts;
pub(crate) mod queries;

pub use crate::ohsdkplugin::{SdkPluginCompletion, SdkPluginExecutor, SdkPluginHost, SdkPluginRequest};
pub use diagnostics::{
    ArktsBuildDiagnostics, ArktsNormalDiagnostics, Diagnostic, DiagnosticPosition,
};
pub use facts::{
    AnnotationConstant, AnnotationInfo, AnnotationProperty, AnnotationTransformImport,
    AnnotationTransformInfo, AnnotationTransformNode, ArktsBuilderReceiverTypeFacts,
    ArktsExpressionTypeFacts, ArktsPropertyTypeFacts, ArktsTransformTypeFacts,
    ArktsTransformTypeQuery, ProgramSourceGraph, ProgramSourceGraphFile, ResolvedTypeIdentity,
    SdkApiUse, TypeQueryRange,
};

use crate::{ApiError, ApiResult, Session, SessionOptions};
use std::sync::Arc;
use tsrs_compiler::Program;
use tsrs_core::{CompilerOptions, P, arena::Region, context::Context, tspath};
use tsrs_project::{
    APICreateProgramRequest, APIReconfigureProgramRequest, APISnapshotRequest, FileChangeSummary,
    ID, Snapshot,
};
use tsrs_vfs::FS;

/// One collection of compiler snapshots. Filesystems passed to create/update must
/// remain immutable while their snapshots are alive. Disk invalidations are explicit.
pub struct Compiler {
    session: Arc<Session>,
    executor: Arc<executor::Executor>,
}

#[derive(Clone, Debug, Default)]
pub struct FileChanges {
    pub changed: Vec<String>,
    pub created: Vec<String>,
    pub deleted: Vec<String>,
}

impl Compiler {
    pub fn new(options: SessionOptions) -> ApiResult<Self> {
        Ok(Self {
            session: Session::new(options),
            executor: Arc::new(executor::Executor::new()?),
        })
    }

    pub fn create_program(
        &self,
        roots: &[String],
        options: CompilerOptions,
        fs: Arc<dyn FS>,
    ) -> ApiResult<ProgramSnapshot> {
        self.build(None, roots, options, fs, &FileChanges::default())
    }

    pub fn update_program(
        &self,
        previous: &ProgramSnapshot,
        roots: &[String],
        options: CompilerOptions,
        fs: Arc<dyn FS>,
        changes: &FileChanges,
    ) -> ApiResult<ProgramSnapshot> {
        if !Arc::ptr_eq(&self.session, &previous.data.session) {
            return Err(ApiError::client("program belongs to another compiler"));
        }
        self.build(Some(previous), roots, options, fs, changes)
    }

    fn build(
        &self,
        previous: Option<&ProgramSnapshot>,
        roots: &[String],
        mut options: CompilerOptions,
        fs: Arc<dyn FS>,
        changes: &FileChanges,
    ) -> ApiResult<ProgramSnapshot> {
        let session = Arc::clone(&self.session);
        let executor = Arc::clone(&self.executor);
        let roots = roots.to_vec();
        let previous = previous.cloned();
        let changes = changes.clone();
        self.executor.run(move || {
            let previous = previous.as_ref();
            crate::ohsdkplugin::prepare_class_plugins(&mut options)?;
            // Synthetic command lines/options are otherwise batch-arena allocations.
            // Capture their regions in every snapshot that can still reach them.
            let region = Region::new(64 << 10);
            let scope = region.enter();
            let request = APICreateProgramRequest {
                root_file_names: roots
                    .iter()
                    .map(|p| tspath::get_normalized_absolute_path(p, session.current_directory()))
                    .collect(),
                compiler_options: Some(P::new(options)),
                project_references: Vec::new(),
                config_file_parsing_diagnostics: Vec::new(),
                module_resolver_factory: None,
                module_resolver_id: 0,
            };
            let mut req = APISnapshotRequest {
                file_system: Some(fs),
                ..Default::default()
            };
            if let Some(previous) = previous {
                req.reconfigure_programs.push(APIReconfigureProgramRequest {
                    program_id: previous
                        .data
                        .project
                        .synthetic()
                        .ok_or_else(|| ApiError::internal("expected synthetic program"))?,
                    request,
                });
            } else {
                req.create_programs.push(request);
            }
            let mut summary = FileChangeSummary::default();
            let uri = |p: &str| {
                tsrs_ls::lsconv::file_name_to_document_uri(&tspath::get_normalized_absolute_path(
                    p,
                    session.current_directory(),
                ))
            };
            for path in &changes.changed {
                summary.changed.add(uri(path));
            }
            for path in &changes.created {
                summary.created.add(uri(path));
            }
            for path in &changes.deleted {
                summary.deleted.add(uri(path));
            }
            summary.includes_watch_change_outside_node_modules = !summary.is_empty();
            let root = previous
                .is_none()
                .then(|| SnapshotReference(session.snapshot_host.new_root_snapshot()));
            let base = previous.map_or_else(
                || &*root.as_ref().unwrap().0,
                |p| &*p.data.snapshot,
            );
            let result = session.snapshot_host.clone_snapshot(
                &Context::background(),
                base,
                summary,
                Some(Arc::new(req)),
            );
            let snapshot = match result {
                Ok(snapshot) => SnapshotReference(snapshot),
                Err((snapshot, err)) => {
                    snapshot.deref();
                    return Err(ApiError::client(err.message));
                }
            };
            for project in snapshot.0.project_collection.projects() {
                if let Some(error) = project
                    .get_program()
                    .and_then(Program::module_resolution_error)
                {
                    return Err(ApiError::internal(error.to_string()));
                }
            }
            let project = match previous {
                Some(previous) => previous.data.project.clone(),
                None => snapshot
                    .0
                    .created_programs()
                    .first()
                    .and_then(Option::as_ref)
                    .ok_or_else(|| ApiError::internal("create program returned no program"))?
                    .id(),
            };
            let mut regions = Vec::new();
            for project in snapshot.0.project_collection.projects() {
                for command_line in project
                    .command_line
                    .into_iter()
                    .chain(project.get_program().map(Program::command_line))
                {
                    if let Some(owner) = Region::containing(command_line.addr()) {
                        regions.push(owner);
                    }
                    if let Some(owner) = command_line
                        .compiler_options()
                        .and_then(|o| Region::containing(o.addr()))
                    {
                        regions.push(owner);
                    }
                }
            }
            drop(scope);
            regions.push(region);
            Ok(ProgramSnapshot {
                data: Arc::new(ProgramData {
                    snapshot,
                    project,
                    session: Arc::clone(&session),
                    executor,
                    _regions: regions,
                }),
            })
        })
    }
}

struct SnapshotReference(Arc<Snapshot>);
impl Drop for SnapshotReference {
    fn drop(&mut self) {
        self.0.deref();
    }
}
impl std::ops::Deref for SnapshotReference {
    type Target = Snapshot;
    fn deref(&self) -> &Snapshot {
        &self.0
    }
}

// Field order: free the program/checkers before their command-line and options regions.
struct ProgramData {
    snapshot: SnapshotReference,
    project: ID,
    session: Arc<Session>,
    executor: Arc<executor::Executor>,
    _regions: Vec<Region>,
}

/// A clone retains the same immutable snapshot. Dropping the last lease frees it.
#[derive(Clone)]
pub struct ProgramSnapshot {
    data: Arc<ProgramData>,
}
impl std::fmt::Debug for ProgramSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgramSnapshot")
            .field("snapshot", &self.data.snapshot.id())
            .field("project", &self.data.project)
            .finish()
    }
}
impl ProgramSnapshot {
    fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Self) -> ApiResult<T> + Send + 'static,
    ) -> ApiResult<T> {
        let snapshot = self.clone();
        self.data.executor.run(move || f(&snapshot))
    }
    fn program(&self) -> ApiResult<&'static Program> {
        self.data
            .snapshot
            .project_collection
            .get_project(&self.data.project)
            .and_then(|p| p.get_program())
            .ok_or_else(|| ApiError::client("snapshot has no program"))
    }
}

/// The private pointer can only be dereferenced while the owning snapshot is held.
#[derive(Clone, Debug)]
pub struct TypeHandle {
    pub id: u32,
    pub flags: u32,
    typ: P<tsrs_checker::Type>,
    snapshot: ProgramSnapshot,
}
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
}

fn protect<T>(f: impl FnOnce() -> ApiResult<T>) -> ApiResult<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        Err(ApiError::internal(format!("compiler panic: {message}")))
    })
}
