#!/usr/bin/env python3
"""Compare captured OH compiler inputs through each service's native API."""
import argparse
import concurrent.futures
import json
from pathlib import Path
from run import Service


class FixtureService(Service):
    def __init__(self, executable, fixture, reference, timeout):
        self.fixture = fixture
        self.reference = reference
        super().__init__(executable, fixture['cwd'], timeout,
                         callbacks='readFile,fileExists,directoryExists,getAccessibleEntries,realpath')

    def callback(self, method, name):
        if name.startswith('bundled:///'):
            return None if self.reference else dict(kind='useOS')
        files = self.fixture['files']
        prefix = name.rstrip('/') + '/'
        if method == 'readFile':
            value = files.get(name)
            return dict(content=value) if self.reference else dict(kind='missing') if value is None else dict(kind='value', value=value)
        if method == 'fileExists':
            value = name in files
        elif method == 'directoryExists':
            value = name == '/' or any(p.startswith(prefix) for p in files)
        elif method == 'realpath':
            value = name
        elif method == 'getAccessibleEntries':
            names = [p[len(prefix):] for p in files if p.startswith(prefix)]
            value = dict(files=sorted(n for n in names if '/' not in n), directories=sorted({n.split('/')[0] for n in names if '/' in n}))
        else:
            raise RuntimeError(f'Unexpected callback {method}')
        return value if self.reference else dict(kind='value', value=value)


def check_backend(executable, fixture, reference, timeout):
    service = FixtureService(executable, fixture, reference, timeout)
    try:
        options = dict(fixture['compilerOptions'], noEmit=True, singleThreaded=True)
        roots = fixture['rootFiles']
        if reference:
            created = service.call('createProgram', dict(rootFiles=roots, createProgramOptions=dict(compilerOptions=options)))
            project = created['project']['id']
        else:
            created = service.call('createSnapshot', dict(createPrograms=[dict(rootFiles=roots, compilerOptions=options)]))
            project, = created['operation']['createdPrograms']
        params = dict(snapshot=created['snapshot'], project=project)
        return {method: service.call(method, params) for method in ['getSyntacticDiagnostics', 'getSemanticDiagnostics', 'getArkTSLinterDiagnostics']}
    finally:
        service.close()


def run_case(path, args):
    fixture = json.loads(path.read_text())
    result = dict(case=path.name)
    try:
        expected = check_backend(args.reference, fixture, True, args.timeout)
        actual = check_backend(args.tsrs, fixture, False, args.timeout)
        result['pass'] = expected == actual
        if not result['pass']:
            result.update(expected=expected, actual=actual)
    except Exception as error:
        result.update(error=str(error), pass_=False)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=Path('target/arkts-compiler-fixtures'))
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--tsrs', type=Path, default=Path('target/debug/tsrs'))
    parser.add_argument('--filter', default='')
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--timeout', type=float, default=30)
    parser.add_argument('--output', type=Path, default=Path('target/arkts-semantic-oracle.json'))
    args = parser.parse_args()
    args.reference = args.reference.resolve()
    args.tsrs = args.tsrs.resolve()
    fixtures = sorted(p for p in args.fixtures.glob('*.json') if args.filter in p.name)
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for result in pool.map(lambda path: run_case(path, args), fixtures):
            results.append(result)
            if not result.get('pass'):
                print(result['case'], result.get('error', 'differs'), flush=True)
    args.output.write_text(json.dumps(results, indent=2) + '\n')
    passed = sum(bool(r.get('pass')) for r in results)
    print(f'{passed}/{len(results)} match; report: {args.output}')
    return 0 if results and passed == len(results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
