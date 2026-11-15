"""Pengamatan live pendek dan benchmark parser fixture; tidak memodifikasi procfs."""
import csv,io,os,statistics,subprocess,tempfile,time
from pathlib import Path
root=Path(__file__).resolve().parent.parent
bench=root/'docs/benchmarks'
def run(args):
 with tempfile.TemporaryFile() as out:
  start=time.monotonic_ns();p=subprocess.Popen(args,stdout=out);_,status,usage=os.wait4(p.pid,0);p.returncode=os.waitstatus_to_exitcode(status);wall=time.monotonic_ns()-start
  if p.returncode:raise RuntimeError(f'proses gagal: {p.returncode}')
  out.seek(0);data=out.read().decode()
 return data,usage,wall
live,usage,wall=run([str(root/'target/release/xynoos-cli'),'pressure','sample','--count','5','--interval-ms','200'])
rows=list(csv.DictReader(io.StringIO(live)));assert len(rows)==5 and all(len(r)==21 and None not in r for r in rows)
assert all(r['psi_status']=='ok' and r['mem_status']=='ok' and r['vm_status']=='ok' for r in rows)
(bench/'task-013-raw.csv').write_text(live)
a,_,_=run([str(root/'target/release/examples/pressureiq_research')]);b,_,_=run([str(root/'target/release/examples/pressureiq_research')]);fixtures=list(csv.DictReader(io.StringIO(a)));second=list(csv.DictReader(io.StringIO(b)))
assert [(r['case'],r['iterations']) for r in fixtures]==[(r['case'],r['iterations']) for r in second] and len(fixtures)==3
(bench/'task-013-fixture.csv').write_text(a)
summary=[]
for key in ['read_psi_ns','parse_psi_ns','read_mem_ns','parse_mem_ns','read_vm_ns','parse_vm_ns','snapshot_ns']:
 x=[int(r[key]) for r in rows];summary.append((key,statistics.median(x),'ns',len(x),'live'))
for r in fixtures:summary.append(('fixture_'+r['case'],r['ns_per_parse'],'ns_per_parse',r['iterations'],'syntetis'))
summary.extend([('cpu_total',(usage.ru_utime+usage.ru_stime)*1e9,'ns',5,'seluruh_proses'),('peak_rss',usage.ru_maxrss,'KiB',5,'seluruh_proses'),('wall_total',wall,'ns',5,'seluruh_proses'),('csv_size',len(live.encode()),'byte',5,'live')])
with (bench/'task-013-summary.csv').open('w',newline='') as f:
 w=csv.writer(f,lineterminator='\n');w.writerow(['metric','value','unit','samples_or_iterations','scope']);w.writerows(summary)
(bench/'task-013-reproducibility.txt').write_text('Dua eksekusi parser fixture memiliki kasus dan jumlah iterasi identik. Waktu dan telemetry live berubah; output live 5 baris tervalidasi dengan 21 kolom. Seluruh sumber tersedia pada pengamatan ini.\n')
print('5 sampel live; 3 fixture; validasi CSV lulus')
