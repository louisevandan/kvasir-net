"""Consume Cargo's graph in an isolated source tree; reject a restored sibling dependency."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

REPO = Path(__file__).resolve().parents[6]


def check(root):
    command = ['cargo', 'metadata', '--offline', '--locked', '--features', 'hf-transformers', '--format-version', '1']
    result = subprocess.run(command, cwd=root, capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        raise RuntimeError(result.stderr)
    graph = json.loads(result.stdout)
    expected = {'p4-hf-adapter': 'layers/adapters/hf/adapter/Cargo.toml',
                'p4-adapter': 'layers/adapters/adapter/Cargo.toml', 'p4-protocol': 'layers/protocol/Cargo.toml'}
    for name, manifest in expected.items():
        packages = [p for p in graph['packages'] if p['name'] == name]
        if len(packages) != 1 or packages[0]['id'] not in graph['workspace_members']:
            raise ValueError(f'not a unique workspace member: {name}')
        if Path(packages[0]['manifest_path']).resolve() != (root / manifest).resolve():
            raise ValueError(f'external source: {name}')
    if (root / 'layers/adapters/hf/Cargo.toml').exists() or (root / 'layers/adapters/hf/Cargo.lock').exists():
        raise ValueError('nested HF workspace/lock')
    return {'members': len(graph['workspace_members']), 'packages': expected, 'command': command}


def main(output):
    output.mkdir(parents=True, exist_ok=False)
    archive = output / 'source.zip'
    subprocess.run(['git', 'archive', '--format=zip', f'--output={archive}', 'HEAD'], cwd=REPO, check=True)
    root = output / 'isolated/p4'
    with zipfile.ZipFile(archive) as source:
        source.extractall(root)
    if (root.parent / 'p4hfadapter').exists():
        raise RuntimeError('test precondition violated: sibling exists')
    result = {'source_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'baseline': check(root)}
    manifest = root / 'entrypoints/agent/Cargo.toml'
    original = manifest.read_text(encoding='utf-8')
    assert original.count('../../layers/adapters/hf/adapter') == 1
    manifest.write_text(original.replace('../../layers/adapters/hf/adapter', '../../../p4hfadapter/crates/p4-hf-adapter'), encoding='utf-8')
    try:
        check(root)
    except RuntimeError as error:
        if 'p4hfadapter' not in str(error):
            raise
        result['old_sibling_rejected'] = str(error)
    else:
        raise AssertionError('old sibling dependency accepted')
    manifest.write_text(original, encoding='utf-8', newline='\n')
    result['restored'] = check(root)
    (output / 'summary.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    print(json.dumps({'ok': True, 'members': result['baseline']['members']}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('output', type=Path)
    main(parser.parse_args().output.resolve())
