use super::{ProgramSnapshot, Symbol, TypeHandle, facts::*, protect};
use crate::{ApiError, ApiResult};
use std::{path::PathBuf, sync::Arc};
use tsrs_ast::{self as ast, Node, SourceFile};
use tsrs_checker::{
    AnnotationConstant as Constant, AnnotationInfo as Info, Checker, LiteralValue, TypeFlags,
    TypeFormatFlags,
};
use tsrs_core::{
    P,
    context::{CheckerLifetime, Context, with_checker_lifetime},
};

impl ProgramSnapshot {
    fn with_checker<T>(&self, f: impl FnOnce(&mut Checker) -> ApiResult<T>) -> ApiResult<T> {
        protect(|| {
            let program = self.program()?;
            let _lease =
                crate::checker::lease::acquire(program).map_err(|e| ApiError::client(e.message))?;
            let mut checker = program.get_type_checker(&with_checker_lifetime(
                &Context::background(),
                CheckerLifetime::API,
            ));
            f(&mut checker)
        })
    }

    fn type_handle(&self, typ: P<tsrs_checker::Type>) -> TypeHandle {
        TypeHandle {
            id: typ.id().0,
            flags: typ.flags().bits(),
            typ,
            snapshot: self.clone(),
        }
    }
    fn validate_type(&self, typ: &TypeHandle) -> ApiResult<()> {
        if !Arc::ptr_eq(&self.data, &typ.snapshot.data) {
            return Err(ApiError::client("type belongs to another program snapshot"));
        }
        Ok(())
    }
    pub fn types_at_positions(&self, file: &str, positions: &[u32]) -> ApiResult<Vec<TypeHandle>> {
        let file = file.to_string();
        let positions = positions.to_vec();
        self.run(move |this| {
            this.with_checker(|checker| {
                let file = this
                    .program()?
                    .get_source_file(&file)
                    .ok_or_else(|| ApiError::client(format!("source file not found: {file}")))?;
                Ok(positions
                    .iter()
                    .map(|&position| {
                        let offset = file
                            .get_position_map()
                            .utf16_to_utf8(position);
                        let node = tsrs_astnav::get_touching_property_name(file, offset);
                        this.type_handle(checker.get_type_at_location(node))
                    })
                    .collect())
            })
        })
    }
    pub fn non_nullable_type(&self, typ: &TypeHandle) -> ApiResult<TypeHandle> {
        self.validate_type(typ)?;
        let typ = typ.clone();
        self.run(move |this| {
            this.with_checker(
                |checker| Ok(this.type_handle(checker.get_non_nullable_type(typ.typ))),
            )
        })
    }
    pub fn type_symbol(&self, typ: &TypeHandle) -> ApiResult<Option<Symbol>> {
        self.validate_type(typ)?;
        let typ = typ.clone();
        self.run(move |this| {
            this.with_checker(|_| {
                Ok(typ.typ.symbol().map(|s| Symbol {
                    name: s.name().to_string(),
                }))
            })
        })
    }

    #[expect(
        clippy::iter_over_hash_type,
        reason = "dependency and type reference lists are sorted before returning"
    )]
    #[expect(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "compiler paths are already normalized, so an exact suffix matches the Go check"
    )]
    pub fn source_graph(&self) -> ApiResult<ProgramSourceGraph> {
        self.run(|this| {
            let program = this.program()?;
            let mut files = Vec::new();
            for file in program.get_source_files() {
                let mut dependencies = Vec::new();
                if let Some(resolutions) = program.get_resolved_modules().get(file.path()) {
                    for resolution in resolutions.values() {
                        if resolution.is_resolved()
                            && (resolution.resolved_file_name.ends_with(".ets")
                                || resolution.resolved_file_name.ends_with(".ts"))
                        {
                            dependencies.push(PathBuf::from(resolution.resolved_file_name));
                        }
                    }
                }
                dependencies.sort_unstable();
                dependencies.dedup();
                files.push(ProgramSourceGraphFile {
                    file_name: file.file_name().into(),
                    dependencies,
                });
            }
            let mut type_reference_files = Vec::new();
            for resolutions in program.get_resolved_type_reference_directives().values() {
                for resolution in resolutions.values() {
                    if resolution.is_resolved() {
                        type_reference_files.push(PathBuf::from(resolution.resolved_file_name));
                    }
                }
            }
            type_reference_files.sort_unstable();
            type_reference_files.dedup();
            Ok(ProgramSourceGraph {
                files,
                type_reference_files,
            })
        })
    }

    pub fn annotation_transform_infos(
        &self,
        files: &[String],
    ) -> ApiResult<Vec<AnnotationTransformInfo>> {
        let files = files.to_vec();
        self.run(move |this| {
            let program = this.program()?;
            let mut result = Vec::new();
            for name in &files {
                if let Some(file) = program.get_source_file(name) {
                    let mut checker = program.get_arkts_build_checker_for_file(file);
                    result.push(annotation_transform_info(&mut checker, file));
                }
            }
            Ok(result)
        })
    }

    pub fn arkts_transform_type_facts(
        &self,
        queries: &[ArktsTransformTypeQuery],
        sdk_files: &[String],
    ) -> ApiResult<Vec<ArktsTransformTypeFacts>> {
        let queries = queries.to_vec();
        let sdk_files = sdk_files.to_vec();
        self.run(move |this| {
            transform_type_facts(
                this.program()?,
                &queries,
                &sdk_files,
                &Context::background(),
            )
        })
    }
}

