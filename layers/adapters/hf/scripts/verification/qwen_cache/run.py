"""Required cache/epoch counterexamples, with isolated Python guard-removal verification."""
from pathlib import Path
import sys,unittest,json,hashlib,subprocess,shutil,os
root=Path(__file__).resolve().parents[3]
sys.path[:0]=[str(root/'python'),str(root)]
from tests.models.qwen3_5_0_8b.cache_epoch.checks import CacheEpochTests
if '--mutations' not in sys.argv:
 result=unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(CacheEpochTests))
 raise SystemExit(0 if result.wasSuccessful() else 1)
output=Path(sys.argv[sys.argv.index('--mutations')+1]).resolve();output.mkdir(parents=True,exist_ok=False)
prefix='python/p4hfadapter/models/qwen3_5_0_8b/'
cases={'cache_elements':(prefix+'cache_export/__init__.py','not torch.allclose(tensor, target, atol=0.125, rtol=0.01)','False'),
 'active_epoch':(prefix+'epoch/__init__.py',' or self.sessions.active',''),
 'old_epoch':(prefix+'epoch/__init__.py','epoch != self.epoch:','False:')}
records=[]
for name,(path,old,new) in cases.items():
 copy=output/name
 for folder in ('python','tests','scripts/verification/qwen_cache'):
  for source in (root/folder).rglob('*.py'):
   dest=copy/source.relative_to(root);dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,dest)
 target=copy/path;text=target.read_text(encoding='utf-8');assert text.count(old)==1
 before=hashlib.sha256(target.read_bytes()).hexdigest();target.write_text(text.replace(old,new),encoding='utf-8',newline='\n')
 run=subprocess.run([sys.executable,'-B',str(copy/'scripts/verification/qwen_cache/run.py')],cwd=copy,capture_output=True,env=dict(os.environ,PYTHONUTF8='1'),timeout=60)
 (copy/'test.log').write_bytes(run.stdout+run.stderr)
 record={'case':name,'exit':run.returncode,'ok':run.returncode==1 and b'FAILED (' in run.stderr,'path':path,'before':before,'after':hashlib.sha256(target.read_bytes()).hexdigest()}
 records.append(record);print(json.dumps(record),flush=True)
(output/'summary.json').write_text(json.dumps(records,indent=2)+'\n',encoding='utf-8')
raise SystemExit(0 if all(r['ok'] for r in records) else 1)
