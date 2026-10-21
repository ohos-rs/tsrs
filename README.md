# `tsrs`

[![npm](https://img.shields.io/npm/v/tsrs)](https://www.npmjs.com/package/tsrs)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue)](https://github.com/oxc-project/tsrs/blob/main/LICENSE)

`tsrs` is a drop-in replacement for the TypeScript 7 compiler and language server, written in Rust.
Our goal is 100% conformance, with much lower memory use and faster type checking. [Read the announcement](https://blog.cloudflare.com/typescript-rust-port/).

This branch also supports the dynamic ArkTS (`.ets` / `.d.ets`), ArkUI checking, the restrictions linter, SDK policies,
Kit imports, OHPM resolution and native snapshot APIs. Build it from source and use your project's SDK declarations and `ets` options; see
[ArkTS configuration and boundaries](docs/ARKTS.md). ArkTS static syntax is excluded.
Rust hosts can embed `tsrs_api::embedded` directly; Arkdown links these crates into its N-API binding.

## Get started

```sh
pnpm add -D tsrs
pnpm exec tsrs --noEmit
```

You can try `tsrs` today and give us feedback on performance, memory usage, and conformance.
Try checking and emitting code in the [browser playground](https://tsrs-playground.void.app).

## Conformance

`tsrs` currently targets the main branch of [microsoft/TypeScript](https://github.com/microsoft/typescript).
We aim to release a stable version against TypeScript 7.1 with 100% conformance. In our testing on existing projects,
we have yet to find a correctness bug in `tsrs`.

There are known order issues: the pinned Go compiler can produce different diagnostics and traces depending on
parallel parse, resolution, and check order, while `tsrs` canonicalizes some checker history by default. For the
closest comparison, use `--checkerAssignment go --noLazyMembers`; tsgo parser and resolver races can still vary
between runs.

`--init` and `--showConfig` are not supported. See [compatibility and evidence](https://github.com/oxc-project/tsrs/blob/main/docs/COMPATIBILITY.md)
for supported features and known gaps.

<a href="https://github.com/oxc-project/tsrs/blob/main/bench/RESULTS.md">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/oxc-project/tsrs/main/bench/large-projects-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/oxc-project/tsrs/main/bench/large-projects.svg">
    <img alt="tsrs vs the TypeScript nightly: times faster against times less peak memory on seven projects at 1, 2, 4 and 8 parallel checker threads" src="https://raw.githubusercontent.com/oxc-project/tsrs/main/bench/large-projects.svg">
  </picture>
</a>

## ArkTS / OpenHarmony support

This branch carries the dynamic ArkTS and OpenHarmony port on top of the upstream compiler. Capabilities, evidence
and known limits:

| feature | status | evidence and notes |
| --- | --- | --- |
| Dynamic ArkTS frontend and component checking | SDK `ets` options | `struct`, UI bodies, builders/styles, lazy imports and annotation checks; 12/12 OXC positive/negative grammar fixtures, virtual-node text regression and validation in [notes/arkts-port.md](notes/arkts-port.md) |
| OHPM and SDK module resolution | `packageManagerType: "ohpm"` / SDK host options | `.ets` / `.d.ets`, `oh_modules`, JSON5 manifests, SDK/fallback roots and package export restrictions ([configuration](docs/ARKTS.md)) |
| ArkTS declarations | `--emitDeclarationOnly` | `.d.ets`, annotation defaults and supported retained decorators; reference declaration test |
| Dynamic ArkTS restrictions linter / Sendable | `needDoArkTsLinter` | 1,003/1,003 complete diagnostic responses match the pinned OH compiler |
| OH TypeScript 4.9 compatibility, Kit imports and SDK policy checks | SDK host options | Historical 243/243 OH fixture comparison; numeric-enum equality, legacy RegExp and concurrent SDK-registration regressions verified on 2026-10-12 ([validation](notes/arkts-port.md#real-build-regressions-2026-10-12)) |
| In-process Rust compiler API | yes | Typed snapshots, diagnostics, annotation/type facts, SDK hosts and RAII leases; 7 embedding regressions, including SDK-host release and small-stack calls, in `crates/tsrs_api/tests/embedded_test.rs`; [contract](docs/NODE_API.md#in-process-rust-embedding) |
| ArkTS binary AST and Node SDK | yes | protocol 10; virtual nodes, parse identity, lazy imports, UI bodies, sync/async snapshots and annotation/type facts ([API](docs/NODE_API.md)) |
| ArkUI/annotation JavaScript lowering | no | TS100069 in direct tsrs emit; Arkdown's Rust/OXC/Rolldown pipeline owns lowering |
| ArkTS static syntax | no | deliberately excluded from this dynamic-language port |

The current ArkTS branch also matches all 13,462 TypeScript error baselines and 12,779 `.types` / `.symbols`
baselines without failures or timeouts; the ArkTS port loses no previously passing case
([validation](notes/arkts-port.md)).

## Extra features

[`@tsrs/wasm`](https://github.com/oxc-project/tsrs/blob/main/npm/tsrs-wasm/README.md) provides WebAssembly support
targeting `wasm32-wasip1` (WASI Preview 1).

## Learn more

- [CLI usage and memory tuning](https://github.com/oxc-project/tsrs/blob/main/docs/USAGE.md)
- [Editor setup](https://github.com/oxc-project/tsrs/blob/main/docs/LSP.md#editor-setup)
- [Node API (unstable)](https://github.com/oxc-project/tsrs/blob/main/docs/NODE_API.md)
- [WebAssembly for Node and browsers](https://github.com/oxc-project/tsrs/blob/main/npm/tsrs-wasm/README.md)
- [Benchmarks and methodology](https://github.com/oxc-project/tsrs/blob/main/bench/README.md)
- [Release notes](https://github.com/oxc-project/tsrs/blob/main/docs/STATUS.md)
- [Engineering notes](https://github.com/oxc-project/tsrs/blob/main/notes/README.md)

## Contributing

The [issue tracker](https://github.com/oxc-project/tsrs/issues) is open for bug reports. Please include a minimal reproduction.
Pull requests remain closed because we don't have the bandwidth to review them.

See the [contributing guide](https://github.com/oxc-project/tsrs/blob/main/CONTRIBUTING.md)
for building from source and running the suites, the [Code of Conduct](https://github.com/oxc-project/tsrs/blob/main/CODE_OF_CONDUCT.md), and
[security policy](https://github.com/oxc-project/tsrs/blob/main/SECURITY.md) for reporting vulnerabilities.

## Acknowledgments

We thank the following organizations and contributors:

<p>
  <a href="https://depot.dev">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/oxc-project/tsrs/main/assets/sponsors/depot-dark.svg">
      <img alt="Depot" src="https://raw.githubusercontent.com/oxc-project/tsrs/main/assets/sponsors/depot-light.svg" height="32">
    </picture>
  </a>
  &nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://www.owner.com">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/oxc-project/tsrs/main/assets/sponsors/owner-dark.svg">
      <img alt="Owner.com" src="https://raw.githubusercontent.com/oxc-project/tsrs/main/assets/sponsors/owner-light.svg" height="32">
    </picture>
  </a>
</p>

- [Depot CI](https://depot.dev) for sponsoring our continuous integration.
- [Owner.com](https://www.owner.com) for sponsoring the initial work.
- [Max Schwenk](https://github.com/maschwenk) for the port.
- [@walkerdb](https://github.com/walkerdb) for improvements to the project.
- [@JiangWeixian](https://github.com/JiangWeixian) for transferring the `tsrs` npm package name.

We extend special thanks to the [TypeScript team](https://www.typescriptlang.org/) for their support.

## License

`tsrs` is a derivative of TypeScript (Copyright Microsoft Corporation), licensed under [Apache-2.0](https://github.com/oxc-project/tsrs/blob/main/LICENSE).
The bundled `lib.*.d.ts` files are TypeScript's. See [NOTICE](https://github.com/oxc-project/tsrs/blob/main/NOTICE) for attribution.
Not affiliated with or endorsed by Microsoft.
