//! OH API annotation and transform facts. All types come from the checker.
use tsrs_ast::{self as ast, SourceFile};
use tsrs_checker::{AnnotationConstant, AnnotationInfo, LiteralValue, TypeFormatFlags};
use tsrs_core::{json::Value, P};
use crate::wire::{Obj, b, s};
use crate::embedded::{queries::collect_nodes, ArktsTransformTypeQuery, TypeQueryRange, ResolvedTypeIdentity};
use super::{host::{CheckerHost, CheckerResult}, params::Params, setup::{Setup, SnapshotCtx}};

fn annotation_constant(value: Option<&AnnotationConstant>) -> Value {
    let Some(value) = value else { return Value::Null; };
    match value {
        AnnotationConstant::Array(values) => Obj::new().set("kind", s("array")).set("value", s(""))
            .set("items", Value::Array(values.iter().map(|v| annotation_constant(Some(v))).collect())).build(),
        AnnotationConstant::Scalar(value) => {
            let (kind, value) = match value {
                LiteralValue::String(v) => ("string", v.to_string()),
                LiteralValue::Boolean(v) => ("boolean", v.to_string()),
                LiteralValue::Number(v) => ("number", if v.0 == f64::INFINITY { "Infinity".into() }
                    else if v.0 == f64::NEG_INFINITY { "-Infinity".into() } else { v.0.to_string() }),
                LiteralValue::BigInt(_) => return Value::Null,
            };
            Obj::new().set("kind", s(kind)).set("value", s(value)).build()
        }
    }
}

impl Setup<'_> {
    fn annotation_info_response(&mut self, info: &AnnotationInfo) -> CheckerResult<Value> {
        let mut properties = Vec::new();
        let flags = TypeFormatFlags::AllowUniqueESSymbolType | TypeFormatFlags::UseAliasDefinedOutsideCurrentScope;
        for property in &info.properties {
            let name = property.declaration.name().unwrap();
            let type_text = self.c().type_to_string_ex_exported(property.typ, Some(property.declaration), flags, None);
            let element_text = self.c().type_to_string_ex_exported(property.element_type, Some(property.declaration), flags, None);
            properties.push(Obj::new()
                .set("name", s(ast::try_get_text_of_property_name(name).unwrap_or_default()))
                .set("nameText", s(tsrs_scanner::get_text_of_node(name)))
                .set("declaration", s(self.sd.host.node_handle(property.declaration)?))
                .set("type", self.type_response(property.typ)?)
                .set("initializer", annotation_constant(property.initializer.as_ref()))
                .set("argument", annotation_constant(property.argument.as_ref()))
                .set("arrayDepth", Value::Number(property.array_depth as f64))
                .set("elementType", self.type_response(property.element_type)?)
                .set_opt("enumDeclaration", property.enum_declaration.map(|node| self.sd.host.node_handle(node).map(s)).transpose()?)
                .set("enumFirstValue", annotation_constant(property.enum_first_value.as_ref()))
                .set("typeText", s(type_text)).set("elementTypeText", s(element_text)).build());
        }
        Ok(Obj::new().set("declaration", s(self.sd.host.node_handle(info.declaration)?))
            .set("sourceRetention", b(info.source_retention)).set("properties", Value::Array(properties)).build())
    }

    fn annotation_transform_info(&mut self, file: P<SourceFile>) -> CheckerResult<Value> {
        let mut declarations = Vec::new();
        let mut uses = Vec::new();
        let mut imports = Vec::new();
        let mut nodes = Vec::new();
        collect_nodes(file.as_node(), &mut nodes);
        let positions = file.get_position_map();
        for node in nodes {
            let range = Obj::new().set("pos", Value::Number(positions.utf8_to_utf16(node.pos()) as f64))
                .set("end", Value::Number(positions.utf8_to_utf16(node.end()) as f64));
            if ast::is_annotation_declaration(node) || ast::is_decorator(node) {
                if let Some(info) = self.c().get_annotation_info(node) {
                    let runtime = ast::is_decorator(node) && node.parent().is_some_and(|owner|
                        (ast::is_class_declaration(owner) && !ast::is_annotation_declaration(owner) && !ast::is_struct_declaration(owner))
                        || ast::is_method_declaration(owner));
                    let response = range.set("runtimeRetained", b(runtime)).set("info", self.annotation_info_response(&info)?).build();
                    if ast::is_annotation_declaration(node) { declarations.push(response); } else { uses.push(response); }
                }
            } else if ast::is_import_specifier(node) {
                let disposition = self.c().get_annotation_import_disposition(node);
                if disposition != "unchanged" { imports.push(range.set("disposition", s(disposition)).build()); }
            }
        }
        Ok(Obj::new().set("fileName", s(file.file_name())).set("declarations", Value::Array(declarations))
            .set("uses", Value::Array(uses)).set("imports", Value::Array(imports)).build())
    }
}

