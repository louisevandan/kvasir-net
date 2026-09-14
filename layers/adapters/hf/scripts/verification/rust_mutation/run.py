"""Compile isolated source copies and require each removed guard to fail its consumer test."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import zipfile

ROOT=Path(__file__).resolve().parents[3]
CASES={
 "baseline":None,
 "epoch":("lifecycle/mod.rs","job.epoch != self.epoch + u64::from(epoch_change)","false"),
 "result_identity":("lifecycle/mod.rs",'reply["job"] != meta["job"] || ',""),
 "input_bound":("retained/mod.rs","if cost > self.limit {","if false {"),
 "held_barrier":("lifecycle/mod.rs","mailbox.storage_snapshot().retained_count > 1","false"),
}
if os.name=="nt":
 CASES["owned_tree"]=("process/windows/mod.rs", 'AssignProcessToJobObject(\n                self.raw(),\n                child.raw_handle().ok_or("missing child handle")?,\n            ) == 0', "false")
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main(output):
 output.mkdir(parents=True,exist_ok=False)
 archive=output / "p4.zip"
 subprocess.run(["git","archive","--format=zip",f"--output={archive}","HEAD"],cwd=ROOT.parents[2],check=True)
 records=[]
 for name,change in CASES.items():
  case=output / name
  with zipfile.ZipFile(archive) as z:z.extractall(case / "p4")
  source=case / "p4/layers/adapters/hf"
  if change:
   path=source / "adapter/src" / change[0]
   text=path.read_text(encoding="utf-8")
   if text.count(change[1])!=1:raise RuntimeError(f"mutation drift {name}")
   before=sha(path);path.write_text(text.replace(change[1],change[2]),encoding="utf-8",newline="\n")
  env=dict(os.environ,CARGO_TARGET_DIR=str(case / "target"),HF_TEST_PYTHON=os.environ.get("HF_TEST_PYTHON","python"))
  cmd=["cargo","test","--locked","-p","p4-hf-adapter","--test","retained","--","--test-threads=1"]
  print(f"START {name}",flush=True)
  with (case / "test.log").open("wb") as log:
   result=subprocess.run(cmd,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=300)
  log=(case / "test.log").read_text(encoding="utf-8",errors="replace")
  binaries=list((case / "target/debug/deps").glob("retained-*.exe"))
  ok=(result.returncode==0 if change is None else result.returncode==101 and "test result: FAILED" in log)
  ok=ok and "Compiling p4-hf-adapter" in log and len(binaries)==1
  record={"case":name,"ok":ok,"exit":result.returncode,"command":cmd,"target":env["CARGO_TARGET_DIR"],
    "binary_sha256":sha(binaries[0]) if binaries else None}
  if change:record["mutation"]={"path":change[0],"before":before,"after":sha(path)}
  records.append(record)
  (output / "summary.json").write_text(json.dumps(records,indent=2)+"\n",encoding="utf-8")
  print(json.dumps(record),flush=True)
 return 0 if all(r["ok"] for r in records) else 1
if __name__=="__main__":
 p=argparse.ArgumentParser();p.add_argument("output",type=Path)
 raise SystemExit(main(p.parse_args().output.resolve()))
