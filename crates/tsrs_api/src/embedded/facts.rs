//! Owned `ArkTS` semantic facts. No arena references escape through these values.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTransformInfo {
    pub file_name: PathBuf,
    pub declarations: Vec<AnnotationTransformNode>,
    pub uses: Vec<AnnotationTransformNode>,
    pub imports: Vec<AnnotationTransformImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationTransformNode {
    pub pos: usize,
    pub end: usize,
    #[serde(default, rename = "runtimeRetained")]
    pub runtime_retained: bool,
    pub info: AnnotationInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationTransformImport {
    pub pos: usize,
    pub end: usize,
    pub disposition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationInfo {
    pub source_retention: bool,
    pub properties: Vec<AnnotationProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationProperty {
    pub name: String,
    pub name_text: String,
    pub initializer: Option<AnnotationConstant>,
    pub argument: Option<AnnotationConstant>,
    pub array_depth: usize,
    pub enum_first_value: Option<AnnotationConstant>,
    pub type_text: String,
    pub element_type_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationConstant {
    pub kind: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub items: Vec<AnnotationConstant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArktsTransformTypeFacts {
    pub file_name: PathBuf,
    pub properties: Vec<ArktsPropertyTypeFacts>,
    pub builder_accesses: Vec<ArktsBuilderReceiverTypeFacts>,
    pub member_accesses: Vec<ArktsExpressionTypeFacts>,
    pub sdk_api_uses: Vec<SdkApiUse>,
}

/// UTF-16 source range shared by native and wire callers. Rust/OXC selects the
/// transform-owned syntax; tsrs resolves only the requested type identities.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TypeQueryRange {
    pub pos: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArktsTransformTypeQuery {
    pub file: PathBuf,
    pub properties: Vec<TypeQueryRange>,
    pub builder_accesses: Vec<TypeQueryRange>,
    pub member_accesses: Vec<TypeQueryRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkApiUse {
    pub api_module: String,
    pub function: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArktsPropertyTypeFacts {
    pub name_pos: usize,
    pub name_end: usize,
    pub r#type: Option<ResolvedTypeIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArktsBuilderReceiverTypeFacts {
    pub pos: usize,
    pub end: usize,
    pub receiver_type: ResolvedTypeIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArktsExpressionTypeFacts {
    pub pos: usize,
    pub end: usize,
    pub r#type: ResolvedTypeIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTypeIdentity {
    pub is_nullable: bool,
    pub is_union: bool,
    pub is_enum: bool,
    pub is_basic: bool,
    pub is_observed_v2: bool,
    pub is_function: bool,
    pub symbol_name: String,
    pub types: Vec<ResolvedTypeIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramSourceGraph {
    pub files: Vec<ProgramSourceGraphFile>,
    pub type_reference_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramSourceGraphFile {
    pub file_name: PathBuf,
    pub dependencies: Vec<PathBuf>,
}
