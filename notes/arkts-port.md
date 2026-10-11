# Dynamic ArkTS and Arkdown service port (2026-10-11)

Base: tsrs `968e5a12`, TypeScript `b85298b6a81f772d080b0455de0ca9d744cd6fd6`.
ArkTS reference: Arkdown's `third_party/typescript` pin,
`ohos-rs/TypeScript@583e9e8b5450c6de49bbe985bd836d7941c7a961`.
OXC dynamic parser reference: local `1cf56806270753cac864b2b6d324280021e5d7a0` with the local ArkTS fixtures.
The reference repositories were read without modifying their tracked sources; temporary Go harness changes used
by the existing tsctests dumper were restored. Static ArkTS is excluded.

## Implementation

The port covers dynamic ArkTS parsing/binding/checking, ArkUI/annotations, the restrictions linter,
Sendable and SDK policies/callbacks, OH TypeScript 4.9 compatibility gates, OHPM/JSON5/SDK resolution,
Kit import expansion and `.d.ets` output. Arkdown uses native snapshot requests with `noEmit`, and keeps
Rust/OXC/Rolldown as its output owner. Static ArkTS is excluded; normal class static blocks are included.

AST/schema/flags/diagnostics remain generated overlays on the unchanged base TypeScript pin. Protocol 10
adds virtual nodes and ArkTS parse metadata, and the owned SDK patches support cloning, visitation, leases,
printing, sync/async snapshot programs and ArkTS queries. The protocol-9 upstream client is intentionally
incompatible. SDK patch regeneration is checked with `npm/sdk/sync-upstream.mjs`.

An incremental regression found that transformed Kit imports reused stale parse-cache entries after config
changes. The project host now fingerprints the actual present/missing Kit JSON inputs before acquiring the
transformed entry. The immutable syntax cache and transformed cache have distinct identities. Regression
coverage edits source, changes/deletes/recreates Kit configs, and checks the original snapshot still has its
old diagnostics. `ensurePrograms` explicitly refreshes dirty API programs, as required by the native API.

This is a correctness port, not a performance change. No performance claims or lint baseline increases.
SDK runtime lowering is outside the checker service, as required by Arkdown's architecture.

## Validation

Measured on this working tree; references are built from their pinned sources, never an npm nightly.
The earlier clean `968e5a12` comparison timed out on two heavy variants on this machine; the final run
completed both. All previous passing cases remain passing.

| Baseline | Initial base run | Final port run | Lost passes |
| --- | ---: | ---: | ---: |
| diagnostics | 13,460 | 13,462 | 0 |
| `.types` | 12,777 | 12,779 | 0 |
| `.symbols` | 12,777 | 12,779 | 0 |
| `.js` | 13,390 | 13,392 | 0 |
| `.js.map` | 149 | 149 | 0 |
| `.sourcemap.txt` | 156 | 156 | 0 |

`tsrs-test run --suite all --baselines types,symbols,js,jsmap,sourcemap --jobs 4 --timeout 90`:
15,197 variants, 1,735 diagnostic skips, zero codes-only mismatches, failures, crashes or timeouts.
The earlier timeouts were `crossProductUnionIntersectionRepeatedPropDeclarationsNoOOM1` and
`intersectionConstructorReductionCrash`; the increased totals are completion, not new semantic coverage.

- OH linter differential: **1,003/1,003** complete diagnostic JSON responses match. The harness reads the
  local OH TypeScript linter corpus and compares code/category/text/positions/chains/order.
- OH compiler differential: **243/243** unique captured programs match syntactic, semantic and linter
  responses. Inputs come from `Test(OH|ArkTS|ArkUI|Ets|Throws|Annotation)` in the pinned Go compiler tests.
  `tools/oracle/arkts/capture.py` restores temporary reference instrumentation in `finally`.
- Arkdown native service tests: **18/18** pass through `ARKDOWN_TSRS_PATH`, including annotations,
  transform type facts, SDK callbacks, source graphs and snapshot updates. The isolated checker manifest
  avoids unrelated workspace dependency resolution; it is not a full application build.
- Owned Node SDK: binary roundtrip/clone/print, virtual nodes, parser identity, leases, sync/async native
  snapshots, annotation metadata (including non-ASCII values) and transform queries pass.
