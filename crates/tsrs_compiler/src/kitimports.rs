//! OH compiler/kitimports.go: semantic @kit expansion before binding.
use std::sync::{Arc, Mutex};

use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use tsrs_ast::{self as ast, Kind, Node, NodeFactory, NodeFlags, SourceFile, TokenFlags};
use tsrs_core::{tspath, ScriptKind, TextRange, P};
use tsrs_vfs::FS;

#[derive(Deserialize)]
struct KitSymbolInfo {
    #[serde(default)]
    source: String,
    #[serde(default)]
    bindings: String,
}

#[derive(Deserialize, Default)]
struct KitJsonInfo {
    #[serde(default)]
    symbols: Option<FxHashMap<String, KitSymbolInfo>>,
}

#[derive(Default)]
pub struct KitImportProcessor {
    cache: Mutex<FxHashMap<String, Option<Arc<KitJsonInfo>>>>,
}

const COMPONENTS: &[&str] = &[
    "ArcList", "ArcListItem", "MovingPhotoView", "ArcSwiper", "ArcScrollBar", "ArcAlphabetIndexer",
    "HdsNavigation", "HdsNavDestination", "HdsTabs", "DotMatrix", "Metaball", "HdsListItemCard",
    "HdsVisualComponent", "MultiWindowEntryInAPP", "AudioWave",
];
const API_MODULES: &[&str] = &[
    "@ohos.arkui.ArcList", "@ohos.arkui.ArcSwiper", "@ohos.arkui.ArcScrollBar", "@ohos.arkui.ArcAlphabetIndexer",
    "@ohos.multimedia.movingphotoview", "@hms.hds.hdsBaseComponent", "@hms.hds.HdsVisualComponent",
    "@hms.hds.dotMatrix", "@hms.hds.Metaball", "@hms.hds.AudioWave",
];
const KIT_MODULES: &[&str] = &["@kit.ArkUI", "@kit.MediaLibraryKit", "@kit.UIDesignKit"];
const TS_MODULES: &[&str] = &[
    "@kit.AccountKit", "@kit.MapKit", "@kit.Penkit", "@kit.ScenarioFusionKit", "@kit.ServiceCollaborationKit",
    "@kit.SpeechKit", "@kit.VisionKit", "@kit.PDFKit", "@kit.ReaderKit",
];
type ComponentMap = FxHashMap<String, Vec<String>>;

impl KitImportProcessor {
    /// Read the inputs of the pre-bind transform even on a parse-cache hit. Reading through
    /// the snapshot FS registers both present and missing configs as watched dependencies.
    pub fn configuration_inputs(file: P<SourceFile>, fs: &dyn FS, mut visit: impl FnMut(&str, Option<&str>)) -> bool {
        let options = file.parse_options();
        if options.no_transformed_kit_in_parser || options.ets_loader_path.is_empty() || !file.diagnostics().is_empty() {
            return false;
        }
        let in_ets = file.script_kind() == ScriptKind::ETS;
        let mut needed = false;
        for &statement in file.statements().nodes() {
            if in_ets && !ast::is_import_declaration(statement) { break; }
            if !file.file_name().ends_with(".d.ts") && Self::is_valid_named_import(statement)
                && API_MODULES.contains(&statement.as_import_declaration().module_specifier.text()) {
                needed = true;
                continue;
            }
            if Self::exclude_kit(statement) { continue; }
            let module = statement.as_import_declaration().module_specifier.text();
            if !in_ets && TS_MODULES.contains(&module) { continue; }
            needed = true;
            for path in Self::kit_config_paths(module, &options.ets_loader_path) {
                let text = fs.read_file(&path);
                visit(&path, text.as_deref());
                if text.is_some() { break; }
            }
        }
        needed
    }

