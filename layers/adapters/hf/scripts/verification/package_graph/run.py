"""Actual Cargo graph gate plus an independent duplicate-source negative fixture."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[3]

def gate(metadata):
    identities={name:[{'id':p['id'],'source':p['source'],'version':p['version']}
        for p in metadata['packages'] if p['name']==name] for name in ('p4-adapter','p4-protocol')}
    if any(len(items)!=1 for items in identities.values()):
        raise ValueError(f'duplicate/missing P4 package identity: {identities}')
    return identities

def metadata(root,locked=False):
    command=['cargo','metadata','--format-version','1']+(['--locked'] if locked else [])
    return json.loads(subprocess.check_output(command,cwd=root,text=True))

def main(args):
    args.output.mkdir(parents=True,exist_ok=False)
    baseline=metadata(args.p4,True)
    result={'baseline':gate(baseline)}
    (args.output/'baseline.json').write_text(json.dumps(baseline,indent=2)+'\n',encoding='utf-8')
    copied=args.output/'protocol-copy'
    shutil.copytree(args.p4/'layers/protocol/src',copied/'src')
    (copied/'Cargo.toml').write_text('[package]\nname="p4-protocol"\nversion="0.9.1"\nedition="2024"\n',encoding='utf-8')
    fixture=args.output/'fixture';(fixture/'src').mkdir(parents=True)
    (fixture/'src/lib.rs').write_text('// Dependency graph fixture.\n',encoding='utf-8')
    manifest='[package]\nname="hf-duplicate-source-fixture"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'
    manifest+=f'p4-adapter={{path="{(args.p4/"layers/adapters/adapter").as_posix()}"}}\n'
    manifest+=f'protocol-copy={{package="p4-protocol",path="{copied.as_posix()}"}}\n'
    (fixture/'Cargo.toml').write_text(manifest,encoding='utf-8')
    negative=metadata(fixture)
    (args.output/'negative.json').write_text(json.dumps(negative,indent=2)+'\n',encoding='utf-8')
    try:gate(negative)
    except ValueError as error:result['negative_rejected']=str(error)
    else:raise AssertionError('duplicate source accepted')
    result['fixture_sha256']={str(p.relative_to(args.output)):hashlib.sha256(p.read_bytes()).hexdigest()
        for p in args.output.rglob('*') if p.is_file() and p.suffix in ('.rs','.toml')}
    result['ok']=True
    (args.output/'summary.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--p4',type=Path,default=ROOT.parents[2]);p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();args.output=args.output.resolve();args.p4=args.p4.resolve();main(args)