- API codec: pinned Go golden AST content, decoder/re-encoder, printer parity and owned Node client pass.
  Test-only protocol-9 to protocol-10 fixture conversion adds empty fields without changing golden contents.
- API session tests, ArkTS parser/CLI/module/options checks and Kit compiler tests pass.
- OXC dynamic grammar corpus: **5/5 positive and 7/7 negative** fixtures pass through Arkdown's native
  service. This found one missing `scanClassMemberStart` annotation guard: `@interface` in a class member
  position panicked instead of recovering. The guard now follows the OH source; a dedicated parser
  regression asserts all four reference diagnostic codes and ranges. All five parser integration tests pass.
- Fourslash: **4,066/4,546** pass, 63 existing content-mapper/`@tsc` harness failures, 417 skips.
- Pinned tsctests: **tsc 191/223**, **tsbuild 189/192**, zero crashes; unsupported CLI commands remain.
- All 28 CLI regressions pass. The emit oracle's fail-closed regression reports zero failures. The earlier
  frontend run also verified byte-identical OH JS/declarations/maps and annotation declarations.
- Workspace/all-targets and wasm32-wasip1 compile. Lint ratchet and source inventory pass with no baseline
  increase. Eight generators reproduce their outputs; the Go generator uses the installed `go1.27.0`
  explicitly because automatic download of `go1.27` failed in this environment.
- Full options tests caught 42 missing declarations for new OH host inputs. They are now accepted as
  typed tsconfig-only options with semantic/build-info invalidation; all 42 tests pass (one existing skip).
- Final verification logs remain under `target/` and temporary local logs; they are not release artifacts.

The OH reference uses Go 1.26.0; base reference tooling uses Go 1.27.0. Rust is pinned to 1.99.0.
See `docs/ARKTS.md` for reproducible commands and the remaining general tsrs limitations.

## In-process crate integration (2026-10-11)

The later integration replaces Arkdown's executable transport with `tsrs_api::embedded`. The compiler
algorithms and diagnostic ordering are unchanged. Typed snapshots retain their options regions and filesystem
owners; type handles retain the snapshot and reject foreign owners. SDK callback policies are shared with the
wire API. The configuration parser's previously leaked host is now request-region owned. A serial worker keeps
the same large compiler stack supplied by the transport, required when called from small Node/Tokio stacks.
This is a functional embedding and ownership change; no performance or RSS improvement is claimed.

The results above describe the earlier dynamic-language port. The executable-based Arkdown results are
historical; the current integration is covered by the embedded API and linked Arkdown tests. The ordinary
conformance counts were measured again below; the fourslash, tsctests and emit-oracle counts remain from that port's run.

The real SDK project exposed two additional porting/ownership gaps, both fixed here:

- `scanner.GetTextOfNodeFromSourceText` in the pinned OH fork swaps substring bounds when a virtual
  zero-width node's trivia advances beyond its end. That branch was missing in tsrs and caused a panic
  while linting a multiline ArkUI `Text` call. The scanner regression follows the reference branch exactly.
- The API's added ArkTS build/strict pools still allocated strict compiler options in batch arenas.
  Their SDK executor kept N-API's callback alive after checking, preventing Node from exiting. Snapshot
  programs now use registered checker regions, like the existing project pool; batch CLI/recycling
  behavior stays unchanged. The regression fails on the old code using a weak SDK-host reference and
  passes after dropping the program, including with `TSRS_ARENA_POISON=1`. Arkdown's Node regression
  requires natural child-process exit, rather than forcibly ending a test runner with live handles.

Current compiler validation:

- 86 API tests pass, including 7 embedded regressions; the embedded tests also pass with arena poisoning.
- 243/243 OH fixture programs again match complete syntactic, semantic and linter responses.
- 15,197 ordinary compiler/conformance variants: diagnostics 13,462 pass; `.types` and `.symbols`
  12,779 each; `.js` 13,392; `.js.map` 149; `.sourcemap.txt` 156. No failures, crashes or timeouts;
  skip counts remain 1,735 / 2,418 / 2,418 / 1,805 / 15,048 / 15,041 respectively.
- 7 scanner and 5 ArkTS CLI tests pass. The protocol-10 Node SDK regression passes.
