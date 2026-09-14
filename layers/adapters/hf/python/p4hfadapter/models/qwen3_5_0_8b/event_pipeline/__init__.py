"""Model controller transport: each step crosses P4 brokers, never worker pipes."""
import hashlib
import json
from pathlib import Path
from safetensors.torch import save,load
from p4hfadapter.integration.packet import pack,unpack

COMMAND="application/vnd.p4.hf.command-v2"


def canonical(value):
    return json.dumps(value,sort_keys=True,separators=(",", ":"),ensure_ascii=False).encode()


class Pipeline:
    def __init__(self,client,plan,model_dir,python,bundle,nodes,generation=1,deployments=None):
        self.client,self.nodes,self.generation=client,nodes,generation
        self.epoch,self.serial=1,0
        self.reports,self.trace=[],[]
        self.ready=[]
        self.created=[]
        self.last={}
        self.bundle=str(Path(bundle).resolve())
        artifact=json.loads((Path(bundle).parent / "manifests/qwen3_5_0_8b/artifact/identity.json").read_text(encoding="utf-8"))
        if len(nodes)!=len(plan["nodes"]) or (deployments is not None and len(deployments)!=len(nodes)):
            raise ValueError("deployment/topology length mismatch")
        for index,node in enumerate(nodes):
            self.control(node,"create",{"adapter_kind":"hf-transformers","queue_capacity":1,"completion_capacity":1,
                "retained_capacity":2,"retained_bytes":128*1024*1024})
            self.created.append(index)
            deployment=(deployments or [{} for _ in nodes])[index]
            if set(deployment)-{"python","bundle","model_dir"}:
                raise ValueError("unsupported deployment field")
            config={"plan":plan,"model_dir":deployment.get("model_dir",str(Path(model_dir).resolve())),"node_id":plan["nodes"][index]["node_id"]}
            identity={"generation":generation,"index":index,"nodes":nodes,"config_sha256":hashlib.sha256(canonical(config)).hexdigest(),
                "model_revision":artifact["revision"],"model_sha256":artifact["files"]["model.safetensors-00001-of-00001.safetensors"]["sha256"],
                "tokenizer_sha256":artifact["files"]["tokenizer.json"]["sha256"],"plan_sha256":hashlib.sha256(canonical(plan)).hexdigest(),
                "dtype":plan["dtype"],"boundary":"qwen3.5-text-safetensors-v2","operations":["step","release","cancel","epoch","cache","unload"]}
            launch={"python":deployment.get("python",str(python)),"bundle":deployment.get("bundle",self.bundle),"bundle_sha256":hashlib.sha256(Path(bundle).read_bytes()).hexdigest(),
                "config":config,"identity":identity,"frame_bytes":32*1024*1024,"scratch_bytes":128*1024*1024,"stderr_bytes":65536,"timeout_ms":120000}
            reply,_=self.command(index,{"op":"load","generation":generation,"launch":launch,"nodes":nodes,"index":index})
            self.ready.append(index)
            self.reports.append(reply["ready"]["report"])
    def target(self,index):
        node=self.nodes[index]
        return (1,node["agent"],node["node"],node["generation"])
    def control(self,node,kind,extra=None,require=True):
        payload={"node_id":node["node"],"node_generation":node["generation"],**(extra or {})}
        _,raw=self.client.exchange((0,node["agent"]),f"application/vnd.p4.node.{kind}-v3+json",canonical(payload))
        result=json.loads(raw)
        if require and not result["ok"]:
            raise RuntimeError(f"P4 {kind}: {result}")
        return result
    def command(self,index,meta,body=b"",require=True):
        envelope,raw=self.client.exchange(self.target(index),COMMAND,pack(meta,body),"hf-transformers")
        reply,output=unpack(raw)
        if require and reply.get("ok") is not True:
            raise RuntimeError(f"HF command failed: {reply}")
        if reply.get("ok") and "job" in meta:
            expected=self.target(index if meta["job"]["kind"] in ("unload","cache","abort") else len(self.nodes)-1)
            if envelope["source"]!=expected or reply.get("job")!=meta["job"]:
                raise ValueError("tail/identity approval mismatch")
        self.trace.append({"input":meta,"result":reply,"body_bytes":len(output),"source":envelope["source"]})
        return reply,output
    def job(self,kind,request="",issue=0,position=0,epoch=None):
        return {"generation":self.generation,"epoch":self.epoch if epoch is None else epoch,"serial":self.serial+1,
            "kind":kind,"request":request,"issue":issue,"position":position}
    def step(self,request,issue,position,tokens):
        job=self.job("step",request,issue,position)
        reply,body=self.command(0,{"job":job,"receipts":[]},save({"tensor":tokens}))
        if len(reply["receipts"])!=len(self.nodes):
            raise ValueError("incomplete stage settlement")
        for receipt in reply["receipts"]:
            if receipt["report"]["issue"]!=issue+1 or receipt["report"]["position"]!=position+tokens.shape[1]:
                raise ValueError("stage progress mismatch")
        self.serial+=1
        self.last[request]=(issue+1,position+tokens.shape[1])
        return load(body)["tensor"]
    def release(self,request,cancel=False):
        issue,position=self.last[request]
        job=self.job("cancel" if cancel else "release",request,issue,position)
        reply,body=self.command(0,{"job":job,"receipts":[]})
        if body or len(reply["receipts"])!=len(self.nodes):
            raise ValueError("incomplete release chain")
        self.serial+=1
        del self.last[request]
    def advance(self):
        if self.last:
            raise ValueError("controller epoch before all stage releases")
        self.command(0,{"job":self.job("epoch",epoch=self.epoch+1),"receipts":[]})
        self.epoch+=1
        self.serial+=1
    def cancel(self,request):
        self.release(request,cancel=True)
    def cache(self,index,request):
        issue,position=self.last[request]
        _,body=self.command(index,{"job":self.job("cache",request,issue,position),"receipts":[]})
        return body
    def shutdown(self):
        for index in self.ready:
            self.command(index,{"job":self.job("unload"),"receipts":[]})
        self.ready.clear()
        for index in reversed(self.created):
            self.control(self.nodes[index],"delete")
        self.created.clear()
    def close(self):
        errors=[]
        for index in self.ready:
            try:
                job=self.job("abort",epoch=0)
                job["serial"]=0
                self.command(index,{"job":job,"receipts":[]})
            except Exception as error:
                errors.append(str(error))
        for index in reversed(self.created):
            try:
                self.control(self.nodes[index],"delete")
            except Exception as error:
                errors.append(str(error))
        if errors:
            raise RuntimeError("; ".join(errors))
        self.ready.clear()
        self.created.clear()
