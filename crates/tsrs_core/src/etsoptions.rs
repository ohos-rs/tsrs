use serde::{Deserialize, Serialize};

// OH types.ts::EtsOptions. None and an explicitly empty table have different
// parser behavior. Preserve table order for Extend and attribute callbacks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EtsOptions {
    pub render: EtsRenderOptions,
    pub components: Option<Vec<String>>,
    pub libs: Option<Vec<String>>,
    pub extend: EtsExtendOptions,
    pub styles: EtsStylesOptions,
    pub concurrent: EtsConcurrentOptions,
    pub custom_component: Option<String>,
    pub property_decorators: Option<Vec<EtsPropertyDecorator>>,
    pub emit_decorators: Option<Vec<EtsEmitDecorator>>,
    pub syntax_components: EtsSyntaxComponents,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsRenderOptions {
    pub method: Option<Vec<String>>,
    pub decorator: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsComponentDeclaration {
    pub name: String,
    pub r#type: String,
    pub instance: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsExtendOptions {
    pub decorator: Option<Vec<String>>,
    pub components: Option<Vec<EtsComponentDeclaration>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsStylesOptions {
    pub decorator: Option<String>,
    pub component: Option<EtsComponentDeclaration>,
    pub property: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsConcurrentOptions {
    pub decorator: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EtsPropertyDecorator {
    pub name: String,
    pub need_initialization: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EtsEmitDecorator {
    pub name: String,
    pub emit_parameters: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsSyntaxComponents {
    #[serde(rename = "paramsUICallback")]
    pub params_ui_callback: Option<Vec<String>>,
    #[serde(rename = "attrUICallback")]
    pub attr_ui_callback: Option<Vec<EtsAttributeCallback>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct EtsAttributeCallback {
    pub name: String,
    pub attributes: Option<Vec<String>>,
}

pub fn ets_contains(values: &Option<Vec<String>>, value: &str) -> bool {
    values.as_ref().is_some_and(|values| values.iter().any(|item| item == value))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OhSdkConfig {
    pub api_paths: Vec<String>,
}
