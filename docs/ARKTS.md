# ArkTS (dynamic)

This branch accepts OpenHarmony's TypeScript-based ArkTS in `.ets` and `.d.ets` files. ArkTS static syntax is
outside this port. Ordinary `.ts`/`.tsx` files retain the pinned TypeScript grammar.

## References

- TypeScript base: `b85298b6a81f772d080b0455de0ca9d744cd6fd6`, unchanged.
- [OHOS TypeScript fork](https://github.com/ohos-rs/TypeScript):
  `583e9e8b5450c6de49bbe985bd836d7941c7a961`, the revision formerly pinned by Arkdown's removed `third_party/typescript` submodule.
  Parser, checker, resolver and emit behavior follow this fork. The reference checkout used for validation is
  `target/reference/typescript-ohos`; its CLI is built from `tsc/cmd/tsc`.
- Local OXC: `1cf56806270753cac864b2b6d324280021e5d7a0` and its ArkTS parser fixtures. Only the dynamic ArkUI
  grammar is used; the separate static ArkTS parser and static language extensions are excluded.

## Supported behavior

- File discovery, imports, declaration output names and language IDs (`arkts` / `ets`).
- `struct`, component bodies, chained attributes, nested builders and the configured UI callback contexts.
- SDK-configured `@Builder`, `@LocalBuilder`, `@Styles`, `@Extend`, `stateStyles`, property decorators and the
  virtual constructor/heritage/type nodes used to check components. Component property bags and attribute arguments
  are type checked. `$r`, `$rawfile` and other dollar-prefixed names still need SDK declarations.
- `import lazy`, preserving its contextual meaning (a default import named `lazy` remains ordinary).
- `@interface` with `etsAnnotationsEnable`: property/default-value rules, constant expressions, use-site arguments,
  runtime/source retention, target restrictions, duplicate annotations, and import/export restrictions.
- `packageManagerType: "ohpm"`: `oh_modules`, JSON5 `oh-package.json5`, package entry points and `.d.ets` resolution.
  Host-provided SDK roots, system-module lists, fallback roots and package export restrictions are accepted.
- `.d.ets` declaration emit, including annotations and their default values.
- The dynamic ArkTS restrictions linter, Sendable checks, strict build diagnostics, `Available`,
  `SuppressWarnings`, SDK API versions, permissions, syscaps, device types and HAR import policies.
- SDK CommonJS checker callbacks (`--sdkPluginCallbacks`) with session-scoped source/config identities.
- The OH fork's TypeScript 4.9 compatibility gates, enabled by `etsLoaderPath`, including inference,
  overload selection, assignability, enums, flow depth and SDK declaration handling.
- Semantic Kit import expansion before binding, including lazy/type/default/aliased imports and component
  attribute supplementation. Snapshot identity fingerprints the actual Kit config inputs; changes,
  deletion and creation invalidate the dependent parse without mutating previous snapshots.
- Native snapshot APIs for build/linter diagnostics, source graphs, annotation information and transform
  type facts. Protocol 10 carries ETS parse options, virtual nodes, component bodies and lazy imports.

## Configuration

Run the native binary with the project's SDK-generated `tsconfig.json`:

```sh
cargo build -p tsrs_cli
target/debug/tsrs -p path/to/project --noEmit
```

The compiler accepts the fork's `ets` object (render methods/decorators, components, Extend/Styles declarations,
custom component base, property decorators and UI callback tables). Supply the table and declarations from the SDK
version used by the project; tsrs does not bundle an OpenHarmony SDK or invent a global component list.
`testdata/arkts/arkui/ets.json` and `sdk.d.ets` are a small reference test environment, not a production SDK.

For an OHPM project, the relevant options include:

```json
{
  "compilerOptions": {
    "noEmit": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "packageManagerType": "ohpm",
    "experimentalDecorators": true,
    "etsAnnotationsEnable": true
  },
  "include": ["src/**/*.ets", "types/**/*.d.ets"]
}
```

Merge the SDK's `ets` table into `compilerOptions`. The host can also provide `etsLoaderPath`, `ohSdkConfigs`
(`[{"apiPaths": [...]}]`), `ohSystemModules`, `ohSdkConfigPrefixes`, `ohFallbackModuleRoots`, `ohLoaderModuleRoot`,
`ohProjectPath`, `ohExternalApiPaths` and `ohPackageExports`. These host paths should be absolute, as in the reference
integration. `ohSystemModules` contains declaration file names such as `@ohos.example.d.ets`. Package export maps
associate a package name with the absolute paths it exposes.

For declarations use `--declaration --emitDeclarationOnly`. JavaScript output for ArkUI or annotation syntax reports
TS100069 and requires the OpenHarmony SDK build transforms; ordinary TypeScript-compatible ETS code can emit JS.

## Arkdown integration and scope

Arkdown starts `tsrs --api --async` and uses `createSnapshot` / `updateSnapshot` directly. Its compiler programs
always use `noEmit`; Rust/OXC/Rolldown own JS, declarations and runtime metadata, consuming tsrs type facts.
There is no legacy `createProgram` compatibility layer. `ARKDOWN_TSRS_PATH` selects a development executable;
release packages embed the binary built from the clean, fixed `third_party/tsrs` gitlink.

Use tsrs's owned Node SDK with this server: protocol 10 replaces protocol 9 for both TS and ETS ASTs. The SDK
preserves virtual nodes, parser metadata, clones and leases; sync and async Program objects expose the ArkTS
queries. The old upstream protocol-9 client is not supported against this server.

Static ArkTS is excluded. Ordinary JavaScript/TypeScript class static members and static blocks remain in scope.
tsrs does not bundle an OpenHarmony SDK or perform ArkUI/runtime JS lowering. This is the same division used by
Arkdown's checker service; direct tsrs JS emission of SDK syntax reports TS100069. CLI watch and the pre-existing
language-server content-mapper/automatic type-acquisition gaps remain listed in the README. Arkdown watch uses
snapshot updates, independently of CLI watch support.

## Tests

```sh
cargo test -p tsrs_parser --test arkts
cargo test -p tsrs_cli --test arkts
node npm/build.mjs --sdk-only
cargo test -p tsrs_module -p tsrs_tsoptions -p tsrs_api_codec -p tsrs_api
node tools/node-api/tests/arkts.mjs
python3 tools/oracle/arkts/run.py --source /path/to/openharmony/third_party/typescript --reference /path/to/tsgo-ohos
python3 tools/oracle/arkts/capture.py --reference-source target/reference/typescript-ohos
python3 tools/oracle/arkts/semantic.py --reference /path/to/tsgo-ohos
TSRS_ARKTS_REFERENCE=/absolute/path/to/tsgo-ohos cargo test -p tsrs_cli --test arkts annotation_reference_programs
```

The CLI fixtures exercise accepted and rejected component calls, annotation declarations/usages, source retention,
declaration output, SDK/JSON5 module resolution and export restrictions. The reference override runs annotation
cases against the pinned OHOS CLI. Its `ets` and SDK host options are API-only, so component/resolver reference cases
run through the fork's `internal/compiler`, `internal/module` and `internal/parser` Go tests. This branch additionally
exposes those host options in tsconfig for standalone CLI use. Validation results and TypeScript regression
comparisons are recorded in `notes/arkts-port.md`.

The linter differential compares 1,003 inputs; compiler fixtures captured from the pinned OH Go tests contribute
243 distinct programs. Diagnostic code, category, positions, text, chains and order must all match. These suites
are evidence for the port, not proof that every possible SDK project has been exercised.
