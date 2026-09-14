"""Run real Qwen through an already running P4 agent, with official model/cache parity."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import time
import traceback
import subprocess

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT / "python"))
from p4hfadapter.integration.transport import Client
from p4hfadapter.models.qwen3_5_0_8b.configuration import parse_plan
from p4hfadapter.models.qwen3_5_0_8b.event_pipeline import Pipeline
from p4hfadapter.models.qwen3_5_0_8b.scenarios import read_scenario
from p4hfadapter.models.qwen3_5_0_8b.scheduling import drive
from p4hfadapter.models.qwen3_5_0_8b.reference import Reference
from p4hfadapter.models.qwen3_5_0_8b.cache_export import compare
from p4hfadapter.models.qwen3_5_0_8b.loading import runtime_check
from transformers import AutoTokenizer


def run(args):
    args.output.mkdir(parents=True,exist_ok=False)
    report={"ok":False,"first_error":None,"cleanup_error":None,"requests":[],"cache_comparisons":[],"scope":"P4 event integration"}
    source={p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()
            for folder in ("python","adapter","scripts") for p in (ROOT / folder).rglob("*")
            if p.is_file() and p.suffix in (".py",".rs",".toml")}
    report["source"]=source
    report["p4_head"]=subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip()
    if args.agent_binary:
        report["agent_sha256"]=hashlib.sha256(args.agent_binary.read_bytes()).hexdigest()
    client=pipeline=reference=None
    try:
        runtime_check()
        plan_raw=json.loads(args.plan.read_text(encoding="utf-8"))
        plan=parse_plan(plan_raw)
        directory=Path(args.model_dir or (ROOT.parents[2] / ".cache/hf/models/checkpoint-path.txt").read_text(encoding="utf-8").strip())
        report["plan"]=plan_raw
        report["scenario"]=json.loads(args.scenario.read_text(encoding="utf-8"))
        report["bundle_sha256"]=hashlib.sha256(args.bundle.read_bytes()).hexdigest()
        client=Client(args.host,args.port)
        _,inspection=client.exchange((0,client.address),"application/vnd.p4.agent.inspect-v1+json",b"{}")
        report["inspect"]=json.loads(inspection)
        nodes=[{"agent":client.address,"node":f"hf-{client.outer[2][:8]}-{i}","generation":1} for i in range(len(plan.nodes))]
        if args.nodes:
            nodes=json.loads(args.nodes.read_text(encoding="utf-8"))
        pipeline=Pipeline.__new__(Pipeline)
        deployments=json.loads(args.deployments.read_text(encoding="utf-8")) if args.deployments else None
        pipeline.__init__(client,plan_raw,directory,args.python,args.bundle,nodes,args.generation,deployments)
        report["nodes"]=pipeline.reports
        tokenizer=AutoTokenizer.from_pretrained(directory,local_files_only=True)
        reference=Reference(directory,plan.dtype,plan.nodes[0].device)
        def caches(request,cache):
            for index,node in enumerate(plan.nodes):
                result=compare(pipeline.cache(index,request),cache,node.start,node.end)
                report["cache_comparisons"].append({"request":request,"node":index,**result})
        start=time.monotonic()
        for block in range(args.blocks):
            _,schedule,requests=read_scenario(args.scenario,plan,tokenizer)
            if args.blocks>1:
                for request in requests:
                    request.session_id="reused-"+request.name
            report["requests"].extend(drive(pipeline,requests,schedule,tokenizer,reference,caches))
            if block+1<args.blocks:
                old_epoch=pipeline.epoch
                pipeline.advance()
                for kind in ("step","release","cancel"):
                    stale=pipeline.job(kind,"stale",epoch=old_epoch)
                    rejection,_=pipeline.command(0,{"job":stale,"receipts":[]},require=False)
                    if rejection.get("ok") is not False:
                        raise AssertionError("stale epoch accepted")
        report["elapsed_seconds"]=time.monotonic()-start
        pipeline.shutdown()
        report["ok"]=True
    except Exception as error:
        report["first_error"]=f"{type(error).__name__}: {error}"
        traceback.print_exc()
    finally:
        if pipeline:
            report["trace"]=getattr(pipeline,"trace",[])
            report["nodes"]=getattr(pipeline,"reports",[])
            try:
                pipeline.close()
            except Exception as error:
                report["cleanup_error"]=str(error)
                report["ok"]=False
        if reference:
            report["comparisons"]=reference.comparisons
        if client:
            if report["cleanup_error"] is None:
                try:
                    client.finish()
                    report["connection_finished"]=True
                except Exception as error:
                    report["cleanup_error"]=f"connection finish: {error}"
                    report["ok"]=False
            report["transport"]={"sent_bytes":client.sent_bytes,"received_bytes":client.received_bytes,"trace":client.trace}
            client.close()
        report["source_unchanged"]=all(hashlib.sha256((ROOT / p).read_bytes()).hexdigest()==h for p,h in source.items())
        report["ok"]=report["ok"] and report["source_unchanged"]
        (args.output / "summary.json").write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    print(json.dumps({"ok":report["ok"],"first_error":report["first_error"],"cleanup_error":report["cleanup_error"],
        "comparisons":len(report.get("comparisons",[])),"cache_comparisons":len(report["cache_comparisons"])}))
    return 0 if report["ok"] else 1


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--host",default="127.0.0.1")
    parser.add_argument("--port",type=int,required=True)
    parser.add_argument("--plan",type=Path,required=True)
    parser.add_argument("--scenario",type=Path,required=True)
    parser.add_argument("--bundle",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--python",default=sys.executable)
    parser.add_argument("--model-dir")
    parser.add_argument("--nodes",type=Path)
    parser.add_argument("--deployments",type=Path)
    parser.add_argument("--agent-binary",type=Path)
    parser.add_argument("--generation",type=int,default=1)
    parser.add_argument("--blocks",type=int,default=1)
    raise SystemExit(run(parser.parse_args()))
