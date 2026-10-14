# SDK plugin source transport (Round 1 / Round 1b / Round 1c / R3)

Scope: arkdown's [PERF-CHECKER-SDK-PLUGIN-MEMORY.md](../../arkdown/meta/tasks/PERF-CHECKER-SDK-PLUGIN-MEMORY.md),
Round 1 A–D. This is the development fork based on `ohos` fb3668e; no acceptance measurements or landing decision
are claimed here. The owner requested implementation and correctness gates now, with measurements performed separately.

## Evidence and ownership

The task records an arkdown baseline of 464,674 SDK calls / 2.27 GB of JSON, including 1.95 GB of declaration
node text, and a 5.02 GB process peak footprint. These are the task author's earlier measurements, not a before/after
measurement of this change. `checker::oh_sdk_plugin` copied both complete files for each callback before the API
intern table compared and retained those copies.

`SourceFile::text()` already supplies immutable arena-backed text. A snapshot now borrows that text only during the
synchronous callback and carries the parsed SourceFile's globally unique node ID. The API intern table keys by
(file name, file ID), retaining only identities and wire IDs, never text or arena pointers. A changed file parsed by
`update_program` has a new identity, even if released arena storage is later reused. Multiple live versions of a
path keep distinct IDs. The existing executor mutex still covers registration through synchronous host dispatch.

The wire node is `{fileName, sourceId, pos, end, start}`, with `source` only on first registration of a file version.
Positions are UTF-16. `start` uses the same scanner `skip_trivia` as the old `get_text_of_node` path for SDK use and
declaration nodes. Function plugin arguments and class `check(node, declaration, projectConfig)` are unchanged.
The ets-loader class ABI and SDK `SyscapChecker.js::check` require real TypeScript nodes. Arkdown still reconstructs
those nodes with the ets-loader TypeScript; its session-wide 64-entry LRU drops application ASTs, node lookup maps,
and child caches together. `.d.ts` / `.d.ets` ASTs remain resident. Source strings remain available for reparsing.
Arkdown initializes the existing mimalloc allocator's purge delay to 0; that is a provisional acceptance candidate.

Earlier work reviewed: `mem-zero-copy-libs.md` (reuse immutable text), `mem-round4.md`, `perf-round2-followups.md`
and `docs/RUST.md` Techniques. This does not retry shared checker graphs, checker retirement, or front-end lazy
parsing. The changed evidence is the SDK callback boundary's repeated full-file and declaration-text copies;
ordinary tsrs benchmarks without SDK callbacks do not exercise that cost.

## Measurements — pending acceptance

| Metric | Before | After |
| --- | --- | --- |
| Instructions retired, one checker / default count | Pending | Pending |
| Peak RSS, one checker / default count | Pending | Pending |
| Arkdown peak footprint / V8 heapUsed | Task baseline only | Pending |
| SDK request JSON bytes | Task baseline only | Pending |
| Wall time, same hardware repeated runs | Pending | Pending |

Acceptance must apply the repository's performance threshold before landing, and compare diagnostics and generated
artifact hashes as described in the arkdown task. No build against the acceptance application was run in this round.

## Regression coverage

`tsrs_api/tests/sdk_plugin_test.rs` retains the concurrent publication-order test and asserts source registration
happens only once. An embedded program test compares **every** outgoing node/declaration's UTF-16 source slice,
byte-for-byte, to the previous scanner text algorithm on a parsed source tree. It includes BOM, CRLF, comments,
Chinese identifiers and astral Unicode, repeated uses and function/class/member declarations. It updates both files
at the same paths through `update_program`, checks new source IDs and resent contents in the same plugin session,
and rejects any outgoing `text` field. Arkdown tests cover recency, eviction/reparse, declaration retention,
version separation, parent links and unmatched-node errors.

## Round 1 acceptance review and Round 1b (2026-10-12)

The reviewer supplied Round 1 measurements in the arkdown task: peak footprint 5.02 -> 4.10 GB, V8 heapUsed
1.2 GB -> 582 MB (265 MB retained after GC), SDK request JSON 2.27 GB -> 285 MB, wall time 157 -> 133 s.
The reviewer verified identical 10,231 diagnostics and artifact hashes. These are reviewer measurements,
not measurements made during this implementation turn. Round 1b instructions retired, peak RSS, footprint,
request JSON and wall-time measurements remain **pending acceptance**.

The updated task explicitly authorizes a deviation from ets-loader's per-use invocation: deterministic SDK
function results are cached by (path, export, operation, serialized arguments), and class results by exact
(node file version/pos/end/start, declaration file version/pos/end/start, config index), separately per plugin.
The cache belongs to the executor. No declaration-only deduplication, checker-count changes, or lazy DTS
changes are included. This differs from the earlier decision recorded above because the owner accepted
result caching after the reviewer measured only 24 distinct value inputs in 232,530 calls.

Function cache entries use OnceLock to coalesce concurrent identical calls. Map mutexes are released before
host calls. Errors and transport failures remove their entries; fallback/default responses produced by error
policy are not cached. Syscap keeps the existing ordered source/config publication lock, retries unsuccessful
publication with full source/config, and caches only successful raw responses. Its keys are grouped per plugin
so 200k result entries do not each duplicate a descriptor path/export. The first invocation registers its
(path, functionName, className) with a pluginId; subsequent requests carry only that ID. OnceLock ensures ID
references follow publication; a transport failure removes that descriptor registration for retry.

