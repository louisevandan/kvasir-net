"""Cut a real two-host model reply mid-frame, abandon it, and reuse both agents."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
import traceback
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'python'))
from p4hfadapter.integration.transport import Client
from p4hfadapter.integration.packet import pack
from p4hfadapter.models.qwen3_5_0_8b.event_pipeline import Pipeline,COMMAND
from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
from p4hfadapter.models.qwen3_5_0_8b.scenarios import read_scenario
from p4hfadapter.models.qwen3_5_0_8b.scheduling import drive
from p4hfadapter.models.qwen3_5_0_8b.reference import Reference
from p4hfadapter.models.qwen3_5_0_8b.cache_export import compare
from safetensors.torch import save
from transformers import AutoTokenizer
import torch

class Reconnected(Client):
    def exchange(self,target,content,payload,adapter=None):
        event_id=self.send(target,content,payload,adapter)
        for _ in range(8):
            meta,body=self.receive()
            if meta['correlation']==event_id:return meta,body
            if meta['correlation']!=self.abandoned:
                raise AssertionError('unrelated late result')
            self.late.append({'envelope':meta,'body_sha256':hashlib.sha256(body).hexdigest()})
        raise AssertionError('unbounded late results')

def main(args):
    args.output.mkdir(parents=True,exist_ok=False)
    report={'ok':False,'first_error':None,'cleanup_error':None,'cache_comparisons':[]}
    sources=[Path(__file__),args.plan,args.nodes,args.deployments,args.bundle,args.agent_binary]
    report['source_sha256']={str(p.resolve()):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    client=pipeline=reference=None
    try:
        raw=json.loads(args.plan.read_text(encoding='utf-8'));plan=parse_plan(raw)
        nodes=json.loads(args.nodes.read_text(encoding='utf-8'))
        deployments=json.loads(args.deployments.read_text(encoding='utf-8'))
        directory=Path((ROOT.parents[2]/'.cache/hf/models/checkpoint-path.txt').read_text(encoding='utf-8').strip())
        tokenizer=AutoTokenizer.from_pretrained(directory,local_files_only=True)
        scenario=ROOT/'scenarios/qwen3_5_0_8b/short/scenario.json'
        _,_,requests=read_scenario(scenario,plan,tokenizer)
        client=Client(args.host,args.port)
        pipeline=Pipeline.__new__(Pipeline)
        pipeline.__init__(client,raw,directory,sys.executable,args.bundle,nodes,1,deployments)
        report['fault_nodes']=pipeline.reports
        assert len({n['host'] for n in pipeline.reports})==2
        # We receive only the size prefix. Neither the event nor its model result is accepted.
        request=requests[0]
        job=pipeline.job('step',request.name,0,0)
        tokens=torch.tensor([request.tokens],dtype=torch.int64)
        event_id=client.send(pipeline.target(0),COMMAND,pack({'job':job,'receipts':[]},save({'tensor':tokens})),'hf-transformers')
        size,=struct.unpack('<I',client.exact(4))
        assert size>4
        old_outer,old_sequence=client.outer,client.sequence
        report['disconnected']={'correlation':event_id,'announced_reply_bytes':size,'received_prefix_bytes':4,'accepted_model_results':0}
        client.close()
        client=Reconnected(args.host,args.port);client.outer=old_outer;client.sequence=old_sequence
        client.abandoned=event_id;client.late=[];pipeline.client=client
        # Explicit abort, never replay the unacknowledged step.
        pipeline.close();report['aborted_and_deleted']=True;report['fault_trace']=pipeline.trace
        for node in nodes:node['generation']+=1
        pipeline=Pipeline.__new__(Pipeline)
        pipeline.__init__(client,raw,directory,sys.executable,args.bundle,nodes,2,deployments)
        report['recovery_nodes']=pipeline.reports
        stale={**job,'generation':1}
        reply,_=pipeline.command(0,{'job':stale,'receipts':[]},require=False)
        assert reply.get('ok') is False
        report['old_load_rejected']=reply
        reference=Reference(directory,plan.dtype,plan.nodes[0].device)
        def caches(request,cache):
            for index,node in enumerate(plan.nodes):
                report['cache_comparisons'].append(compare(pipeline.cache(index,request),cache,node.start,node.end))
        _,schedule,requests=read_scenario(scenario,plan,tokenizer)
        report['requests']=drive(pipeline,requests,schedule,tokenizer,reference,caches)
        expected={'arithmetic':'4','korean':'서울','context':'BLUE'}
        assert all(r['released'] and r['terminal']=='eos' and r['text'].strip()==expected[r['id']] for r in report['requests'])
        pipeline.shutdown();report['ok']=True
    except Exception as error:
        report['first_error']=f'{type(error).__name__}: {error}';traceback.print_exc()
    finally:
        if pipeline:
            report['trace']=getattr(pipeline,'trace',[])
            try:pipeline.close()
            except Exception as error:report['cleanup_error']=str(error);report['ok']=False
        if reference:report['comparisons']=reference.comparisons
        if client:
            report['transport']=client.trace;report['late_results']=getattr(client,'late',[]);client.close()
        report['agent_sha256']=hashlib.sha256(args.agent_binary.read_bytes()).hexdigest()
        report['source_unchanged']=all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==h for p,h in report['source_sha256'].items())
        report['ok']=report['ok'] and report['source_unchanged']
        (args.output/'summary.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
        print(json.dumps({k:v for k,v in report.items() if k in ('ok','first_error','cleanup_error','disconnected','aborted_and_deleted')},ensure_ascii=False))
    return 0 if report['ok'] else 1

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--host',default='192.168.0.6');p.add_argument('--port',type=int,default=51056)
    for name in ('plan','nodes','deployments','bundle','output','agent-binary'):p.add_argument('--'+name,type=Path,required=True)
    raise SystemExit(main(p.parse_args()))
