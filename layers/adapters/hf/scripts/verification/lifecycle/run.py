"""Failure/recovery through actual agent CREATE, retained nodes and DELETE."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT / "python"))
from p4hfadapter.integration.transport import Client
from p4hfadapter.integration.packet import pack,unpack
COMMAND="application/vnd.p4.hf.command-v2"

def main(args):
 args.output.mkdir(parents=True,exist_ok=False)
 client=Client(args.host,args.port)
 results=[]
 def control(node,op):
  value={"node_id":node,"node_generation":1}
  if op=="create":value.update(adapter_kind="hf-transformers",queue_capacity=1,completion_capacity=1,retained_capacity=2,retained_bytes=32768)
  _,body=client.exchange((0,client.address),f"application/vnd.p4.node.{op}-v3+json",json.dumps(value).encode())
  return json.loads(body)
 for mode in ("normal","ready_mismatch","death","partial","magic","version","reserved","length","identity","hang","incompatible","hash_mismatch"):
  node=f"fixture-{client.outer[2][:8]}-{mode}"
  folder=args.output / mode;folder.mkdir()
  worker=(ROOT / "tests/fixtures/bridge_worker/main.py").read_bytes();(folder / "entry.py").write_bytes(worker)
  manifest={"protocol":3 if mode=="incompatible" else 2,"entry":"entry.py","files":{"entry.py":hashlib.sha256(worker).hexdigest()}}
  bundle=folder / "bundle.json";bundle.write_text(json.dumps(manifest),encoding="utf-8")
  topology=[{"agent":client.address,"node":node,"generation":1}]
  def command(meta):
   _,body=client.exchange((1,client.address,node,1),COMMAND,pack(meta),"hf-transformers")
   return unpack(body)[0]
  def job(kind,serial=1,epoch=1):return {"job":{"generation":1,"epoch":epoch,"serial":serial,"kind":kind,"request":"A","issue":0,"position":0},"receipts":[]}
  record={"mode":mode,"replies":[]};results.append(record)
  created=loaded=False
  try:
   assert control(node,"create")["ok"];created=True
   launch={"python":args.python,"bundle":str(bundle.resolve()),"bundle_sha256":"00"*32 if mode=="hash_mismatch" else hashlib.sha256(bundle.read_bytes()).hexdigest(),
    "config":{"mode":mode},"identity":{"generation":1,"index":0,"nodes":topology},"frame_bytes":2048,"scratch_bytes":8192,"stderr_bytes":1024,"timeout_ms":500}
   reply=command({"op":"load","generation":1,"nodes":topology,"index":0,"launch":launch});record["replies"].append(reply)
   if mode in ("ready_mismatch","incompatible","hash_mismatch"):
    assert reply["ok"] is False
   else:
    assert reply["ok"];loaded=True
    assert control(node,"delete")["ok"] is False
    reply=command(job("step"));record["replies"].append(reply)
    if mode=="normal":
     assert reply["ok"]
     assert command(job("unload",2))["ok"] is False
     assert command(job("cancel",2))["ok"]
     assert command(job("epoch",3,2))["ok"]
     assert command(job("release",4,1))["ok"] is False
     assert command(job("step",4,2))["ok"]
     assert command(job("release",5,2))["ok"]
     assert command(job("unload",6,2))["ok"];loaded=False
    else:
     assert reply["ok"] is False and reply["uncertain"]
     assert command(job("step",2))["ok"] is False
   record["ok"]=True
  except Exception as error:
   record["ok"]=False;record["first_error"]=str(error)
  finally:
   if loaded:
    abort=job("abort",0,0);abort["job"]["request"]=""
    try:assert command(abort)["ok"]
    except Exception as error:record["cleanup_error"]=str(error);record["ok"]=False
   if created:
    try:assert control(node,"delete")["ok"]
    except Exception as error:record["cleanup_error"]=str(error);record["ok"]=False
   (args.output / "summary.json").write_text(json.dumps({"cases":results,"trace":client.trace},indent=2)+"\n",encoding="utf-8")
  print(json.dumps(record),flush=True)
 client.close()
 return 0 if all(r["ok"] for r in results) else 1
if __name__=="__main__":
 p=argparse.ArgumentParser();p.add_argument("--host",default="192.168.0.6");p.add_argument("--port",type=int,default=41980)
 p.add_argument("--python",default=sys.executable);p.add_argument("--output",type=Path,required=True)
 raise SystemExit(main(p.parse_args()))