The scanner regression now includes missing nodes, reversed virtual substring bounds, JSDoc type
expressions and children, plus a JSDoc-flagged non-type node that still uses a slice. Reparsed literals also
retain legacy text because the scanner can quote or rename them. Ordinary snapshots omit text; exceptional
snapshots carry exact scanner text, and the JS host compares getText for them, including cached-node hits.
Tests protect operation/argument typing in keys, concurrent cache hits, independent function calls during a
blocked host invocation, version/config/span/plugin misses, descriptor publication and error retries.

Arkdown's overlay preprocessing is injected through a checker-owned trait. Each immutable source version
has a OnceLock; changed raw input and observed disk invalidation create a new version. OXC semantic
inventories and UI/Extend names are selected from the Program's read source graph, and facts use the exact
processed text from those overlays. The architecture guard is restored exactly, and the temporary local
Cargo patch still causes its original lockfile pin assertion to fail as expected.

## Round 1c: config identity, publication gates, and file facts (2026-10-12)

Scope and profile: arkdown task `R1b 验收` / `R1c 范围`. The reviewer measured R1b at
3.90 / 4.05 GB footprint, 172.14 / 174.02 s wall and 85.489 / 83.627 s checker time.
The reviewer's four-checker-thread profile found 34% of time waiting on the global syscap mutex,
versus about 3% waiting for JS. Of busy CPU, 95.5% was OH SDK use checking, including 61% serde_json
and 30% tspath; actual type checking was under 1%. These are reviewer-supplied measurements.
This is new evidence for removing repeated plugin-boundary work, not a retry of shared type graphs,
checker retirement, or lazy DTS ideas rejected in the earlier notes listed above.

Compiler options now carry `Arc<Map>` for an explicit plugin config. The derived config uses a
OnceLock shared by the immutable options snapshot; cloning mutable options clears that derived
cache so incremental reconfiguration cannot reuse stale fields. Arkdown wraps the complete config
once when producing the program options. The executor first looks up the Arc's address through a
read lock; only a new Arc compares content. Every encountered Arc stays retained until executor drop,
including content-equal Arcs, preventing address reuse from aliasing a different config.

Syscap and function result maps use FxHash and read locks on hits, with a per-key OnceLock to
coalesce concurrent misses. Failed raw responses and transport failures are removed and retried;
error-policy defaults are still never cached. Function keys recursively retain JSON value types,
object iteration order and floating-point bit patterns, without JSON serialization. Plugin IDs
avoid repeating descriptor strings in result entries. `prepareClass` / `releaseSession` remain uncached.

The short source-registration mutex reserves IDs and publication gates only. Serialization, the host
call, JS reply, and waits for initial publication all happen outside it. A request reserves all its
unpublished resources together only after checking for pending publications, avoiding cycles between
overlapping node/declaration/config sets. Successful completion publishes those resources; error or
unwind releases waiters with an unsuccessful publication so the next call resends full data. Descriptor
publication retains its own completion gate. Thus ID-only requests follow an acknowledged publication
without depending on any N-API threadsafe-function FIFO behavior. Already published resources and
cached results continue through a blocked independent callback. Source IDs use immutable SourceFile
node identity; source text remains borrowed only for the synchronous call and is never retained by Rust.

The checker's OH SDK state now caches file classification by SourceFile node ID: normalized names,
project/SDK/system classification, Available/deviceInfo text scans, card/ohosTest membership, ArkUI
directories, API module names and SDK whitelist membership. It keeps the original path predicates,
including direct-child exclusions, exactly. Separate live versions at one path have separate facts.
No checker-count, diagnostics, plugin ABI, LRU policy or front-end behavior changes are included.

Regression additions cover a blocked syscap host with concurrent cache hits and independent misses;
Arc identity/content reuse and copy-on-write config changes; derived config sharing/reconfiguration;
recursive argument keys (including integer/float and signed zero); and SourceFile version identity plus
classification boundaries. The prior source-slice, incremental source resend, first-publication ordering,
plugin descriptor and failure-retry tests remain in place.

The owner explicitly delegated post-change `sample` profiling to the reviewer because this sandbox
cannot inspect other processes. No post-change CPU percentages, retired instruction counts, acceptance
footprint, or claim that type checking is now the largest CPU consumer are available. Those measurements
and the repository's performance landing threshold remain pending acceptance. No commit was made.

### Sandbox-relative cold run (not acceptance measurements)

The release binding was rebuilt before the meituan acceptance cold run. No concurrent acceptance
build and no `sample` invocation were used in this continuation. The corrected harness exports
`LC_ALL=C`. Its diagnostics and artifact gates passed: 10,231 identical diagnostic lines and the
baseline `modules.abc` / `sourceMaps.map` hashes.

| Metric | Earlier sandbox baseline (`r1c-before`) | R1c (`r1c`) |
| --- | --- | --- |
| Wall time | 236.07 s | 147.53 s |
| Checker diagnostic phase | 130.522 s | 9.516 s |
| Peak RSS | Unavailable with the old harness | 1,957,280 KB (`ARKDOWN_MAXRSS_KB`, harness fallback) |
| Peak footprint | Unavailable | Unavailable |
| Retired instructions / checker CPU category percentages | Unavailable | Reviewer profiling pending |