pub(crate) fn get_annotation_info(host: &dyn CheckerHost, p: &Params) -> CheckerResult<Value> {
    let mut setup = Setup::new(host, p.u64("snapshot")?, p.project()?)?;
    let node = setup.resolve_node(p.string("location")?)?;
    match setup.c().get_annotation_info(node) {
        Some(info) => setup.annotation_info_response(&info),
        None => Ok(Value::Null),
    }
}

pub(crate) fn get_annotation_transform_infos(host: &dyn CheckerHost, p: &Params) -> CheckerResult<Value> {
    let mut setup = Setup::new(host, p.u64("snapshot")?, p.project()?)?;
    let mut result = Vec::new();
    for name in p.documents("files")? {
        if let Some(file) = setup.program.get_source_file(&name.to_file_name()) {
            result.push(setup.annotation_transform_info(file)?);
        }
    }
    Ok(Value::Array(result))
}

fn identity_response(t: &ResolvedTypeIdentity) -> Value {
    Obj::new().set("isNullable", b(t.is_nullable)).set("isUnion", b(t.is_union))
        .set("isEnum", b(t.is_enum)).set("isBasic", b(t.is_basic)).set("isObservedV2", b(t.is_observed_v2))
        .set("isFunction", b(t.is_function)).set("symbolName", s(&t.symbol_name))
        .set("types", Value::Array(t.types.iter().map(identity_response).collect())).build()
}

pub(crate) fn get_transform_type_facts(host: &dyn CheckerHost, p: &Params) -> CheckerResult<Value> {
    let sd = SnapshotCtx::new(host, p.u64("snapshot")?, p.project()?)?;
    let mut queries = Vec::new();
    for query in p.array("queries")? {
        let query = Params::new(query, "ArkTSTransformTypeFactsFileQuery")?;
        let ranges = |key| -> CheckerResult<Vec<TypeQueryRange>> {
            query.array(key)?.iter().map(|item| {
                let item = Params::new(item, "ArkTSTypeQueryRange")?;
                Ok(TypeQueryRange { pos: 0, end: item.u32("end")? as usize })
            }).collect()
        };
        queries.push(ArktsTransformTypeQuery { file: query.document("file")?.to_file_name().into(),
            properties: ranges("properties")?, builder_accesses: ranges("builderAccesses")?, member_accesses: ranges("memberAccesses")? });
    }
    let sdk_files: Vec<_> = p.documents("sdkApiUseFiles")?.iter().map(|d| d.to_file_name()).collect();
    let facts = crate::embedded::queries::transform_type_facts(sd.program()?, &queries, &sdk_files, &host.context()).map_err(super::from_api_error)?;
    Ok(Value::Array(facts.iter().map(|f| {
        let properties = f.properties.iter().map(|p| Obj::new().set("namePos", Value::Number(p.name_pos as f64))
            .set("nameEnd", Value::Number(p.name_end as f64)).set_opt("type", p.r#type.as_ref().map(identity_response)).build()).collect();
        let builders = f.builder_accesses.iter().map(|p| Obj::new().set("pos", Value::Number(p.pos as f64))
            .set("end", Value::Number(p.end as f64)).set("receiverType", identity_response(&p.receiver_type)).build()).collect();
        let members = f.member_accesses.iter().map(|p| Obj::new().set("pos", Value::Number(p.pos as f64))
            .set("end", Value::Number(p.end as f64)).set("type", identity_response(&p.r#type)).build()).collect();
        Obj::new().set("fileName", s(f.file_name.to_str().unwrap())).set("properties", Value::Array(properties))
            .set("builderAccesses", Value::Array(builders)).set("memberAccesses", Value::Array(members))
            .set("sdkApiUses", Value::Array(f.sdk_api_uses.iter().map(|p| Obj::new().set("apiModule", s(&p.api_module))
                .set("function", s(&p.function)).build()).collect())).build()
    }).collect()))
}
