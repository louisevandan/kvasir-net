"""Conformance matrix, including deployment replacement and fixed semantic expectations."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
ROOT=Path(__file__).resolve().parents[3]

def quality(request):
 name=request['id']
 expected={'arithmetic':'4','korean':'서울','context':'BLUE','cancel':'Counting','keep':'Jupiter','reuse':'6'}
 text=expected.get(name)
 if name.startswith('wave-'):text='4' if int(name[5:])%2==0 else '서울'
 return request['released'] and request['text'].strip()==text and request['terminal']==('cancelled_at_step_boundary' if name=='cancel' else 'eos')

def main(args):
 for name in ('output','bundle','bundle_b','agent_binary','nodes','deployments'):
  value=getattr(args,name)
  if value is not None:setattr(args,name,value.resolve())
 if args.plan:args.plan=str(Path(args.plan).resolve())
 args.output.mkdir(parents=True,exist_ok=False)
 cases=[(name,'short',1,args.bundle) for name in ('single_gpu','balanced_two_gpu_fp32','uneven_three_stage_fp32','attention_boundaries_fp32','cpu_gpu','single_cpu')]
 cases += [('balanced_two_gpu_fp32',name,1,args.bundle) for name in ('chunked_prefill','interleaved_cancel')]
 cases += [('balanced_two_gpu_fp32','epoch_eight',3,args.bundle),('balanced_two_gpu_fp32','short',1,args.bundle_b)]
 if args.plan:
  cases=[(args.plan,name,3 if name=='epoch_eight' else 1,args.bundle) for name in ('short','chunked_prefill','interleaved_cancel','epoch_eight')]
 records=[]
 for i,(plan,scenario,blocks,bundle) in enumerate(cases):
  name=f'{i:02}-{Path(plan).stem}-{scenario}'
  plan_path=Path(plan) if args.plan else ROOT / f'plans/qwen3_5_0_8b/{plan}/plan.json'
  command=[sys.executable,'-B',str(ROOT / 'scripts/verification/event_qwen/run.py'),'--host',args.host,'--port',str(args.port),
   '--plan',str(plan_path),'--scenario',str(ROOT / f'scenarios/qwen3_5_0_8b/{scenario}/scenario.json'),
   '--bundle',str(bundle),'--output',str(args.output / name),'--blocks',str(blocks),'--agent-binary',str(args.agent_binary)]
  if args.nodes:
   nodes=json.loads(args.nodes.read_text(encoding='utf-8'))
   for node in nodes:node['generation'] += i
   nodes_path=args.output / f'{name}-nodes.json'
   nodes_path.write_text(json.dumps(nodes,indent=2)+'\n',encoding='utf-8')
   command += ['--nodes',str(nodes_path),'--deployments',str(args.deployments)]
  print('START '+name,flush=True)
  with (args.output / f'{name}.log').open('wb') as log:
   run=subprocess.run(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,timeout=900)
  path=args.output / name / 'summary.json'
  result=json.loads(path.read_text(encoding='utf-8')) if path.exists() else {}
  record={'case':name,'command':command,'exit':run.returncode,'ok':run.returncode==0 and result.get('ok',False),
   'quality':bool(result.get('requests')) and all(quality(r) for r in result.get('requests',[])),
   'comparisons':len(result.get('comparisons',[])),'cache_comparisons':len(result.get('cache_comparisons',[])),
   'first_error':result.get('first_error'),'cleanup_error':result.get('cleanup_error'),
   'hosts':sorted({n['host'] for n in result.get('nodes',[])}),'labels':[n.get('build_label') for n in result.get('nodes',[])]}
  record['ok']=record['ok'] and record['quality'] and (not args.plan or len(record['hosts'])==2)
  records.append(record)
  (args.output / 'matrix.json').write_text(json.dumps(records,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
  print(json.dumps(record,ensure_ascii=False),flush=True)
  if not record['ok']:break
 return 0 if len(records)==len(cases) and all(r['ok'] for r in records) else 1
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--host',default='192.168.0.6');p.add_argument('--port',type=int,default=41980)
 p.add_argument('--bundle',type=Path,required=True);p.add_argument('--bundle-b',type=Path)
 p.add_argument('--agent-binary',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
 p.add_argument('--plan');p.add_argument('--nodes',type=Path);p.add_argument('--deployments',type=Path)
 raise SystemExit(main(p.parse_args()))