These single cold runs show 37.5% lower wall time and 92.7% lower checker phase time **in this sandbox**,
not an acceptance-hardware claim. Other stages varied substantially (packaging 58.763 -> 90.800 s).
The old baseline's `diag=DIFF` was collation-only: sorting its saved diagnostics under `LC_ALL=C`
now compares identical to the baseline. The latest run requires no correction. `time -l` cannot read
kernel clock statistics in the sandbox, so the harness reports the Node maxRSS fallback, which does not
measure compressed-memory footprint and cannot be compared with the reviewer's 3.90 / 4.05 GB figures.

Exact latest harness line (literal `\t` separators emitted by the harness):

```text
r1c\tcold\trun1\tok=1\tpeak=rss1.96GB\twall=147.53s\tstartup=11824ms\tprofiles=3265\tbundling=33777\tchecker=9516\tabc=7628\tpackage=90800\tdiag=same\tartifacts=same
```

Verification: `cargo test -p tsrs_api` (98 passed), `cargo test -p tsrs_checker` (10 passed),
`python3 tools/lint/ratchet.py` (3 findings, none new), and `python3 tools/lint/source.py` passed.
Arkdown checker/build tests: 136 passed, 2 existing ignored; workspace clippy, fmt and release binding
build passed. Node tests: 29 passed, with only the expected original lockfile-pin assertion failing
under the temporary local Cargo patch. The guard remains byte-identical to its pre-Round-1 backup;
`plugin-adapter.js` remains 44,893 bytes. Post-change acceptance profiling is still required.

## R3: per-file syscap batches and node locality (2026-10-13)

Scope: arkdown task `R3 范围（syscap 按文件批量 + JS 节点定位）`. R1c was accepted by the owner
on a clean disk: wall 68.19 / 67.59 s, checker 10.069 / 10.655 s, footprint 4.32 / 4.38 GB.
The clean pre-Codex baseline was wall 134.8 s, checker 78.9 s, footprint 5.01 GB. The owner
confirmed identical 10,231 diagnostics, ABC/source-map hashes, and R1c HAP entry lists. The
previous 138–145 s packaging observations and the earlier sandbox packaging comparison above
were invalidated by a full disk and near-full swap. They are not R1c acceptance results.

The new evidence differs from the earlier R1c mutex/serde profile: checker threads now spend
about two thirds of their time waiting for JS; the JS thread is 86% busy. The reviewer attributed
about 30% of V8 CPU to scanners and SourceFile reparsing, 13% to node lookup/dispatch, 8.6% to GC,
and only 1.7% to SDK checker code. The checker-end committed V8 memory was about 1.1 GB, falling
to 284 MB after checker release. These are reviewer measurements, not this implementation's samples.
No shared type graph, checker-count change, lazy DTS, or normal/strict pool retirement is included.

The existing `check_source_file_worker` and split-file piece lifecycle now flush pending syscap
uses after their deferred checks. The checker adds returned warnings through its original diagnostic
collection, in discovery order; the existing final sort/dedup and suppression logic remain responsible
for output. Queries outside a file check flush immediately. Function plugins remain individual calls.
No scheduler was added. SourceFile/node pointers are held only by the checker while that file check
is active; only scalar keys and owned results enter the executor cache.

The executor reserves all uncached keys of the file together, coalescing repeated uses both inside
one batch and across concurrent overlapping batches. Cache hits never cross the host boundary.
Reservations wait before claiming any partial set, avoiding cycles; waits and host calls hold no map
or registration lock. Successful raw responses are cached; load/invoke/transport errors and skipped
callbacks are not. Config/source/descriptor publication uses one acknowledged gate for the reserved
batch, including registrations attached to skipped callbacks. No N-API FIFO assumption is needed.

Wire requests remain an array of the existing interned node/config/plugin requests. An optional
`after` index identifies the previous uncached class callback for the same use. JS preserves class
order and skips successors after false results or invocation errors, returning `{skipped: true}`;
load errors retain the previous missing-plugin behavior. Rust folds cached and uncached responses
in the original chain, including cached false results and the final successful checker's message.
The external `check(ts.Node, ts.Node, projectConfig)` ABI and function arguments are unchanged.

JS registers the batch's resources and resolves nodes grouped by TypeScript module, session and
sourceId, then invokes callbacks in wire order. Traversal prunes on raw pos/end; only end-matching
candidates call getStart. Exceptional legacy text still requires exact getText equality. Declaration
ASTs remain resident. The application LRU remains 64 entries: a file batch normally uses one application
AST, with headroom for interleaved checkers. Eviction drops its node/child indexes with the AST; source
text remains available for reparsing. A batch can temporarily retain resolved nodes from more than
64 application files until its callbacks finish; the persistent LRU itself still holds only 64.

Regression coverage includes one host call per file version after update_program; exact normal/strict
warning bytes with immediate warnings interleaved with deferred warnings and canIUse suppression;
plugin-chain short circuit, mixed hits/misses, failed-call retry, skipped descriptor publication, and
concurrent reversed overlapping batches. JS counters prove that 70 interleaved sourceIds each parse
once in a batch, callback order is unchanged, only end-matching candidates call getStart, the newest
64 application ASTs remain cached, and declarations survive eviction. Existing real-node transport,
legacy-text, incremental source version, and function-cache tests remain in place.