pub(crate) fn annotation_constant(value: Option<&Constant>) -> Option<AnnotationConstant> {
    match value? {
        Constant::Array(values) => Some(AnnotationConstant {
            kind: "array".into(),
            value: String::new(),
            items: values
                .iter()
                .filter_map(|v| annotation_constant(Some(v)))
                .collect(),
        }),
        Constant::Scalar(value) => {
            let (kind, value) = match value {
                LiteralValue::String(v) => ("string", v.to_string()),
                LiteralValue::Boolean(v) => ("boolean", v.to_string()),
                LiteralValue::Number(v) => (
                    "number",
                    if v.0 == f64::INFINITY {
                        "Infinity".into()
                    } else if v.0 == f64::NEG_INFINITY {
                        "-Infinity".into()
                    } else {
                        v.0.to_string()
                    },
                ),
                LiteralValue::BigInt(_) => return None,
            };
            Some(AnnotationConstant {
                kind: kind.into(),
                value,
                items: Vec::new(),
            })
        }
    }
}

pub(crate) fn annotation_info(checker: &mut Checker, info: &Info) -> AnnotationInfo {
    let flags = TypeFormatFlags::AllowUniqueESSymbolType
        | TypeFormatFlags::UseAliasDefinedOutsideCurrentScope;
    AnnotationInfo {
        source_retention: info.source_retention,
        properties: info
            .properties
            .iter()
            .map(|p| {
                let name = p.declaration.name().unwrap();
                AnnotationProperty {
                    name: ast::try_get_text_of_property_name(name).unwrap_or_default(),
                    name_text: tsrs_scanner::get_text_of_node(name),
                    initializer: annotation_constant(p.initializer.as_ref()),
                    argument: annotation_constant(p.argument.as_ref()),
                    array_depth: p.array_depth,
                    enum_first_value: annotation_constant(p.enum_first_value.as_ref()),
                    type_text: checker.type_to_string_ex_exported(
                        p.typ,
                        Some(p.declaration),
                        flags,
                        None,
                    ),
                    element_type_text: checker.type_to_string_ex_exported(
                        p.element_type,
                        Some(p.declaration),
                        flags,
                        None,
                    ),
                }
            })
            .collect(),
    }
}

pub(crate) fn collect_nodes(node: P<Node>, nodes: &mut Vec<P<Node>>) {
    node.for_each_child(&mut |child| {
        nodes.push(child);
        collect_nodes(child, nodes);
        false
    });
}

fn annotation_transform_info(
    checker: &mut Checker,
    file: P<SourceFile>,
) -> AnnotationTransformInfo {
    let mut result = AnnotationTransformInfo {
        file_name: file.file_name().into(),
        declarations: Vec::new(),
        uses: Vec::new(),
        imports: Vec::new(),
    };
    let mut nodes = Vec::new();
    collect_nodes(file.as_node(), &mut nodes);
    let positions = file.get_position_map();
    for node in nodes {
        let pos = positions.utf8_to_utf16(node.pos().source_offset().unwrap_or(0)) as usize;
        let end = positions.utf8_to_utf16(node.end().source_offset().unwrap_or(0)) as usize;
        if ast::is_annotation_declaration(node) || ast::is_decorator(node) {
            if let Some(info) = checker.get_annotation_info(node) {
                let runtime_retained = ast::is_decorator(node)
                    && node.parent().is_some_and(|owner| {
                        (ast::is_class_declaration(owner)
                            && !ast::is_annotation_declaration(owner)
                            && !ast::is_struct_declaration(owner))
                            || ast::is_method_declaration(owner)
                    });
                let value = AnnotationTransformNode {
                    pos,
                    end,
                    runtime_retained,
                    info: annotation_info(checker, &info),
                };
                if ast::is_annotation_declaration(node) {
                    result.declarations.push(value);
                } else {
                    result.uses.push(value);
                }
            }
        } else if ast::is_import_specifier(node) {
            let disposition = checker.get_annotation_import_disposition(node);
            if disposition != "unchanged" {
                result.imports.push(AnnotationTransformImport {
                    pos,
                    end,
                    disposition: disposition.into(),
                });
            }
        }
    }
    result
}

