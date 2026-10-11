#!/usr/bin/env python3
"""Compare dynamic ArkTS linter diagnostics with the pinned OH TypeScript service.

Uses each backend's native program creation API. Requires a local OpenHarmony
TypeScript source checkout for tests/arkTSTest/testcase; never fetches fixtures.
"""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time


class Service:
    def __init__(self, executable, directory, timeout, callbacks=None):
        self.stderr = tempfile.TemporaryFile()
        command = [str(executable), '--api', '--async', '--cwd', str(directory)]
        if callbacks:
            command += ['--callbacks', callbacks]
        self.process = subprocess.Popen(command,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr)
        self.buffer = b''
        self.sequence = 0
        self.timeout = timeout

    def call(self, method, params):
        self.sequence += 1
        data = json.dumps(dict(jsonrpc='2.0', id=self.sequence, method=method, params=params)).encode()
        self.process.stdin.write(f'Content-Length: {len(data)}\r\n\r\n'.encode() + data)
        self.process.stdin.flush()
        deadline = time.monotonic() + self.timeout
        while True:
            if b'\r\n\r\n' in self.buffer:
                header, rest = self.buffer.split(b'\r\n\r\n', 1)
                length = int(next(line.split(b':', 1)[1] for line in header.split(b'\r\n') if line.lower().startswith(b'content-length:')))
                if len(rest) >= length:
                    self.buffer = rest[length:]
                    response = json.loads(rest[:length])
                    if 'method' in response:
                        reply = json.dumps(dict(jsonrpc='2.0', id=response['id'], result=self.callback(response['method'], response.get('params')))).encode()
                        self.process.stdin.write(f'Content-Length: {len(reply)}\r\n\r\n'.encode() + reply)
                        self.process.stdin.flush()
                        continue
                    if 'error' in response:
                        raise RuntimeError(response['error'])
                    if response.get('id') == self.sequence:
                        return response.get('result')
                    continue
            ready, _, _ = select.select([self.process.stdout], [], [], max(0, deadline - time.monotonic()))
            if not ready:
                raise TimeoutError(method)
            chunk = os.read(self.process.stdout.fileno(), 65536)
            if not chunk:
                self.stderr.seek(0)
                raise RuntimeError(self.stderr.read().decode(errors='replace')[-4000:])
            self.buffer += chunk

    def close(self):
        self.process.kill()
        self.process.communicate()
        self.stderr.close()


def diagnostics(executable, source, roots, options, reference, timeout):
    service = Service(executable, source.parent, timeout)
    try:
        if reference:
            created = service.call('createProgram', dict(rootFiles=roots, createProgramOptions=dict(compilerOptions=options)))
            project = created['project']['id']
        else:
            created = service.call('createSnapshot', dict(createPrograms=[dict(rootFiles=roots, compilerOptions=options)]))
            project, = created['operation']['createdPrograms']
        return service.call('getArkTSLinterDiagnostics', dict(snapshot=created['snapshot'], project=project))
    finally:
        service.close()


def run_case(case, args, ets):
    source = case.with_suffix('.ets')
    if not source.exists():
        source = case.with_suffix('.ts')
    roots = [str(source)] + sorted(str(p) for p in source.parent.iterdir() if p != source and (p.name.endswith('.d.ts') or p.name.endswith('.d.ets')))
    options = dict(noEmit=True, strict=False, module=6, moduleResolution=2, target=8,
                   lib=['lib.es2021.d.ts'], skipLibCheck=True, experimentalDecorators=True,
                   etsAnnotationsEnable=True, needDoArkTsLinter=True, compatibleSdkVersion=12,
                   compatibleSdkVersionStage='beta2' if 'arkts-sendable-beta-compatible' in str(source) else 'beta3',
                   ets=ets, singleThreaded=True)
    result = dict(case=str(case.relative_to(args.source / 'tests/arkTSTest/testcase')))
    try:
        expected = diagnostics(args.reference, source, roots, options, True, args.timeout)
        actual = diagnostics(args.tsrs, source, roots, options, False, args.timeout)
        result['pass'] = actual == expected
        if not result['pass']:
            result.update(expected=expected, actual=actual)
    except Exception as error:
        result.update(pass_=False, error=str(error))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--tsrs', type=Path, default=Path('target/debug/tsrs'))
    parser.add_argument('--ets-options', type=Path, default=Path('testdata/arkts/arkui/ets.json'))
    parser.add_argument('--filter', default='')
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--timeout', type=float, default=30)
    parser.add_argument('--output', type=Path, default=Path('target/arkts-linter-oracle.json'))
    args = parser.parse_args()
    args.source = args.source.resolve()
    args.tsrs = args.tsrs.resolve()
    args.reference = args.reference.resolve()
    cases = [p for p in sorted((args.source / 'tests/arkTSTest/testcase').rglob('*.json')) if args.filter in str(p) and (p.with_suffix('.ets').exists() or p.with_suffix('.ts').exists())]
    ets = json.loads(args.ets_options.read_text())
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for result in pool.map(lambda case: run_case(case, args, ets), cases):
            results.append(result)
            if len(results) % 50 == 0:
                print(f'{len(results)}/{len(cases)}: {sum(bool(r.get("pass")) for r in results)} match', flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2) + '\n')
    passed = sum(bool(r.get('pass')) for r in results)
    print(f'{passed}/{len(results)} match; report: {args.output}')
    return 0 if passed == len(results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