Each released JS session reports parse/reparse/batch counters through typed internal tracing. It also
reports V8 `total_physical_size` sampled after batch node preparation and after session release/GC.
Those are process-wide committed V8 heap samples, not a continuous vmmap peak or process footprint.
The owner will perform the authoritative profile; `sample` is deliberately not run in the sandbox.
Retired instructions, comparable before/after RSS and authoritative V8/footprint acceptance remain
pending. The repository performance landing threshold still requires the owner's measurements.

### R3 sandbox cold run and verification

The rebuilt release binding produced the following single authorized run. No other acceptance build
or profiler ran concurrently. Diagnostics (10,231), ABC/source-map hashes and the unsigned HAP entry
list (name, CRC, size, compression) all match the fresh baseline.

```text
r3\tcold\trun1\tok=1\tpeak=rss1.70GB\twall=111.64s\tstartup=14738ms\tprofiles=13959\tbundling=36049\tchecker=9834\tabc=8672\tpackage=37539\tdiag=same\tartifacts=same\thap=same
```

The wall time and Node maxRSS fallback (1,703,696 KB) are sandbox-relative, not an acceptance footprint.
The checker took 9.834 s; the <=4 s target is **not demonstrated**. The session recorded 5,250 syscap
batches, 5,556 SourceFile parses and only 3 reparses (0.054%). This supports retaining capacity 64:
reparse churn is already negligible, while increasing capacity would retain more application ASTs.

Committed V8 heap samples peaked at 958,611,456 bytes (958.6 MB) and fell to 198,049,792 bytes
(198.0 MB) after release/GC. Relative to the reviewer's R1c observations of ~1.1 GB / 284 MB,
these are nominally ~141 MB / 86 MB lower. The instruments and sampling points differ (V8
`total_physical_size` versus vmmap), so this is **not a validated before/after reduction**; the
reviewer's same-instrument rerun is still required. The <=400 MB checker committed-memory target
is **not met by these samples**, and no claim about the JS CPU bottleneck is made without profiling.

Verification: tsrs API 101 tests passed, checker 10 passed; lint ratchet passed (3 findings, none new)
and source.py passed. Arkdown checker/build tests passed (136 passed, 2 existing ignored), workspace
clippy and fmt passed. Node tests passed 31/32; the sole failure is the expected original lockfile-pin
assertion under the local Cargo patch. The guard is byte-identical to both its pre-Round-1 and pre-R3
backups. `plugin-adapter.js` is 44,979 bytes (<44*1024). `npm run build` generated the arm64 Mach-O
N-API binding. No commits or guard changes were made.


## R4a: retire the normal build pool before the strict linter

Scope/evidence: arkdown `meta/tasks/PERF-CHECKER-SDK-PLUGIN-MEMORY.md`, sections
“R3 验收”, “R4a 范围（内存）” and “normal / strict pool 串行释放”. Only R4a items
1 and 2 are implemented. No async syscap pipeline or lazy-DTS option is included.

The reviewer's R3 cold reruns measured 8.3 / 9.1 s checker time and 4.28 / 4.31 GB
peak footprint, with identical diagnostics, ABC/source-map hashes and HAP entries.
The late strict pass overlapped live normal checkers: +715 MB arena and +568 MB
mimalloc. Front-end parse/bind was ~0.87 GB; normal/strict initialization arena
reservations were 872 / 1261 MiB. V8 live heap peaked at 268 MB with frequent GC;
without it committed memory was ~0.88 GB. The reviewer's timer-GC experiment
reduced total peak from 4.30 to 3.84 GB without changing checker time. These are
reviewer measurements, not new sandbox measurements.

This reuses the region/retirement mechanism in `mem-recycle-checkers.md`. It does
not revisit the rejected shared-graph experiments catalogued in
`perf-round2-followups.md`, merge checker graphs, or retire a checker during a
file pass. It removes the observed overlap of completed / pending build phases.

The compiler now separates normal collection from strict collection. It captures
normal comparison sets as immutable SourceFile node IDs -> `(code,pos,len)` sets.
The embedded `arkts_normal_diagnostics` returns an opaque snapshot-bound token
containing those keys and deeply owned diagnostic text, UTF-16 positions, message
chains and related information. After owned annotation and transform facts are
collected, `arkts_strict_diagnostics` consumes that token, retires all normal build
checkers, then runs the existing strict scheduler / diagnostic filtering. Returned
list order remains linter, syntactic, semantic (and declaration diagnostics in the
arkdown caller). Existing `arkts_build_diagnostics` callers retain its behavior.

`SlotChecker::retire` is shared with `retire_checker`: drop the checker heap state
inside its RegionScope/ScratchScope, leave the scope, then drop its region. Slots
remain stable for mutex guards; a subsequent query recreates a retired checker
on acquisition. Retirement is unsafe in the raw compiler API, with a no-outstanding-
checker-values / no-concurrent-queries contract. The embedded API satisfies that
contract with its existing serialized executor and owned result conversion. API
TypeHandles still use the separate external pool. Annotation transform facts now
use the normal build pool, as the other value-only transform facts already did.
Transform-only builds release that pool too, without creating a strict pool.

