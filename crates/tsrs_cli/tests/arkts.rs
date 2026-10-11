use std::{path::PathBuf, process::Command};

fn compiler() -> std::ffi::OsString {
    std::env::var_os("TSRS_ARKTS_REFERENCE").unwrap_or_else(|| env!("CARGO_BIN_EXE_tsrs").into())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tsrs-arkts-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn arkui_reference_programs() {
    let dir = scratch("arkui");
    let options = include_str!("../../../testdata/arkts/arkui/ets.json");
    std::fs::write(dir.join("tsconfig.json"), format!(r#"{{"compilerOptions":{{"noEmit":true,"lib":["es2020"],"strict":true,"module":"esnext","moduleResolution":"bundler","allowImportingTsExtensions":true,"experimentalDecorators":true,"ets":{options}}},"include":["**/*"]}}"#)).unwrap();
    std::fs::write(dir.join("sdk.d.ets"), include_str!("../../../testdata/arkts/arkui/sdk.d.ets")).unwrap();
    std::fs::write(dir.join("Card.ets"), include_str!("../../../testdata/arkts/arkui/Card.ets")).unwrap();
    for (name, expression, want_errors) in [
        ("valid", r#"Card({ title: "hello" })"#, false),
        ("wrong property type", r#"Card({ title: 42 })"#, true),
        ("unknown property", r#"Card({ missing: true })"#, true),
        ("nested error", r#"Column() { Text(42) }"#, true),
        ("unknown member", r#"Text(this.missing)"#, true),
        ("styles", r#"Text("hello").common().emphasis(20)"#, false),
        ("styles argument", r#"Text("hello").emphasis("wrong")"#, true),
        ("wrong styles target", r#"Column().emphasis(20)"#, true),
        ("common attributes", r#"Card().width(100)"#, false),
        ("state styles", r#"Text("hi").stateStyles({ normal: { .fontSize(20) } })"#, false),
        (
            "state styles type error",
            r#"Text("hi").stateStyles({ normal: { .fontSize("bad") } })"#,
            true,
        ),
        (
            "dollar identifiers need declarations",
            r#"Card({ title: $message }); Text($$this.message)"#,
            true,
        ),
        ("declared dollar identifier", r#"Text($r("app.string.title"))"#, false),
        ("unknown binding", r#"Text($missing)"#, true),
        ("generic component", r#"Box<string>({ value: "hello" })"#, false),
        ("generic component error", r#"Box<string>({ value: 42 })"#, true),
        ("require is checked by build transform", r#"Box<string>()"#, false),
        ("local storage parameter", r#"Card({ title: "hello" }, new LocalStorage())"#, false),
        ("local storage type", r#"Card({}, 1)"#, true),
        ("empty struct storage", r#"Empty(new LocalStorage())"#, false),
        ("empty struct has no property bag", r#"Empty({}, new LocalStorage())"#, true),
    ] {
        std::fs::write(dir.join("Page.ets"), format!(r#"import {{ Card, Box }} from "./Card"; struct Empty {{ build() {{}} }} @Styles function common() {{ .width(100) }} @Extend(Text) function emphasis(size: number) {{ .fontSize(size) }} @Entry @Component struct Page {{ @State message: string = "hello"; build() {{ {expression} }} }}"#)).unwrap();
        let output = Command::new(compiler())
            .args(["-p", ".", "--pretty", "false"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.code(),
            Some(if want_errors { 2 } else { 0 }),
            "{name}: {text}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if want_errors {
            assert!(text.contains("error TS"), "{name}: {text}");
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn annotation_reference_programs() {
    let dir = scratch("annotations");
    std::fs::write(dir.join("tsconfig.json"), r#"{"compilerOptions":{"etsAnnotationsEnable":true,"noEmit":true,"lib":["es2020"],"strict":false,"module":"esnext","moduleResolution":"bundler"},"include":["**/*"]}"#).unwrap();
    std::fs::write(dir.join("sdk.d.ets"), "declare const enum RetentionPolicy { SOURCE, RUNTIME }").unwrap();
    std::fs::write(dir.join("imported.d.ets"), "export declare @interface Imported {}").unwrap();
    for (source, code) in [
        (r#"export @interface Retention { policy: RetentionPolicy; }"#, 0),
        (r#"@interface A { value: number = 1 } @A class C { @A() method() {} }"#, 0),
        (
            r#"@interface A { value: number; other: string = "ok" } @A({ value: 2 }) class C {}"#,
            0,
        ),
        (r#"@interface A { value: number } @A class C {}"#, 28019),
        (r#"@interface A { value: number } @A(1) class C {}"#, 28020),
        (r#"@interface A { value: number } @A({ value:"bad" }) class C {}"#, 2322),
        (r#"@interface A {} @A @A class C {}"#, 28023),
        (r#"@interface A {} @A abstract class C {}"#, 28021),
        (r#"@interface A {} class C { @A value: number }"#, 28022),
        (
            r#"@interface A { count = 42; text: string = "ok"; flag: boolean = !false; values: number[][] = [[1 + 2]] }"#,
            0,
        ),
        (
            r#"const N = 2; @interface A { value: number = N * 4; policy: RetentionPolicy = RetentionPolicy.RUNTIME }"#,
            0,
        ),
        (r#"@interface A { value: string = "a" + "b"; flag: boolean = (1 < 2) && true }"#, 0),
        (r#"@interface A { value: object }"#, 28033),
        (r#"@interface A { value: number | string }"#, 28033),
        (r#"@interface A { value: [number] }"#, 28033),
        (r#"@interface A { value }"#, 28032),
        (r#"function f(): number { return 1 } @interface A { value: number = f() }"#, 28034),
        (r#"let x = 1; @interface A { value: number = x }"#, 28034),
        (r#"@interface A { value: string = "a" + 1 }"#, 28034),
        (r#"function f() { @interface A {} }"#, 28024),
        (r#"@interface A {} let value: A;"#, 28028),
        (r#"@interface A {} const value = A;"#, 28027),
        (r#"@interface A {} const value = { annotation: A };"#, 28027),
        (
            r#"import { Imported } from './imported'; const value = { annotation: Imported };"#,
            28029,
        ),
        (r#"import { Imported } from './imported'; const value = { Imported };"#, 28029),
        (r#"@A class C {} @interface A {}"#, 28036),
        (r#"@A() class C {} @interface A {}"#, 28036),
        (r#"@A({ value: 1 }) class C {} @interface A { value: number }"#, 28036),
        (r#"@A class C {} declare @interface A {}"#, 0),
        (r#"function f() { @A class C {} } @interface A {}"#, 0),
        (r#"@interface A {} const value = new A();"#, 28027),
        (r#"export default @interface A {}"#, 28031),
    ] {
        std::fs::write(dir.join("input.ets"), source).unwrap();
        let output = Command::new(compiler())
            .args(["-p", ".", "--pretty", "false"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        if code == 0 {
            assert!(
                output.status.success(),
                "{source}: {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            assert!(
                text.contains(&format!("error TS{code}:")),
                "{source}: expected {code}, got {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn source_retention_reference_programs() {
    let dir = scratch("retention");
    std::fs::write(dir.join("tsconfig.json"), r#"{"compilerOptions":{"etsAnnotationsEnable":true,"noEmit":true,"target":"esnext","module":"esnext","moduleResolution":"bundler"},"files":["entry.ets"]}"#).unwrap();
    std::fs::write(dir.join("@arkts.lang.d.ets"), r#"export declare const enum RetentionPolicy { SOURCE = "source", RUNTIME = "runtime" } export declare @interface Retention { policy: string; }"#).unwrap();
    std::fs::write(dir.join("@ohos.annotation.d.ets"), r#"export declare @interface Available { minApiVersion: string; } export declare const enum Rules { API = 'api' } export declare @interface SuppressWarnings { rules: Rules[]; }"#).unwrap();
    for (name, source, code) in [
        ("function", r#"@Source function f() {}"#, 0),
        ("variable", r#"@Source const x = 1;"#, 0),
        (
            "interface",
            r#"@Source interface I { @Source value: number; @Source method(): void; }"#,
            0,
        ),
        ("type", r#"@Source type T = number;"#, 0),
        ("enum", r#"@Source enum E { A }"#, 0),
        ("namespace", r#"@Source namespace N { export const value = 1; }"#, 0),
        (
            "abstract",
            r#"@Source abstract class C { @Source abstract method(): void; @Source value: number = 0; }"#,
            0,
        ),
        (
            "accessors",
            r#"class C { @Source get value() { return 1; } @Source set value(n: number) {} }"#,
            0,
        ),
        ("annotation", r#"@Source @interface Another {}"#, 0),
        ("constructor", r#"class C { @Source constructor() {} }"#, 28043),
        ("index signature", r#"interface I { @Source readonly [key: string]: number; }"#, 0),
        ("parameter", r#"function f(@Source value: number) {}"#, 28043),
        ("duplicate", r#"@Source @Source function f() {}"#, 28023),
        (
            "runtime",
            r#"@Retention({policy: RetentionPolicy.RUNTIME}) @interface Runtime {} @Runtime function f() {}"#,
            28022,
        ),
        (
            "quoted-policy",
            r#"@Retention({'policy': RetentionPolicy.SOURCE}) @interface Quoted {} @Quoted function f() {}"#,
            28022,
        ),
        (
            "enum-identity",
            r#"const enum Custom { SOURCE = 'source' } @Retention({policy: Custom.SOURCE}) @interface CustomPolicy {} @CustomPolicy function f() {}"#,
            0,
        ),
        (
            "sdk-available",
            r#"import { Available } from './@ohos.annotation'; @Available({minApiVersion: '26.0.0'}) function f() {}"#,
            0,
        ),
        (
            "sdk-suppress",
            r#"import { SuppressWarnings, Rules } from './@ohos.annotation'; @SuppressWarnings({rules: [Rules.API]}) interface I {}"#,
            0,
        ),
        (
            "available-homonym",
            r#"@interface Available {minApiVersion: string;} @Available({minApiVersion: '26.0.0'}) function f() {}"#,
            28022,
        ),
    ] {
        std::fs::write(
            dir.join("entry.ets"),
            format!(
                r#"import {{ Retention, RetentionPolicy }} from './@arkts.lang';
@Retention({{policy: RetentionPolicy.SOURCE}}) @interface Source {{}}
{source}"#
            ),
        )
        .unwrap();
        let output = Command::new(compiler())
            .args(["-p", ".", "--pretty", "false"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        if code == 0 {
            assert!(
                output.status.success(),
                "{name}: {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            assert!(
                text.contains(&format!("error TS{code}:")),
                "{name}: expected {code}, got {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn arkts_declarations_and_emit_boundary() {
    let dir = scratch("emit");
    std::fs::write(dir.join("tsconfig.json"), r#"{"compilerOptions":{"etsAnnotationsEnable":true,"declaration":true,"emitDeclarationOnly":true,"target":"esnext","module":"esnext","moduleResolution":"bundler","outDir":"out"},"files":["annotation.ets"]}"#).unwrap();
    std::fs::write(dir.join("@arkts.lang.d.ets"), r#"export declare const enum RetentionPolicy { SOURCE = "source" } export declare @interface Retention { policy: RetentionPolicy; }"#).unwrap();
    std::fs::write(
        dir.join("annotation.ets"),
        r#"import { Retention, RetentionPolicy } from './@arkts.lang';
@Retention({policy: RetentionPolicy.SOURCE}) export @interface Source { value = 42; }
@Source export interface I { @Source value: number; }
@Source export function f(): number { return 1; }
@Source export class C { @Source method() {} }"#,
    )
    .unwrap();
    let output = Command::new(compiler())
        .args(["-p", ".", "--pretty", "false"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let declaration = std::fs::read_to_string(dir.join("out/annotation.d.ets")).unwrap();
    for expected in [
        "import { Retention, RetentionPolicy }",
        "@Retention({ policy: RetentionPolicy.SOURCE })",
        "@interface Source",
        "value: number = 42",
    ] {
        assert!(declaration.contains(expected), "missing {expected}: {declaration}");
    }
    assert_eq!(declaration.matches("@Source").count(), 5, "{declaration}");
    let output = Command::new(compiler())
        .args(["-p", ".", "--emitDeclarationOnly", "false", "--pretty", "false"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("TS100069"));
    assert!(!dir.join("out/annotation.js").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ohpm_project_resolution_and_exports() {
    let dir = scratch("ohpm");
    let package = dir.join("oh_modules/pkg");
    let sdk = dir.join("sdk");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::create_dir_all(&sdk).unwrap();
    std::fs::write(
        package.join("oh-package.json5"),
        "{name: 'pkg', version: '1.0.0', types: './index.d.ets',}",
    )
    .unwrap();
    std::fs::write(package.join("index.d.ets"), "export declare const value: string;").unwrap();
    std::fs::write(sdk.join("@ohos.demo.d.ets"), "export declare const value: number;").unwrap();
    std::fs::write(
        dir.join("entry.ets"),
        "import { value } from 'pkg'; import { value as sdk } from '@ohos.demo'; const text: string = value; const number: number = sdk;",
    )
    .unwrap();
    for (exports, expected) in [("", None), (",\"ohPackageExports\":{\"pkg\":[]}", Some(28045))] {
        std::fs::write(dir.join("tsconfig.json"), format!(r#"{{"compilerOptions":{{"noEmit":true,"module":"esnext","moduleResolution":"bundler","packageManagerType":"ohpm","ohSdkConfigs":[{{"apiPaths":["{}"]}}],"ohSystemModules":["@ohos.demo.d.ets"]{exports}}},"files":["entry.ets"]}}"#, sdk.display())).unwrap();
        let output = Command::new(compiler())
            .args(["-p", ".", "--pretty", "false"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        match expected {
            None => assert!(output.status.success(), "{text}\n{}", String::from_utf8_lossy(&output.stderr)),
            Some(code) => assert!(
                text.contains(&format!("TS{code}")),
                "{text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
