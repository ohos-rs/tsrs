//! OH API annotation and transform facts. All types come from the checker.
use tsrs_ast::{self as ast, Node, SourceFile};
use tsrs_checker::{AnnotationConstant, AnnotationInfo, LiteralValue, TypeFormatFlags};
use tsrs_core::{json::Value, P};
use crate::wire::{Obj, b, s};
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

fn collect_nodes(node: P<Node>, nodes: &mut Vec<P<Node>>) {
    node.for_each_child(&mut |child| {
        nodes.push(child);
        collect_nodes(child, nodes);
        false
    });
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

fn type_identity(typ: P<tsrs_checker::Type>) -> Value {
    use tsrs_checker::TypeFlags as F;
    let flags = typ.flags();
    let basic = F::String | F::Number | F::Boolean | F::Enum | F::BigInt | F::StringLiteral
        | F::NumberLiteral | F::BooleanLiteral | F::EnumLiteral | F::BigIntLiteral;
    let symbol = typ.symbol();
    let name = symbol.map_or("", |symbol| symbol.name());
    let observed = symbol.is_some_and(|symbol| symbol.declarations().iter().any(|&node|
        ast::is_class_declaration(node) && ast::has_arkui_bare_decorator(node.modifiers(), &["ObservedV2"])));
    let types = if flags.intersects(F::UnionOrIntersection) { typ.types().iter().map(|&t| type_identity(t)).collect() } else { Vec::new() };
    Obj::new().set("isNullable", b(flags.intersects(F::Nullable))).set("isUnion", b(flags.intersects(F::Union)))
        .set("isEnum", b(flags.intersects(F::EnumLike))).set("isBasic", b(flags.intersects(basic)))
        .set("isObservedV2", b(observed)).set("isFunction", b(name == "Function"))
        .set("symbolName", s(name)).set("types", Value::Array(types)).build()
}

pub(crate) fn get_transform_type_facts(host: &dyn CheckerHost, p: &Params) -> CheckerResult<Value> {
    let sd = SnapshotCtx::new(host, p.u64("snapshot")?, p.project()?)?;
    let program = sd.program()?;
    let mut result = Vec::new();
    for query in p.array("queries")? {
        let query = Params::new(query, "ArkTSTransformTypeFactsFileQuery")?;
        let Some(file) = program.get_source_file(&query.document("file")?.to_file_name()) else { continue; };
        let mut checker = program.get_arkts_build_checker_for_file(file);
        let positions = file.get_position_map();
        let ends = |key| -> CheckerResult<rustc_hash::FxHashSet<i32>> {
            query.array(key)?.iter().map(|item| {
                let item = Params::new(item, "ArkTSTypeQueryRange")?;
                Ok(positions.utf16_to_utf8(item.u32("end")? as i32))
            }).collect()
        };
        let property_ends = ends("properties")?;
        let builder_ends = ends("builderAccesses")?;
        let member_ends = ends("memberAccesses")?;
        let mut properties = Vec::new();
        let mut builders = Vec::new();
        let mut members = Vec::new();
        if !property_ends.is_empty() || !builder_ends.is_empty() || !member_ends.is_empty() {
            let mut nodes = Vec::new();
            collect_nodes(file.as_node(), &mut nodes);
            for node in nodes {
                if ast::is_property_declaration(node) && !node.parent().is_some_and(ast::is_annotation_declaration)
                    && node.name().is_some_and(|name| property_ends.contains(&name.end())) {
                    let name = node.name().unwrap();
                    let typ = node.type_node().map(|node| type_identity(checker.get_type_from_type_node_exported(node)));
                    properties.push(Obj::new().set("namePos", Value::Number(positions.utf8_to_utf16(name.pos()) as f64))
                        .set("nameEnd", Value::Number(positions.utf8_to_utf16(name.end()) as f64)).set_opt("type", typ).build());
                }
                if ast::is_property_access_expression(node) {
                    let range = || Obj::new().set("pos", Value::Number(positions.utf8_to_utf16(node.pos()) as f64))
                        .set("end", Value::Number(positions.utf8_to_utf16(node.end()) as f64));
                    if builder_ends.contains(&node.end()) && node.name().unwrap().text() == "builder" {
                        let typ = checker.get_type_at_location(node.expression().unwrap());
                        builders.push(range().set("receiverType", type_identity(typ)).build());
                    }
                    if member_ends.contains(&node.end()) && ast::is_identifier(node.expression().unwrap()) {
                        let typ = checker.get_type_at_location(node);
                        members.push(range().set("type", type_identity(typ)).build());
                    }
                }
            }
        }
        result.push(Obj::new().set("fileName", s(file.file_name())).set("properties", Value::Array(properties))
            .set("builderAccesses", Value::Array(builders)).set("memberAccesses", Value::Array(members))
            .set("sdkApiUses", Value::Array(Vec::new())).build());
    }
    for name in p.documents("sdkApiUseFiles")? {
        let Some(file) = program.get_source_file(&name.to_file_name()) else { continue; };
        let mut checker = program.get_arkts_build_checker_for_file(file);
        checker.get_diagnostics_exported(&host.context(), file);
        let facts = checker.oh_sdk_use_facts(file.file_name());
        if facts.is_empty() { continue; }
        let index = result.iter().position(|r| crate::wire::Params(r).str("fileName").ok() == Some(file.file_name()));
        let index = index.unwrap_or_else(|| {
            result.push(Obj::new().set("fileName", s(file.file_name())).set("properties", Value::Array(Vec::new()))
                .set("builderAccesses", Value::Array(Vec::new())).set("memberAccesses", Value::Array(Vec::new())).build());
            result.len()-1
        });
        let Value::Object(response) = &mut result[index] else { unreachable!() };
        response.insert("sdkApiUses".to_string(), Value::Array(facts.into_iter().map(|fact|
            Obj::new().set("apiModule", s(fact.api_module)).set("function", s(fact.function)).build()).collect()));
    }
    Ok(Value::Array(result))
}