SDK host GC runs once at the end of a dispatched batch, after node-preparation
locals have gone out of scope. It checks heapUsed >128 MiB and >2x the last
post-explicit-GC reading (initially host-construction heapUsed), then invokes
`global.gc()` when available and records the new post-GC reading. Session-release
GC updates the same baseline. No timer, worker, ABI change, or function batching
was introduced. The declaration cache and 64-entry application AST LRU remain.

Regression coverage compares every owned diagnostic field and original list
order against the existing combined API in normal, strict-only and no-linter
modes. It reads annotation/type facts after retirement, keeps external TypeHandles
live, reacquires retired checkers, rejects foreign snapshot tokens, and releases
transform-only checkers. `TSRS_ARENA_POISON=1 cargo test -p tsrs_api --test
embedded_test` passes all 8 tests. The JS test checks both exclusive thresholds,
the updated baseline after GC, batch callback completion before collection, and
operation without exposed GC. The original guard test is unchanged.

Retired instructions, authoritative before/after RSS/footprint and V8 committed
memory acceptance remain pending the owner's rerun. The repository performance
landing threshold must be evaluated with those measurements. No `sample` was run.


### R4a sandbox verification and single cold run

```text
r4a	cold	run1	ok=1	peak=rss1.45GB	wall=106.12s	startup=13311ms	profiles=5908	bundling=41106	checker=11102	abc=11484	package=33688	diag=same	artifacts=same	hap=same
```

The 10,231 diagnostics, fresh-baseline ABC/source-map hashes and HAP entry list all
match. The Node maxRSS fallback was 1,453,632 KB (R3 sandbox: 1,703,696 KB), about
14.7% lower. This is sandbox RSS, not the reviewer's footprint. Wall time 106.12 s
and checker diagnostic time 11.102 s are sandbox-relative; transform fact time
(1.360 s) is excluded from the diagnostic timer as before. Cleanup of this run's
debug incremental generations/test executables left ~7.7 GiB free before the
bench; disk remains nearly full. No second acceptance build or sample was run.

At the same JS batch-preparation sampling points, peak V8 committed heap was
532,267,008 bytes versus R3 sandbox's 958,611,456 bytes: about 44.5% lower.
Post-session-release committed heap was 155,713,536 versus 198,049,792 bytes.
These V8 total_physical_size samples are not continuous vmmap measurements;
**the <=400 MB committed-memory target is not demonstrated**. The requested
2x-growth / >128 MiB policy is implemented unchanged; authoritative footprint,
V8 and timing acceptance remains with the reviewer.

Verification: tsrs API 102 passed; compiler 22 passed; checker 10 passed; poison
embedded target 8 passed. Ratchet passed (3 findings, none new); source checks
passed (681 files, 31 Send/Sync impls, 79 pre-existing uncommented orderings).
Arkdown checker/build tests passed (136 passed, 2 existing ignored); clippy and
fmt passed. Node 32/33 passed: the sole failure remains the original git lockfile
pin assertion under the local Cargo patch. Both original guard backups match
byte-for-byte. The adapter is 45,046 bytes (<45,056). The release arm64 Mach-O
binding was rebuilt successfully. Logs and R4a-only diffs are in the temporary
review directory; no commits, guard edits, or baseline changes were made.


## R5: upstream linter qualified-name cache and shared snapshot text

Scope: arkdown `meta/tasks/PERF-CHECKER-SDK-PLUGIN-MEMORY.md`, “R2 归因结果”
and “R5 范围”, items 1 and 2. R2 supplied new evidence for this exact upstream
port: ets-loader `typescript.js:48957`, `getFullyQualifiedName`, guards on
`isTypeCheckerForLinter` and absent `containingLocation`. The previously rejected
shared-type-graph, speculative arena scopes, and mmap proposals are not needed.

The checker owns an optional FxHashMap keyed by `P<Symbol>` identity, with owned
String values. Only `new_arkts_linter_checker` creates it. A location bypasses it;
an empty cached string follows upstream's falsy-result miss behavior. The outer
result is cached, while `get_fully_qualified_name_no_cache` recursively computes
parents using the original algorithm. Checker retirement drops the table and
strings. No cache is created for normal checkers.

`FS::read_file_shared` defaults to the existing owned read converted to Arc<str>.
The embedded snapshot, request filesystems, forwarding wrappers and file handles
preserve shared allocations. SnapshotFS cache insertion/reload and overlay
version copies retain Arc<str>; hashes and change notifications retain their
existing meaning. Mutable editor changes produce new immutable text. This
removes the ownership duplication attributed to SnapshotFS in R2 (79.58 MiB),
without rereading overlay files. R5 did not repeat DHAT, so 79.58 MiB is the R2
attribution, not a fresh measured R5 heap delta.

Tests compare cold and cached nested/merged symbol names against normal-checker
results, verify that recursive parents are not cached, and verify that repeated
normal queries still allocate formatter contexts while linter hits do not.
Snapshot tests assert Arc pointer sharing and unchanged old-version content.
The arkdown embedded test proves tsrs retains the input allocation, updates a
changed version's diagnostic, and keeps the previous snapshot readable. Existing
preprocessing and incremental/sourceId regressions also pass.

### Sandbox measurements (2026-10-13; acceptance pending)

