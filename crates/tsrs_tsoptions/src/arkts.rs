use crate::commandlineoption::{CommandLineOption, CommandLineOptionKind, CompilerOptionsValue};
use crate::gojson::GoJson;
use crate::parsinghelpers::{IsZero, ToOptionsValue};
use std::sync::Arc;
use tsrs_core::{EtsOptions, json};

pub(crate) static OPTIONS: &[CommandLineOption] = &[
    CommandLineOption {
        name: "ets",
        kind: CommandLineOptionKind::Object,
        is_tsconfig_only: true,
        affects_source_file: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_emit: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "etsAnnotationsEnable",
        kind: CommandLineOptionKind::Boolean,
        affects_source_file: true,
        affects_semantic_diagnostics: true,
        affects_emit: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "etsLoaderPath",
        kind: CommandLineOptionKind::String,
        is_file_path: true,
        affects_source_file: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "packageManagerType",
        kind: CommandLineOptionKind::String,
        affects_source_file: true,
        affects_module_resolution: true,
        affects_emit: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohSdkConfigs",
        kind: CommandLineOptionKind::List,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohSystemModules",
        kind: CommandLineOptionKind::List,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohSdkConfigPrefixes",
        kind: CommandLineOptionKind::List,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohFallbackModuleRoots",
        kind: CommandLineOptionKind::List,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohLoaderModuleRoot",
        kind: CommandLineOptionKind::String,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohProjectPath",
        kind: CommandLineOptionKind::String,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohExternalApiPaths",
        kind: CommandLineOptionKind::List,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    CommandLineOption {
        name: "ohPackageExports",
        kind: CommandLineOptionKind::Object,
        is_tsconfig_only: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    host_option("isCompileJsHar", CommandLineOptionKind::Boolean),
    host_option("moduleRootPath", CommandLineOptionKind::String),
    host_option("maxFlowDepth", CommandLineOptionKind::Number),
    host_option("compileSdkVersion", CommandLineOptionKind::Number),
    CommandLineOption {
        name: "noTransformedKitInParser",
        kind: CommandLineOptionKind::Boolean,
        is_tsconfig_only: true,
        affects_source_file: true,
        affects_module_resolution: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    },
    host_option("tsImportSoCheck", CommandLineOptionKind::Boolean),
    host_option("needDoArkTsLinter", CommandLineOptionKind::Boolean),
    host_option("isCompatibleVersion", CommandLineOptionKind::Boolean),
    host_option("tsImportSendableEnable", CommandLineOptionKind::Boolean),
    host_option("emitNodeModulesFiles", CommandLineOptionKind::Boolean),
    host_option("skipTscOhModuleCheck", CommandLineOptionKind::Boolean),
    host_option("skipArkTSStaticBlocksCheck", CommandLineOptionKind::Boolean),
    host_option("skipPathsInKeyForCompilationSettings", CommandLineOptionKind::Boolean),
    host_option("skipBaseUrlInKeyForCompilationSettings", CommandLineOptionKind::Boolean),
    host_option("compatibleSdkVersion", CommandLineOptionKind::Number),
    host_option("compatibleSdkVersionStage", CommandLineOptionKind::String),
    host_option("skipOhModulesLint", CommandLineOptionKind::Boolean),
    host_option("enableStrictCheckOHModule", CommandLineOptionKind::Boolean),
    host_option("disableStrictCheckPaths", CommandLineOptionKind::List),
    host_option("disableSendableCheckRules", CommandLineOptionKind::List),
    host_option("mixCompile", CommandLineOptionKind::Boolean),
    host_option("strictCheckerOnly", CommandLineOptionKind::Boolean),
    host_option("ohRuntimeOS", CommandLineOptionKind::String),
    host_option("ohOriginCompatibleSdkVersion", CommandLineOptionKind::String),
    host_option("ohProjectRootPath", CommandLineOptionKind::String),
    host_option("ohModulePath", CommandLineOptionKind::String),
    host_option("ohAllModulePaths", CommandLineOptionKind::List),
    host_option("ohGlobalModulePaths", CommandLineOptionKind::List),
    host_option("ohArkUIDeclarationDirs", CommandLineOptionKind::List),
    host_option("ohRequestPermissions", CommandLineOptionKind::List),
    host_option("ohSyscapIntersection", CommandLineOptionKind::List),
    host_option("ohSyscapUnion", CommandLineOptionKind::List),
    host_option("ohDeviceTypes", CommandLineOptionKind::List),
    host_option("ohCardEntryFiles", CommandLineOptionKind::List),
    host_option("ohCrossplatform", CommandLineOptionKind::Boolean),
    host_option("ohIgnoreCrossplatformCheck", CommandLineOptionKind::Boolean),
    host_option("ohCompileMode", CommandLineOptionKind::String),
    host_option("ohBundleType", CommandLineOptionKind::String),
    host_option("ohApiCompatibilityCheck", CommandLineOptionKind::String),
    host_option("ohSdkCheckPlugins", CommandLineOptionKind::List),
    host_option("ohSdkClassCheckPlugins", CommandLineOptionKind::List),
    host_option("ohSdkPluginProjectConfig", CommandLineOptionKind::Object),
];

// OH's host-only fields are also accepted in tsconfig by this standalone compiler.
// They remain typed inputs and participate in semantic/build-info invalidation.
const fn host_option(name: &'static str, kind: CommandLineOptionKind) -> CommandLineOption {
    CommandLineOption {
        name, kind, is_tsconfig_only: true,
        affects_semantic_diagnostics: true,
        affects_build_info: true,
        ..CommandLineOption::DEFAULT
    }
}

pub(crate) fn parse_ets(value: &CompilerOptionsValue) -> Result<Arc<EtsOptions>, serde_json::Error> {
    serde_json::from_str(&crate::tsconfigparsing::stringify_json(value))
}

impl IsZero for Arc<EtsOptions> {
    fn is_zero(&self) -> bool {
        **self == EtsOptions::default()
    }
}

impl GoJson for Arc<EtsOptions> {
    fn to_go_json(&self) -> Option<json::Value> {
        if self.is_zero() {
            return None;
        }
        Some(json::unmarshal(&serde_json::to_string(self).unwrap()).unwrap())
    }
    fn from_go_json(value: &json::Value, _field: &str) -> Result<Self, String> {
        if matches!(value, json::Value::Null) {
            return Ok(Self::default());
        }
        serde_json::from_str(&json::marshal(value)?).map_err(|error| error.to_string())
    }
}

impl ToOptionsValue for Arc<EtsOptions> {
    fn to_options_value(&self) -> CompilerOptionsValue {
        if self.is_zero() {
            CompilerOptionsValue::Null
        } else {
            serde_options_value(serde_json::to_value(self).unwrap())
        }
    }
}

impl GoJson for Option<Vec<tsrs_core::OhSdkConfig>> {
    fn to_go_json(&self) -> Option<json::Value> {
        self.as_ref().map(|v| json::unmarshal(&serde_json::to_string(v).unwrap()).unwrap())
    }
    fn from_go_json(value: &json::Value, _field: &str) -> Result<Self, String> {
        serde_json::from_str(&json::marshal(value)?).map_err(|e| e.to_string())
    }
}
impl ToOptionsValue for Option<Vec<tsrs_core::OhSdkConfig>> {
    fn to_options_value(&self) -> CompilerOptionsValue {
        self.as_ref().map_or(CompilerOptionsValue::Null, |configs| {
            CompilerOptionsValue::Array(
                configs
                    .iter()
                    .map(|c| {
                        CompilerOptionsValue::Object(
                            [("apiPaths".to_string(), CompilerOptionsValue::StringArray(c.api_paths.clone()))]
                                .into_iter()
                                .collect(),
                        )
                    })
                    .collect(),
            )
        })
    }
}

fn serde_options_value(value: serde_json::Value) -> CompilerOptionsValue {
    match value {
        serde_json::Value::Null => CompilerOptionsValue::Null,
        serde_json::Value::Bool(value) => CompilerOptionsValue::Bool(value),
        serde_json::Value::Number(value) => CompilerOptionsValue::Float(value.as_f64().unwrap()),
        serde_json::Value::String(value) => CompilerOptionsValue::String(value),
        serde_json::Value::Array(values) => CompilerOptionsValue::Array(values.into_iter().map(serde_options_value).collect()),
        serde_json::Value::Object(values) => {
            CompilerOptionsValue::Object(values.into_iter().map(|(key, value)| (key, serde_options_value(value))).collect())
        }
    }
}

// Host-only options use the same typed JSON representation in API responses,
// compiler option identity and the build-info serializer.
macro_rules! serde_host_option {
    ($($ty:ty),* $(,)?) => {$(
        impl GoJson for $ty {
            fn to_go_json(&self) -> Option<json::Value> {
                if self.is_zero() { return None; }
                Some(json::unmarshal(&serde_json::to_string(self).unwrap()).unwrap())
            }
            fn from_go_json(value: &json::Value, _field: &str) -> Result<Self, String> {
                if matches!(value, json::Value::Null) { return Ok(Self::default()); }
                serde_json::from_str(&json::marshal(value)?).map_err(|error| error.to_string())
            }
        }
        impl ToOptionsValue for $ty {
            fn to_options_value(&self) -> CompilerOptionsValue {
                if self.is_zero() { CompilerOptionsValue::Null }
                else { serde_options_value(serde_json::to_value(self).unwrap()) }
            }
        }
    )*};
}
impl IsZero for f64 {
    fn is_zero(&self) -> bool { *self == 0.0 }
}
serde_host_option!(
    f64, Option<f64>,
    Option<Vec<tsrs_core::OhSdkCheckPlugin>>,
    Option<Vec<tsrs_core::OhSdkClassCheckPlugin>>,
    Option<serde_json::Map<String, serde_json::Value>>,
);