pub(crate) fn type_identity(typ: P<tsrs_checker::Type>) -> ResolvedTypeIdentity {
    use TypeFlags as F;
    let flags = typ.flags();
    let basic = F::String
        | F::Number
        | F::Boolean
        | F::Enum
        | F::BigInt
        | F::StringLiteral
        | F::NumberLiteral
        | F::BooleanLiteral
        | F::EnumLiteral
        | F::BigIntLiteral;
    let symbol = typ.symbol();
    let name = symbol.map_or("", |symbol| symbol.name());
    let observed = symbol.is_some_and(|symbol| {
        symbol.declarations().iter().any(|&node| {
            ast::is_class_declaration(node)
                && ast::has_arkui_bare_decorator(node.modifiers(), &["ObservedV2"])
        })
    });
    ResolvedTypeIdentity {
        is_nullable: flags.intersects(F::Nullable),
        is_union: flags.intersects(F::Union),
        is_enum: flags.intersects(F::EnumLike),
        is_basic: flags.intersects(basic),
        is_observed_v2: observed,
        is_function: name == "Function",
        symbol_name: name.into(),
        types: if flags.intersects(F::UnionOrIntersection) {
            typ.types().iter().map(|&t| type_identity(t)).collect()
        } else {
            Vec::new()
        },
    }
}

pub(crate) fn transform_type_facts(
    program: &'static tsrs_compiler::Program,
    queries: &[ArktsTransformTypeQuery],
    sdk_files: &[String],
    context: &Context,
) -> ApiResult<Vec<ArktsTransformTypeFacts>> {
    let mut result = Vec::new();
    for query in queries {
        let name = query
            .file
            .to_str()
            .ok_or_else(|| ApiError::client("query path is not UTF-8"))?;
        let Some(file) = program.get_source_file(name) else {
            continue;
        };
        let mut checker = program.get_arkts_build_checker_for_file(file);
        let positions = file.get_position_map();
        let ends = |ranges: &[TypeQueryRange]| -> ApiResult<rustc_hash::FxHashSet<u32>> {
            ranges
                .iter()
                .map(|r| {
                    let end = u32::try_from(r.end).map_err(|_| {
                        ApiError::client("query range exceeds compiler offset range")
                    })?;
                    Ok(positions.utf16_to_utf8(end))
                })
                .collect()
        };
        let property_ends = ends(&query.properties)?;
        let builder_ends = ends(&query.builder_accesses)?;
        let member_ends = ends(&query.member_accesses)?;
        let mut facts = ArktsTransformTypeFacts {
            file_name: file.file_name().into(),
            properties: Vec::new(),
            builder_accesses: Vec::new(),
            member_accesses: Vec::new(),
            sdk_api_uses: Vec::new(),
        };
        if !property_ends.is_empty() || !builder_ends.is_empty() || !member_ends.is_empty() {
            let mut nodes = Vec::new();
            collect_nodes(file.as_node(), &mut nodes);
            for node in nodes {
                if ast::is_property_declaration(node)
                    && !node.parent().is_some_and(ast::is_annotation_declaration)
                    && node
                        .name()
                        .is_some_and(|name| property_ends.contains(&name.end().source_offset().unwrap_or(0)))
                {
                    let name = node.name().unwrap();
                    facts.properties.push(ArktsPropertyTypeFacts {
                        name_pos: positions.utf8_to_utf16(name.pos().source_offset().unwrap_or(0)) as usize,
                        name_end: positions.utf8_to_utf16(name.end().source_offset().unwrap_or(0)) as usize,
                        r#type: node.type_node().map(|node| {
                            type_identity(checker.get_type_from_type_node_exported(node))
                        }),
                    });
                }
                if ast::is_property_access_expression(node) {
                    let pos = positions.utf8_to_utf16(node.pos().source_offset().unwrap_or(0)) as usize;
                    let end = positions.utf8_to_utf16(node.end().source_offset().unwrap_or(0)) as usize;
                    if builder_ends.contains(&node.end().source_offset().unwrap_or(0))
                        && node.name().unwrap().text() == "builder"
                    {
                        facts.builder_accesses.push(ArktsBuilderReceiverTypeFacts {
                            pos,
                            end,
                            receiver_type: type_identity(
                                checker.get_type_at_location(node.expression().unwrap()),
                            ),
                        });
                    }
                    if member_ends.contains(&node.end().source_offset().unwrap_or(0))
                        && ast::is_identifier(node.expression().unwrap())
                    {
                        facts.member_accesses.push(ArktsExpressionTypeFacts {
                            pos,
                            end,
                            r#type: type_identity(checker.get_type_at_location(node)),
                        });
                    }
                }
            }
        }
        result.push(facts);
    }
    for name in sdk_files {
        let Some(file) = program.get_source_file(name) else {
            continue;
        };
        let mut checker = program.get_arkts_build_checker_for_file(file);
        checker.get_diagnostics_exported(context, file);
        let facts = checker.oh_sdk_use_facts(file.file_name());
        if facts.is_empty() {
            continue;
        }
        let index = result
            .iter()
            .position(|r| r.file_name == std::path::Path::new(file.file_name()))
            .unwrap_or_else(|| {
                result.push(ArktsTransformTypeFacts {
                    file_name: file.file_name().into(),
                    properties: Vec::new(),
                    builder_accesses: Vec::new(),
                    member_accesses: Vec::new(),
                    sdk_api_uses: Vec::new(),
                });
                result.len() - 1
            });
        result[index].sdk_api_uses = facts
            .into_iter()
            .map(|f| SdkApiUse {
                api_module: f.api_module,
                function: f.function,
            })
            .collect();
    }
    Ok(result)
}
