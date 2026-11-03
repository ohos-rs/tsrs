#[expect(dead_code, reason = "This target uses only part of the shared integration helper module.")]
mod common;
use common::*;
use tsrs_core::json;

fn linter_codes(source: &str) -> Vec<i64> {
    let dir = TempDir::new("arkts-linter-alias-enum");
    dir.write("entry.ets", source);
    let session = session(&dir.dir(), false);
    let created = call(
        &session,
        "createSnapshot",
        &format!(
            r#"{{"createPrograms":[{{"rootFiles":[{}],"compilerOptions":{{"noEmit":true,"strict":false,"skipLibCheck":true,"needDoArkTsLinter":true}}}}]}}"#,
            quote(&dir.path("entry.ets"))
        ),
    );
    let params = format!(
        r#"{{"snapshot":{},"project":{}}}"#,
        json::marshal(get(&created, "snapshot")).unwrap(),
        quote(str_of(get(&created, "operation.createdPrograms.0")))
    );
    let diagnostics = call(&session, "getArkTSLinterDiagnostics", &params);
    let build = call(&session, "getArkTSBuildDiagnostics", &params);
    let rendered = json::marshal(&diagnostics).unwrap();
    assert_eq!(json::marshal(get(&build, "linterDiagnostics")).unwrap(), rendered);
    let values: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic["category"].as_i64(), Some(1), "{diagnostic}");
            diagnostic["code"].as_i64().unwrap()
        })
        .collect()
}

#[test]
fn sdk_record_target_alias_accepts_named_chained_and_generic_aliases() {
    // SDK ArkTSLinter_1_1::isStdRecordType checks target.aliasSymbol, rather
    // than the user-facing aliasSymbol. The final any is a linter control.
    let codes = linter_codes(
        r#"
export type Result = Record<string, Object>;
export type Chained = Result;
export type Generic<T> = Record<string, T>;
export const direct: Record<string, Object> = { "code": 0 };
export const named: Result = { "code": 0 };
export const chained: Chained = { "code": 0 };
export const generic: Generic<number> = { "code": 0 };
export const code: number = generic["code"];
export const forbidden: any = 0;
"#,
    );
    assert_eq!(codes, vec![8]);
}

#[test]
fn sdk_record_keys_and_shadowed_mapped_types_remain_restricted() {
    let codes = linter_codes(
        r#"
export type Result = Record<string, Object>;
export const invalidKey: Result = { code: 0 };
export namespace Local {
  export type Record<K extends string, V> = { [P in K]: V };
  export const shadowed: Record<string, Object> = { "code": 0 };
}
"#,
    );
    assert_eq!(codes, vec![38, 83, 38]);
}

#[test]
fn sdk_enum_symbols_allow_reverse_indexing_but_class_fields_do_not() {
    // SDK ArkTSLinter_1_1::isEnumType accepts the enum symbol as well as
    // Enum/EnumLiteral flags. typeof Enum is an object with an enum symbol.
    let codes = linter_codes(
        r"
export enum Status { SUCCESS = 0, ERROR = 1 }
export namespace Nested { export enum Status { SUCCESS = 0 } }
export const name: string = Status[0];
export const nested: string = Nested.Status[0];
export class Fields { code: number = 0; }
export const invalid: number = new Fields()[0];
export const forbidden: any = 0;
",
    );
    assert_eq!(codes, vec![29, 8]);
}
