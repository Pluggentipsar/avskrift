"""Measure peak resident memory and wall time of an explicitly supplied local command."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import psutil

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("command",nargs=argparse.REMAINDER)
    a=p.parse_args()
    cmd=a.command[1:] if a.command[:1]==["--"] else a.command
    if not cmd: p.error("Command required after --")
    start=time.perf_counter()
    child=subprocess.Popen(cmd)
    process=psutil.Process(child.pid)
    peak=0
    while child.poll() is None:
        try:
            memory=process.memory_info()
            peak=max(peak,memory.rss,getattr(memory,"peak_wset",0))
        except psutil.NoSuchProcess:
            break
        time.sleep(0.1)
    code=child.wait()
    report={"exit_code":code,"peak_resident_bytes":peak,"wall_seconds":time.perf_counter()-start,
            "scope":"child process only; includes model loading and all checks"}
    a.output.write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(json.dumps(report))
    raise SystemExit(code)

if __name__ == "__main__": main()