One cold build of the 13,487-file arkdown acceptance application used temporary
per-checker counters and existing `TSRS_MEM_SPLIT=1`, with all workers joined at
each phase boundary. No sample or vmmap. Parent recursion is excluded from the
outer-cache counts; each of the four checkers has its own table.

- Hits **303,887**, misses **79,036**, hit rate **79.36%**.
- Strict semantic end: **234.13 MiB used**, versus R2 **234.11 MiB**.
- Strict linter end: **354.70 MiB used**, versus R2 **685.35 MiB**:
  **-330.65 MiB / -48.24%**.
- Strict checker capacity: **412.00 MiB**, versus R2 **762.41 MiB**.
- Total compressed-arena chunk reservation at strict-linter end: **1,388.7 MiB**,
  versus R2 **1,739.2 MiB**. These are allocation/reservation measurements, not
  resident physical footprint. R2 used the system allocator plus census; R5 used
  normal mimalloc. The nearly identical strict-semantic used bytes provide a
  phase-local cross-check; worker assignment can still move counts slightly.

```text
r5	cold	run1	ok=1	peak=rss2.17GB	wall=75.18s	startup=11698ms	profiles=2955	bundling=30947	checker=7787	abc=8104	package=21031	diag=same	artifacts=same	hap=same
```

10,231 diagnostics, the ABC/source-map hashes, and HAP entries all match the
baseline. Checker diagnostics took **7.787 s**, full wall **75.18 s**. The fallback
Node maxRSS was **2,169,200 KB** (2.17 decimal GB). This is sandbox-relative and is
not comparable to the reviewer's R4a 3.46/3.48 GB footprint. The older R4a sandbox
maxRSS was 1,453,632 KB under different disk/memory pressure: these runs do not
establish a whole-process RSS reduction. Authoritative peak footprint and retired
instructions remain pending acceptance; no claim of meeting the 2.0 GB target.

The four-file instrumentation patch was removed by exact byte restoration of the
pre-counter R5 sources, and the production binding was rebuilt. The patch and
raw measurements were kept outside the repository for audit.

Validation: tsrs API 102, checker 10, compiler 23, project 105, VFS 87 tests pass;
ratchet passes (3 findings, none new); source.py passes. Arkdown checker/build:
137 pass, 2 existing ignored; clippy and fmt pass. Node 32/33 pass; only the
expected local-patch lockfile assertion fails. No guard or JS adapter edits;
the adapter remains 45,046 bytes. Release arm64 N-API binding built successfully.

## R6: per-call NodeBuilder state (2026-10-13)

Scope and acceptance: arkdown's
`meta/tasks/PERF-CHECKER-SDK-PLUGIN-MEMORY.md`, “R5 验收” and “R6 范围（内存）”.
The accepted R5 peak was 2.71/2.78 GB and its checker took 7.6/7.4 s. This round
revisits the narrow, explicitly identified lifetime in `mem-emit-regions.md`;
it does not retry the rejected whole-checker scratch scope or recreate the
persistent builder/caches. Upstream ets-loader TypeScript's
`createNodeBuilder` / `withContext` likewise returns the synthesized result
without retaining its per-call context.

### R5 measurement before choosing the change

The R2 arena-site/census and DHAT patch was temporarily applied to R5, built in
an isolated target directory, and copied beside isolated JS. DHAT ran two
separate sessions: normal pool creation through normal transform-facts end,
then strict pool creation through strict-linter end. Arena rows were filtered
by the four checker-region identities at each endpoint. Counts below are both
created and retained counts: R5 did not reclaim these per-call objects before
pool retirement. DHAT uses the system allocator; bytes are requested live
allocation sizes at the endpoint (`eb`), not mimalloc usable sizes or RSS.

| Pool endpoint | Contexts | Trackers | Context arena bytes | Tracker arena bytes | Per-call owned heap bytes | Total MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Normal, after facts | 3,858 | 3,858 | 1,604,928 | 92,592 | 925,104 | 2.501 |
| Strict, after linter | 218,234 | 218,368 | 90,785,344 | 5,240,832 | 52,129,032 | 141.292 |

Strict per-call heap attribution: four COW Rc backing objects **41,900,928 B**;
tracked-symbol Vec buffers **10,107,216 B**; symbol-depth / visited-type /
enclosing-symbol tables **120,536 B**; COW symbol-set buckets **352 B**.
Normal: **740,736 B**, **128,592 B**, **55,776 B**, and **0 B**, respectively.
Persistent emit metadata, serialized-type caches, identifiers and other result
nodes are deliberately excluded. The strict endpoint exceeds the requested
100 MB threshold. The 134 additional strict trackers are recovery trackers.

The instrumented run's 10,231 rendered diagnostics equal the baseline under
`LC_ALL=C`. The controller stopped the process after checking and rendering
diagnostics (exit -15). Instrumentation overhead makes its 782 s checker time
unsuitable for performance comparison. Its source changes were restored
byte-for-byte before implementing R6, and its throwaway target was deleted.
Raw arena TSVs, endpoint DHAT JSON, attribution script and measurement patches
were kept outside the repository.

### Lifetime change

Each NodeBuilder call owns a minimum-page `Region::new_scratch`. It is entered
only while constructing the context and its initial/recovery SymbolTrackerImpl
objects. The allocation target is restored before executing checker work or
constructing synthesized results. Nested calls save the previous context and
region. Pop restores the previous context before dropping the completed call's
region, which also drops its COW Rc backing storage and Vec/Map buffers.

