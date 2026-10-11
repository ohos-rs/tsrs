#!/usr/bin/env python3
"""Capture compiler test inputs from the pinned local OH Go checkout.

Temporary instrumentation is removed in finally; test expectations and the
reference executable are never changed. Only use the OH in-memory test suite.
"""
import argparse
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--reference-source', type=Path, required=True)
parser.add_argument('--output', type=Path, default=Path('target/arkts-compiler-fixtures'))
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
compiler = args.reference_source / 'tsc/internal/compiler'
program = compiler / 'program.go'
instrumentation = compiler / 'tsrs_oracle_capture.go'
original = program.read_bytes()
assert not instrumentation.exists()
try:
    instrumentation.write_text(Path(__file__).with_name('capture.go.txt').read_text())
    program.write_bytes(original.replace(b'func NewProgram(opts ProgramOptions) *Program {', b'func NewProgram(opts ProgramOptions) *Program {\n captureArkTSOracleInput(opts)'))
    result = subprocess.run(['go', 'test', './internal/compiler', '-run', '^Test(OH|ArkTS|ArkUI|Ets|Throws|Annotation)', '-count=1', '-timeout=180s'], cwd=args.reference_source / 'tsc', env=dict(os.environ, TSRS_ARKTS_CAPTURE=str(args.output.resolve()), GOWORK='off', GOTOOLCHAIN=os.environ.get('GOTOOLCHAIN', 'go1.26.0')))
finally:
    program.write_bytes(original)
    instrumentation.unlink()
raise SystemExit(result.returncode)
