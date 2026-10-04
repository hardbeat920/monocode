"""Qualify stable native locale and provider source in an owned task checkout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

REQUIRED = [
    'catalog_locale_preserves_effort_rank_and_collates_unknown_variants',
    'catalog_locale_keeps_equivalent_model_names_in_provider_order',
    'catalog_locale_preserves_equivalent_pi_and_omp_model_order',
    'catalog_locale_selects_french_and_swedish_pi_and_omp_model_order',
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--source-hashes', type=Path, required=True)
    parser.add_argument('--default-helper', type=Path, required=True)
    parser.add_argument('--node', required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    summary = {'complete': False, 'source': str(args.source), 'steps': [],
               'platform': sys.platform}
    hashes = json.loads(args.source_hashes.read_bytes())
    summary['source_archive_sha256'] = hashes['source_archive_sha256']
    (args.output/'source-sha256.json').write_bytes(args.source_hashes.read_bytes())
    baseline = ['cargo', 'test', '--workspace', '--lib', '--bins',
                '--all-features', '--locked', '--offline', '-j2']

    def record():
        (args.output/'summary.json').write_bytes(
            (json.dumps(summary, indent=2) + '\n').encode('utf-8'))

    def verify(label):
        for item in hashes['records']:
            file = args.source/item['path']
            assert hashlib.sha256(file.read_bytes()).hexdigest() == item['sha256'], file
            assert file.stat().st_size == item['size'], file
        (args.output/(label + '.log')).write_text(
            'Verified {} exact source files\n'.format(len(hashes['records'])))

    def run(label, command):
        print('Running ' + label, flush=True)
        with (args.output/(label + '.log')).open('wb') as log:
            result = subprocess.run(command, cwd=args.source, stdout=log,
                                    stderr=subprocess.STDOUT)
        summary['steps'].append({'step': label, 'command': [str(x) for x in command],
                                 'exit_code': result.returncode})
        record()
        if result.returncode:
            raise subprocess.CalledProcessError(result.returncode, command)

    try:
        verify('source-before')
        for item in hashes['records']:
            (args.source/item['path']).touch()
        run('compile', baseline + ['--no-run', '--message-format=json'])
        executables = set()
        for line in (args.output/'compile.log').read_text(encoding='utf-8').splitlines():
            if not line.startswith('{'):
                continue
            event = json.loads(line)
            if event.get('reason') == 'compiler-artifact' and event.get('executable') \
                    and event['target']['name'] == 'monocode_harness' \
                    and event.get('profile', {}).get('test'):
                executables.add(event['executable'])
        assert len(executables) == 1, executables
        harness = Path(executables.pop())
        summary['harness_binary'] = str(harness)
        summary['harness_sha256'] = hashlib.sha256(harness.read_bytes()).hexdigest()
        inventory = subprocess.run([harness, '--list', '--format', 'terse'],
                                   capture_output=True, check=True)
        (args.output/'test-inventory.log').write_bytes(inventory.stdout)
        inventory_text = inventory.stdout.decode('utf-8')
        for name in REQUIRED:
            assert name + ': test' in inventory_text, name
        run('provider-catalog', [harness, 'catalog_locale', '--nocapture'])
        results = (args.output/'provider-catalog.log').read_text(encoding='utf-8')
        for name in REQUIRED:
            assert name + ' ... ok' in results, name
        assert '4 passed; 0 failed;' in results
        run('harness', [harness])
        run('locale', ['cargo', 'test', '-p', 'monocode-locale', '--all-features',
                       '--locked', '--offline', '-j2', '--', '--nocapture'])
        run('clippy', ['cargo', 'clippy', '--workspace', '--all-targets',
                       '--all-features', '--locked', '--offline', '-j2', '--', '-D', 'warnings'])
        run('probe-build', ['cargo', 'build', '-p', 'monocode-locale', '--example',
                           'locale_probe', '--all-features', '--locked', '--offline', '-j2'])
        target = Path(os.environ.get('CARGO_TARGET_DIR', str(args.source/'target')))
        probe = target/'debug'/'examples'/('locale_probe.exe' if sys.platform == 'win32' else 'locale_probe')
        command = [sys.executable, args.default_helper, '--probe', probe, '--node',
                   args.node, '--output', args.output/'default-locale.json']
        if sys.platform.startswith('linux'):
            command.append('--linux-matrix')
        run('default-locale', command)
        verify('source-after')
        summary['complete'] = True
        record()
        print('Native locale/provider qualification passed ' + str(args.output), flush=True)
        return 0
    except Exception as error:
        summary['error'] = str(error)
        record()
        raise


if __name__ == '__main__':
    raise SystemExit(main())