The persistent builder, link stores, serialized-type caches, recovery boundaries,
wrappers and result nodes keep their existing owners. No call is skipped or
reordered. No allocator/free-list implementation was added; this uses the
existing scratch-region drop and slab reuse machinery. The new compiler test
prints saved results after repeated and nested calls with both checker modes.
It and all eight embedded API tests pass with `TSRS_ARENA_POISON=1`.

### Companion V8 experiments: rejected

Eight isolated JS probes used the unmodified R5 binding and recorded heapUsed,
heapTotal and physical heap at batch ends. All diagnostic / artifact / HAP-entry
gates passed. No timers, sample or vmmap were used.

| Policy | Checker s | Max heapUsed MB | Max heapTotal MB | Max physical MB |
| --- | ---: | ---: | ---: | ---: |
| Existing R5 policy | 7.677 | 501.98 | 534.40 | 534.30 |
| Plain GC, 32 MiB growth above 96 MiB | 9.921 | 314.74 | 347.19 | 347.10 |
| Last-resort major GC, same growth policy | 10.060 | 312.33 | 339.94 | 338.03 |
| Async major GC, 16 MiB growth above 96 MiB | 10.825 | 299.80 | 344.11 | 339.38 |
| Optimize-for-size and heap-growing-percent=10, R5 trigger | 9.434 | 311.50 | 327.88 | 322.99 |
| Last-resort major GC above 296 MB physical | 12.804 | 286.44 | 311.43 | 295.99 |
| 1 MiB semi-space, heap-growing-percent=5, compact every full GC, R5 trigger | 11.669 | 308.41 | 317.49 | 317.49 |
| 1 MiB semi-space, last-resort GC above 296 MB physical | 11.525 | 287.14 | 296.37 | 295.98 |

MB here is decimal. These are batch-end samples, not continuous whole-process
footprint measurements. Plain/last-resort post-GC live heap still reaches
281.73/280.62 MB, leaving little headroom under 300 MB. The final combination
met the batch-end memory limits but measured 50.1% longer than the earlier checker baseline; it was not
landed. Node 22's last-resort path requests memory reduction and runs at least
two major collections (`deps/v8/src/extensions/gc-extension.cc` and
`deps/v8/src/heap/heap.cc::CollectAllAvailableGarbage`). No measured policy met
both R6 GC gates. The arkdown adapter/CLI and their guards remain unchanged;
the adapter is 45,046 bytes. The GC part of R6 remains unmet, not claimed as
complete. Retained JS data or a relaxed memory/time constraint needs separate
review before changing that boundary.

Retired-instruction and authoritative footprint comparisons await acceptance;
no claim is made of reaching the overall 2.0 GB target.

### R6 validation and relative bench

API **102**, checker **10**, compiler **24** tests pass; the new lifetime test
passes separately under arena poisoning, as do **8** embedded tests. Ratchet:
**3 findings, none new**. Source check: **681 files / 31 Send/Sync impls**,
no new findings. Arkdown checker/build: **137 pass, 2 existing ignored**;
Clippy and formatting pass. Node: **32/33 pass**, only the expected git-pin
assertion fails while the local Cargo patch exists. No guard was changed.
`npm run build` produced the normal arm64 release binding.

```text
r6	cold	run1	ok=1	peak=rss1.64GB	wall=82.12s	startup=12551ms	profiles=3252	bundling=36057	checker=10499	abc=8675	package=21217	diag=same	artifacts=same	hap=same
```

```text
r6-repeat	cold	run1	ok=1	peak=rss1.12GB	wall=130.05s	startup=12827ms	profiles=4888	bundling=65744	checker=16824	abc=16945	package=29217	diag=same	artifacts=same	hap=same
```

Both runs have byte-identical diagnostics, artifact hashes and HAP entry lists.
The initial R5 probe was 7.677 s checker / 77.01 s wall / 2.20 GB sandbox maxRSS;
R6 measured 10.499 / 16.824 s checker and 1.64 / 1.12 GB maxRSS. These numbers
are relative only. Bundling also changed from 36.057 to 65.744 s between the R6
runs, and the host one-minute load average was 24.77 during the second run.
This does not establish a causal RSS delta or a no-regression timing result;
authoritative timing/footprint acceptance remains necessary. The scoped Rust
change is correctness-verified, not claimed as a performance acceptance.

The before/after file inventories show only the four checker implementation
files, compiler regression test, and this note changed in tsrs. Arkdown source
files match their R6-start hashes. Temporary profiler/GC bindings, logging JS,
and the throwaway target directory were removed. No commits, resets, stashes,
pushes, guard edits or allocator/lint suppressions were made.

### R6 acceptance: measured and rejected; reverted for R7a

The independent acceptance reruns reported peak footprint **2.74 / 2.73 GB**,
versus R5 **2.71 / 2.78 GB**. The strict-end vmmap peak remained **2560 MB**;
the strict arena was only about **50 MB** smaller (under 2% of process peak,
below the 5% memory acceptance bar). Checker-time non-regression could not be
established under the observed host load. The 141 MiB retained-allocation
attribution above therefore did not predict an equivalent process-peak saving.