    /// Called inside the parse allocation scope, before the file is bound or shared.
    pub fn process(&self, file: P<SourceFile>, fs: &dyn FS) -> bool {
        let options = file.parse_options();
        if options.no_transformed_kit_in_parser || options.ets_loader_path.is_empty() || !file.diagnostics().is_empty() {
            return false;
        }
        let in_ets = file.script_kind() == ScriptKind::ETS;
        let mut api_map = ComponentMap::default();
        let mut kit_map = ComponentMap::default();
        let mut existing = FxHashSet::default();
        for &statement in file.statements().nodes() {
            if !Self::is_valid_named_import(statement) { continue; }
            let declaration = statement.as_import_declaration();
            let module = declaration.module_specifier.text();
            let destination = if API_MODULES.contains(&module) { &mut api_map } else if KIT_MODULES.contains(&module) { &mut kit_map } else { continue };
            let named = declaration.import_clause.unwrap().as_import_clause().named_bindings.unwrap();
            for &element in named.as_named_imports().elements().nodes() {
                let name = element.name().unwrap().text();
                if COMPONENTS.contains(&name) {
                    let set = destination.entry(module.to_string()).or_default();
                    if !set.iter().any(|value| value == name) { set.push(name.to_string()); }
                } else if name.strip_suffix("Attribute").is_some_and(|name| COMPONENTS.contains(&name)) {
                    existing.insert(name.to_string());
                }
            }
        }
        let factory = NodeFactory::default();
        let mut statements = Vec::new();
        let mut ranges = Vec::new();
        let mut skip_rest = false;
        let mut changed = false;
        for &statement in file.statements().nodes() {
            if !skip_rest && in_ets && !ast::is_import_declaration(statement) { skip_rest = true; }
            if !skip_rest && !file.file_name().ends_with(".d.ts") && Self::is_valid_named_import(statement)
                && API_MODULES.contains(&statement.as_import_declaration().module_specifier.text()) {
                let module = statement.as_import_declaration().module_specifier.text();
                let updated = if let Some(components) = api_map.remove(module) {
                    Self::supplement_named_bindings(&factory, statement, &components, &mut existing)
                } else { statement };
                statements.push(updated);
                changed |= updated != statement;
                continue;
            }
            if skip_rest || Self::exclude_kit(statement) { statements.push(statement); continue; }
            let module = statement.as_import_declaration().module_specifier.text();
            if !in_ets && TS_MODULES.contains(&module) { statements.push(statement); continue; }
            let info = self.get_kit_json(module, &options.ets_loader_path, fs);
            let imports = info.as_deref().and_then(|info| Self::process_kit_declaration(&factory, statement, info, in_ets, &mut kit_map, &mut existing));
            if let Some(imports) = imports {
                statements.extend(imports);
                ranges.push(statement.loc());
                changed = true;
            } else { statements.push(statement); }
        }
        if !changed { return false; }
        let list = factory.new_node_list(statements);
        list.loc.set(file.statements().loc.get());
        file.statements.set(list);
        file.marked_kit_import_ranges.set(tsrs_core::alloc_vec(ranges));
        ast::set_parent_in_children(file.as_node());
        ast::set_external_module_indicator(&file, options.external_module_indicator_options);
        tsrs_parser::recollect_external_module_references(file);
        true
    }

