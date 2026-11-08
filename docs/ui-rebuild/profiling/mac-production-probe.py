import subprocess,time,json
from pathlib import Path
import argparse
parser=argparse.ArgumentParser(description='Sample three fresh-process production launches; run independently of desktop tests.')
parser.add_argument('binary',type=Path);parser.add_argument('--output',type=Path,default=Path('/private/tmp/kokin-production-probe.json'))
args=parser.parse_args();binary=str(args.binary.resolve())
def processes():
 rows={}
 for line in subprocess.run(['ps','-axo','pid,ppid,rss,command'],capture_output=True,text=True).stdout.splitlines()[1:]:
  cols=line.strip().split(None,3)
  if len(cols)==4:rows[int(cols[0])]={'pid':int(cols[0]),'ppid':int(cols[1]),'rss_kib':int(cols[2]),'command':cols[3]}
 return rows
runs=[]
for number in range(3):
 baseline=set(processes());start=time.monotonic();p=subprocess.Popen([binary],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);samples=[];first_webkit=None
 try:
  while time.monotonic()-start<10:
   rows=processes();new=[r for pid,r in rows.items() if pid not in baseline and '/System/' in r['command'] and 'com.apple.WebKit.' in r['command']]
   if first_webkit is None and new:first_webkit=round((time.monotonic()-start)*1000,1)
   own=rows.get(p.pid)
   samples.append({'seconds':round(time.monotonic()-start,3),'host_rss_kib':own['rss_kib'] if own else None,'new_webkit':new})
   time.sleep(.1)
  runs.append({'run':number+1,'host_pid':p.pid,'first_new_webkit_ms':first_webkit,'alive_after_10_seconds':p.poll() is None,'samples':samples})
 finally:
  p.terminate()
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:p.kill();p.wait()
 # Do not terminate unrelated/orphan WebKit processes; observe their lifetime in the next baseline.
args.output.write_text(json.dumps({'method':'Three no-driver release-binary launches, 10 seconds each. Fresh process, warm filesystem caches. Mac locked; first WebKit process is a startup proxy, not visible readiness. New WebKit subprocesses attributed by PID difference on this otherwise idle sample, not guaranteed parent ownership.','runs':runs},indent=2))
for run in runs:
 last=run['samples'][-1];print(json.dumps({'run':run['run'],'first_webkit_ms':run['first_new_webkit_ms'],'alive':run['alive_after_10_seconds'],'host_rss_kib':last['host_rss_kib'],'new_webkit_rss_kib':sum(r['rss_kib'] for r in last['new_webkit'])}))