The per-call NodeBuilder scratch-region experiment is **measured and rejected**.
Its four implementation files and compiler test were reverse-applied from the
saved R6 patch. The tracked diff matches the saved R5 diff byte-for-byte; the
only file differing from the R5 inventory is this note. The preceding lifetime
and validation sections describe the reverted experiment, not current behavior.
The V8 probes remain rejected too; none met both memory and time requirements,
and no GC policy change was landed. Do not retry either idea without new
retained-data evidence or a changed acceptance constraint. See arkdown's
`meta/tasks/PERF-CHECKER-SDK-PLUGIN-MEMORY.md`, “R6 验收” and “R7a 范围”.

### R8: bounded syscap submission inside the normal checker group (2026-10-13)

Scope/source: arkdown `meta/tasks/PERF-CHECKER-SDK-PLUGIN-MEMORY.md`, “R8 范围”
(the syscap pipeline previously deferred from R3/R4b). The new evidence is that
normal file checks still wait for JS reparsing/SDK callbacks, preventing overlap;
strict checking makes no SDK calls. The user explicitly retained real ets-loader
TypeScript nodes, so lazy app nodes and GC-policy changes remain out of scope.
`api_check_utils.ts::checkSyscapAbility` and ets-loader TypeScript's
`expressionCheckByJsDoc` / `collectDiagnostics` remain the diagnostic owners.

Each normal checker submits the file's batch and continues within the existing
`checkerPool::for_each_checker_group_do_ex` work group. A finish callback drains
all replies before the original per-file semantic diagnostic/directive filtering
runs; no new scheduler or worker thread is introduced. The bound is **two batches
per checker**: one can run in JS while the checker computes the next file, without
retaining a program-sized queue. File/use order is preserved when draining.
Suppression is captured in the originating file's end-of-check context; the
syscap branch only reads AST annotations/comments and already-resolved canIUse
symbol links. It never skips SDK callbacks based on that captured result.
`after`/`skipped`, result caching, diagnostic collection/sorting, function plugin
calls and the strict pass retain their existing behavior.

N-API submission uses its existing nonblocking threadsafe-function callback.
Only owned transport values cross it; arena-backed source snapshots are copied
before submission returns. Callback completion publishes results and releases
exact-key reservations independently of when the checker drains its reply.
Each immutable source/config/descriptor ID has a stable acknowledgement cell.
Until any full publication succeeds, each in-flight batch resends the payload.
A failed or newer pending resend cannot conceal an earlier successful publication.
This requires **no FIFO guarantee** from N-API. The host still registers all batch
payloads before running callbacks, including skipped requests. JS retains the existing projectConfig object
on a full resend of an already-registered ID, preserving plugin-visible identity.
Non-event-loop
hosts retain their synchronous transport implementation.

Verification: `cargo test -p tsrs_api -p tsrs_checker -p tsrs_compiler`: 136 passed;
SDK integration tests with `TSRS_ARENA_POISON=1`: 14 passed. Concurrency tests hold
several replies across two submitting threads, process them out of order, and
verify full resends followed by id-only references even with a newer registration
still pending. The embedded test holds two file replies, proves bounded progress,
and compares complete ordered diagnostics against synchronous checking, including
canIUse (identifier argument), @SuppressWarnings, @ts-ignore and @ts-expect-error;
it retires the normal regions after draining. Existing cache/error/short-circuit
and source-version tests pass. Ratchet: 3 findings, none new. Source checks pass.
Arkdown checker/build: 137 passed, 2 existing ignored. Node: 33/34, only the expected
local-patch lockfile-pin guard fails. Clippy, fmt and the release binding build pass.

Sandbox cold acceptance run:

```text
r8 cold run1 ok=1 peak=rss2.07GB wall=106.76s startup=7180ms profiles=3542 bundling=31861 checker=7676 abc=8597 package=55330 diag=same artifacts=same hap=same
```

Requested log timestamps: Bundling 10:34:11.950Z -> type facts completed
10:34:19.594Z -> ArkTS build diagnostics completed 10:34:21.382Z:
**normal including creation/facts = 7.644 s; strict = 1.788 s**.
The reviewed R7b normal/strict intervals were 9.3–9.9 / 1.9–2.3 s on the user's
machine. These are not a controlled before/after pair: sandbox wall/RSS and a
single run do not establish acceptance. Free disk fell to ~2.1 GiB during this
run, despite cleaning both development targets. Packaging took 55.330 s.
The harness's fallback maxRSS (2.07 GB) excludes compressed memory; it is not
comparable to the user's vmmap/footprint peak. Retired instructions and OS process
profiles were not available/measured in this sandbox; no sample/vmmap was run.
Independent timing/footprint acceptance remains pending.

A throwaway external Node preloader sampled `performance.eventLoopUtilization()`
every 50 ms. In the normal interval: **5.754 s active / 1.890 s idle, 75.28% active**.
The enclosing samples bound the active share to 75.279–75.280%; this measures event
loop activity, not a CPU-stack profile. The preloader was removed after measurement;
the only production JS change is idempotent config registration.
`plugin-adapter.js` is 45,049 bytes, its 64-entry
AST LRU and batch-end GC trigger unchanged. Both development targets were cleaned
after verification. Detailed command tails and measurements were kept outside the repository
in the measurement environment.
