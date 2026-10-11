// Protocol 10 and native snapshots, using tsrs's owned SDK (no Go client).
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { API as SyncAPI } from '../../../npm/tsrs/dist/api/sync/api.js';
import { API as AsyncAPI } from '../../../npm/tsrs/dist/api/async/api.js';
import { NodeFlags, ScriptKind, SyntaxKind } from '../../../npm/tsrs/dist/ast/index.js';
import { RemoteSourceFile } from '../../../npm/tsrs/dist/api/node/node.js';
import { Wtf8Decoder } from '../../../npm/tsrs/dist/api/node/wtf8.js';
import { encodeNode } from '../../../npm/tsrs/dist/api/node/encoder.js';
import { cloneNode } from '../../../npm/tsrs/dist/ast/factory.generated.js';

const root = path.resolve(import.meta.dirname, '../../..');
const data = fs.readFileSync(path.join(root, 'target/arkts-codec.bin'));
const decoder = new Wtf8Decoder();
function source(bytes) { return new RemoteSourceFile(bytes, decoder); }
function inspect(file) {
    assert.equal(file.scriptKind, ScriptKind.ETS);
    assert.equal(file.statements[0].importClause.isLazy, true);
    assert.equal(file.arktsParseOptions.etsAnnotationsEnable, true);
    assert.equal(file.statements[1].flags & NodeFlags.Annotation, NodeFlags.Annotation);
    const struct = file.statements[2];
    assert.equal(struct.flags & NodeFlags.Struct, NodeFlags.Struct);
    assert.equal(struct.members[0].isVirtual, true);
    assert.equal(cloneNode(struct.members[0]).isVirtual, true);
    const build = struct.members.find(n => n.kind === SyntaxKind.MethodDeclaration);
    const ui = build.body.statements[0].expression;
    assert.ok(ui.etsBody);
    assert.equal(ui.etsBody.statements.length, 1);
}
inspect(source(data));
const encoded = encodeNode(source(data));
inspect(source(encoded));
inspect(source(encodeNode(cloneNode(source(data)))));
fs.writeFileSync(path.join(root, 'target/arkts-codec-client.bin'), encoded);

const cwd = fs.mkdtempSync(path.join(os.tmpdir(), 'tsrs-sdk-arkts-'));
const input = path.join(cwd, 'input.ets');
fs.writeFileSync(input, '@interface Meta { value: string = "默认"; } @Meta class Example {} export const value: string = 42;');
const loader = path.join(cwd, 'sdk/openharmony/ets/build-tools/ets-loader');
const sdkApi = path.join(cwd, 'sdk/api');
const kitInput = path.join(cwd, 'kit.ets');
fs.mkdirSync(path.join(loader, 'kit_configs'), { recursive: true });
fs.mkdirSync(sdkApi, { recursive: true });
fs.writeFileSync(path.join(loader, 'kit_configs/@kit.Test.json'), JSON.stringify({ symbols: { Foo: { source: '@ohos.foo.d.ts', bindings: 'Foo' } } }));
fs.writeFileSync(path.join(sdkApi, '@ohos.foo.d.ts'), 'export class Foo { value: number }');
fs.writeFileSync(kitInput, '/*😀*/ import { Foo } from "@kit.Test"; const value: number = new Foo().value;');
try {
    for (const API of [SyncAPI, AsyncAPI]) {
        const api = new API({ cwd, tsserverPath: path.join(root, 'target/debug/tsrs') });
        try {
            const printed = await api.printer.printNode(source(data));
            assert.match(printed, /struct Page/);
            assert.match(printed, /@interface Mark/);
            const standalone = await api.createSourceFile(input, '@interface Meta { value: string = "x" }', { scriptKind: ScriptKind.ETS, etsAnnotationsEnable: true });
            assert.ok(standalone.sourceFile.statements[0].flags & NodeFlags.Annotation);
            assert.equal(standalone.sourceFile.arktsParseOptions.etsAnnotationsEnable, true);
            const retained = await api.retainSourceFile(standalone.sourceFile);
            assert.equal(retained.sourceFile.parseOptionsKey, standalone.sourceFile.parseOptionsKey);
            await standalone.dispose();
            await retained.dispose();
            const snapshot = await api.createSnapshot({ createPrograms: [{ rootFiles: [input], compilerOptions: { noEmit: true, etsLoaderPath: cwd, etsAnnotationsEnable: true } }] });
            const program = snapshot.operation.createdPrograms[0];
            const diagnostics = await program.getArkTSBuildDiagnostics();
            assert.ok(diagnostics.semanticDiagnostics.some(d => d.code === 2322));
            const file = await program.getSourceFile(input);
            assert.equal(file.scriptKind, ScriptKind.ETS);
            assert.equal(file.arktsParseOptions.etsLoaderPath, cwd);
            assert.deepEqual((await program.getProgramSourceGraph()).files.find(f => f.fileName === input).dependencies, []);
            const [annotations] = await program.getAnnotationTransformInfos([input]);
            assert.equal(annotations.declarations.length, 1);
            assert.equal(annotations.uses.length, 1);
            const info = annotations.declarations[0].info;
            assert.equal(info.properties[0].initializer.value, "默认");
            assert.deepEqual(await program.getAnnotationInfo(info.declaration), info);
            assert.deepEqual(await program.getArkTSTransformTypeFacts([], []), []);
            await snapshot.dispose();
            const kitSnapshot = await api.createSnapshot({ createPrograms: [{ rootFiles: [kitInput], compilerOptions: {
                noEmit: true, skipLibCheck: true, module: 99, moduleResolution: 100, etsLoaderPath: loader,
                ohSdkConfigs: [{ apiPaths: [sdkApi] }], ohSystemModules: ['@ohos.foo.d.ts'],
            } }] });
            const kitProgram = kitSnapshot.operation.createdPrograms[0];
            assert.deepEqual((await kitProgram.getArkTSBuildDiagnostics()).semanticDiagnostics, []);
            const kitFile = await kitProgram.getSourceFile(kitInput);
            assert.equal(kitFile.statements[0].moduleSpecifier.text, '@ohos.foo');
            assert.equal(kitFile.statements[0].isVirtual, true);
            assert.match(kitFile.arktsParseOptions.kitConfigKey, /^[0-9a-f]{32}$/);
            assert.equal(kitFile.arktsParseOptions.markedKitImportRanges[0][1], kitFile.text.indexOf(';') + 1);
            const kitLease = await api.retainSourceFile(kitFile);
            assert.equal(kitLease.sourceFile.parseOptionsKey, kitFile.parseOptionsKey);
            await kitLease.dispose();
            await kitSnapshot.dispose();
        } finally { await api.close(); }
    }
} finally { fs.rmSync(cwd, { recursive: true, force: true }); }
console.log('ArkTS Node SDK: binary roundtrip, parser options, leases, sync/async native snapshots pass');