    fn get_kit_json(&self, name: &str, loader: &str, fs: &dyn FS) -> Option<Arc<KitJsonInfo>> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(info) = cache.get(name) { return info.clone(); }
        let text = Self::kit_config_paths(name, loader).iter().find_map(|path| fs.read_file(path));
        let info = text.filter(|text| !text.is_empty()).map(|text| Arc::new(serde_json::from_str(&text).unwrap_or_else(|error| panic!("invalid OpenHarmony kit config {name}: {error}"))));
        cache.insert(name.to_string(), info.clone());
        info
    }

    fn kit_config_paths(name: &str, loader: &str) -> [String; 2] {
        let mixed = tspath::normalize_path(loader).ends_with("dynamic/build-tools/ets-loader");
        let sdk = tspath::resolve_path(loader, &[if mixed { "../../../../.." } else { "../../../.." }]);
        let roots = if mixed {
            ["openharmony/ets/dynamic/build-tools/ets-loader/kit_configs", "hms/ets/dynamic/build-tools/ets-loader/kit_configs"]
        } else { ["openharmony/ets/build-tools/ets-loader/kit_configs", "hms/ets/build-tools/ets-loader/kit_configs"] };
        roots.map(|root| tspath::resolve_path(&sdk, &[root, &format!("{name}.json")]))
    }

    fn is_valid_named_import(statement: P<Node>) -> bool {
        if !ast::is_import_declaration(statement) { return false; }
        let declaration = statement.as_import_declaration();
        ast::is_string_literal(declaration.module_specifier) && declaration.import_clause
            .and_then(|clause| clause.as_import_clause().named_bindings).is_some_and(ast::is_named_imports)
    }

    fn exclude_kit(statement: P<Node>) -> bool {
        if !ast::is_import_declaration(statement) { return true; }
        let declaration = statement.as_import_declaration();
        let Some(clause) = declaration.import_clause else { return true; };
        statement.modifiers().is_some() || declaration.attributes().is_some() || !ast::is_string_literal(declaration.module_specifier)
            || !declaration.module_specifier.text().starts_with("@kit.")
            || clause.as_import_clause().named_bindings.is_some_and(ast::is_namespace_import)
    }

    fn supplement_named_bindings(factory: &NodeFactory, declaration: P<Node>, components: &[String], existing: &mut FxHashSet<String>) -> P<Node> {
        let import = declaration.as_import_declaration();
        let clause = import.import_clause.unwrap();
        let named = clause.as_import_clause().named_bindings.unwrap();
        let elements = named.as_named_imports().elements().nodes();
        let mut updated = elements.to_vec();
        for component in components {
            let attribute = format!("{component}Attribute");
            if !existing.insert(attribute.clone()) { continue; }
            let identifier = factory.new_identifier(tsrs_core::alloc_str(&attribute));
            identifier.set_virtual(true);
            let specifier = factory.new_import_specifier(false, None, identifier);
            specifier.set_virtual(true);
            updated.push(specifier);
        }
        if updated.len() == elements.len() { return declaration; }
        let named = factory.new_named_imports(factory.new_node_list(updated));
        let clause = factory.update_import_clause(clause, Kind::Unknown, false, None, Some(named));
        factory.update_import_declaration(declaration, declaration.modifiers(), Some(clause), import.module_specifier, import.attributes())
    }

    fn process_kit_declaration(factory: &NodeFactory, declaration: P<Node>, info: &KitJsonInfo, in_ets: bool, kit_map: &mut ComponentMap, existing: &mut FxHashSet<String>) -> Option<Vec<P<Node>>> {
        let symbols = info.symbols.as_ref()?;
        let import = declaration.as_import_declaration();
        let clause_node = import.import_clause.unwrap();
        let clause = clause_node.as_import_clause();
        let module = import.module_specifier.text();
        let mut result = Vec::new();
        if let Some(name) = clause_node.name() {
            let symbol = symbols.get("default")?;
            if !in_ets && symbol.source.ends_with(".d.ets") { return None; }
            result.push(Self::create_kit_import(factory, clause.phase_modifier.get() == Kind::TypeKeyword, clause.is_lazy, name, symbol, declaration, name));
        }
        if clause.named_bindings.is_none() { return Some(result); }
        let working = if let Some(components) = kit_map.remove(module) {
            Self::supplement_named_bindings(factory, declaration, &components, existing)
        } else { declaration };
        let named = working.as_import_declaration().import_clause.unwrap().as_import_clause().named_bindings.unwrap();
        for &element in named.as_named_imports().elements().nodes() {
            let specifier = element.as_import_specifier();
            let name = element.name().unwrap();
            let import_name = specifier.property_name().unwrap_or(name).text();
            if matches!((module, import_name), ("@kit.CoreFileKit", "DfsListeners") | ("@kit.NetworkKit", "VpnExtensionContext") | ("@kit.ArkUI", "CustomContentDialog"))
                || !in_ets && module == "@kit.ConnectivityKit" && import_name == "socket" { return None; }
            let symbol = symbols.get(import_name)?;
            if !in_ets && symbol.source.ends_with(".d.ets") || clause.phase_modifier.get() == Kind::TypeKeyword && specifier.is_type_only { return None; }
            result.push(Self::create_kit_import(factory, clause.phase_modifier.get() == Kind::TypeKeyword || specifier.is_type_only, clause.is_lazy, name, symbol, declaration, element));
        }
        Some(result)
    }

    fn create_kit_import(factory: &NodeFactory, is_type: bool, is_lazy: bool, name: P<Node>, symbol: &KitSymbolInfo, original: P<Node>, original_specifier: P<Node>) -> P<Node> {
        let source = symbol.source.strip_suffix(".d.ets").unwrap_or(&symbol.source);
        let source = source.strip_suffix(".d.ts").unwrap_or(source);
        let module = factory.new_string_literal(tsrs_core::alloc_str(source), TokenFlags::None);
        Self::mark_virtual(module, original.as_import_declaration().module_specifier.loc());
        let phase = if is_type { Kind::TypeKeyword } else { Kind::Unknown };
        let clause = if symbol.bindings == "default" {
            factory.new_import_clause(phase, is_lazy, Some(name), None)
        } else {
            let property = if ast::is_import_specifier(original_specifier) && original_specifier.as_import_specifier().property_name().is_none() && symbol.bindings == name.text() { None } else {
                let property = factory.new_identifier(tsrs_core::alloc_str(&symbol.bindings));
                Self::mark_virtual(property, name.loc());
                Some(property)
            };
            let specifier = factory.new_import_specifier(false, property, name);
            Self::mark_virtual(specifier, original_specifier.loc());
            let named = factory.new_named_imports(factory.new_node_list(vec![specifier]));
            Self::mark_virtual(named, original_specifier.loc());
            factory.new_import_clause(phase, is_lazy, None, Some(named))
        };
        Self::mark_virtual(clause, original_specifier.loc());
        let declaration = factory.new_import_declaration(None, Some(clause), module, None);
        Self::mark_virtual(declaration, original.loc());
        declaration
    }

    fn mark_virtual(node: P<Node>, loc: TextRange) {
        node.set_virtual(true);
        node.set_flags(node.flags() | NodeFlags::KitImport);
        node.set_loc(loc);
    }
}
