"""Export one committed P4 source tree and reproduce both agent feature configurations."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def export(output, repo):
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=repo, text=True).strip():
        raise RuntimeError('commit changes before source export')
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
    output.mkdir(parents=True, exist_ok=False)
    archive = output / 'p4.zip'
    subprocess.run(['git', 'archive', '--format=zip', f'--output={archive}', commit], cwd=repo, check=True)
    helper = output / 'restore.py'
    helper.write_bytes(Path(__file__).read_bytes())
    manifest = {'schema': 2, 'source': {'commit': commit, 'archive': archive.name, 'sha256': digest(archive)},
                'restore_tool_sha256': digest(helper)}
    (output / 'sources.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    return manifest


def restore(bundle, destination):
    manifest = json.loads((bundle / 'sources.json').read_text(encoding='utf-8'))
    if manifest.get('schema') != 2 or digest(bundle / 'restore.py') != manifest['restore_tool_sha256']:
        raise RuntimeError('unsupported or corrupt source bundle')
    item = manifest['source']
    archive = (bundle / item['archive']).resolve()
    if archive.parent != bundle.resolve() or digest(archive) != item['sha256']:
        raise RuntimeError('source archive hash/path mismatch')
    destination.mkdir(parents=True, exist_ok=False)
    root = (destination / 'p4').resolve()
    with zipfile.ZipFile(archive) as zipped:
        if any(not (root / member).resolve().is_relative_to(root) for member in zipped.namelist()):
            raise RuntimeError('source archive path escapes checkout')
        zipped.extractall(root)
    (root / '.p4-source.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    builds = []
    for name, features in (('disabled', []), ('enabled', ['--features', 'hf-transformers'])):
        command = ['cargo', 'build', '--locked', '-p', 'p4-agent', '-p', 'p4-event-drive', *features]
        with (destination / f'build-{name}.log').open('wb') as log:
            run = subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT)
        builds.append({'configuration': name, 'command': command, 'exit_code': run.returncode})
        (destination / 'reproduction.json').write_text(json.dumps({'source': manifest, 'builds': builds}, indent=2)+'\n', encoding='utf-8')
        if run.returncode:
            raise RuntimeError(f'reproduction failed: build-{name}.log')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    subs = parser.add_subparsers(dest='operation', required=True)
    p = subs.add_parser('export'); p.add_argument('output', type=Path); p.add_argument('--repo', type=Path)
    p = subs.add_parser('restore'); p.add_argument('bundle', type=Path); p.add_argument('destination', type=Path)
    args = parser.parse_args()
    if args.operation == 'export':
        repo = args.repo or Path(__file__).resolve().parents[6]
        print(json.dumps(export(args.output.resolve(), repo.resolve())))
    else:
        restore(args.bundle.resolve(), args.destination.resolve())
