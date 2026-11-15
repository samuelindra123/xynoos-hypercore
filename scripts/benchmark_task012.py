"""Eksperimen paired TASK010/TASK012 dengan urutan proses ABBA."""
import csv, io, os, subprocess, tempfile, statistics
from pathlib import Path
root=Path(__file__).resolve().parent.parent
bench=root/'docs'/'benchmarks'
with tempfile.TemporaryDirectory(prefix='xynoos-012-') as temporary:
    base=Path(temporary)/'base';base.mkdir()
    archive=subprocess.Popen(['git','archive','d082f60ade87b30697fa03384ace1a8089f036d3'],cwd=root,stdout=subprocess.PIPE)
    subprocess.run(['tar','-x','-C',str(base)],stdin=archive.stdout,check=True)
    archive.stdout.close()
    assert archive.wait()==0
    (base/'crates/xynoos-aura/examples/typed_research.rs').write_bytes((root/'crates/xynoos-aura/examples/typed_research.rs').read_bytes())
    env=os.environ.copy();env['CARGO_TARGET_DIR']=str(Path(temporary)/'target')
    subprocess.run(['cargo','build','--offline','--manifest-path',str(base/'Cargo.toml'),'-p','xynoos-aura','--example','typed_research','--release'],env=env,check=True,stdout=subprocess.DEVNULL)
    binaries={'baseline':Path(temporary)/'target/release/examples/typed_research','current':root/'target/release/examples/typed_research'}
    rows=[]
    for process,version in enumerate(['baseline','current','current','baseline']):
        with tempfile.TemporaryFile(mode='w+b') as output:
            child=subprocess.Popen([str(binaries[version])],stdout=output)
            _,status,usage=os.wait4(child.pid,0);child.returncode=os.waitstatus_to_exitcode(status)
            if child.returncode!=0:raise RuntimeError(f'{version}: {child.returncode}')
            output.seek(0);data=list(csv.DictReader(io.TextIOWrapper(output,encoding='utf8')))
        assert len(data)==324
        rows.extend(dict(process=process,version=version,rss_peak_kib=usage.ru_maxrss,**r) for r in data)
assert len(rows)==1296
by={(r['version'],r['process']//2 if r['version']=='current' else (0 if r['process']==0 else 1),r['trace'],r['seed'],r['groups'],r['mode'],r['sample']):r for r in rows}
# Kesesuaian simulasi diperiksa terhadap baseline berurutan untuk setiap process.
base={(r['trace'],r['seed'],r['groups'],r['mode'],r['sample']):r for r in rows if r['version']=='baseline' and r['process']==0}
ignore={'version','process','rss_peak_kib','event_ns','inspection_ns','alloc_count','alloc_bytes','dealloc_count','live_count','live_bytes','peak_live_bytes'}
for r in rows:
    b=base[(r['trace'],r['seed'],r['groups'],r['mode'],r['sample'])]
    assert all(str(r[k])==str(b[k]) for k in r if k not in ignore)
with (bench/'task-012-raw.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator='\n');w.writeheader();w.writerows(rows)
fields=['groups','mode','version','samples','event_ns_median','event_ns_min','event_ns_max','inspection_ns_median','alloc_count_median','alloc_bytes_median','peak_live_bytes_median','rss_peak_kib_max']
with (bench/'task-012-summary.csv').open('w',newline='') as f:
    w=csv.DictWriter(f,fieldnames=fields,lineterminator='\n');w.writeheader()
    for n in ['1','16','64']:
        for mode in ['clone','journal','fast']:
            for version in ['baseline','current']:
                group=[r for r in rows if r['groups']==n and r['mode']==mode and r['version']==version]
                def med(k):return statistics.median(int(x[k]) for x in group)
                w.writerow(dict(groups=n,mode=mode,version=version,samples=len(group),event_ns_median=med('event_ns'),event_ns_min=min(int(x['event_ns']) for x in group),event_ns_max=max(int(x['event_ns']) for x in group),inspection_ns_median=med('inspection_ns'),alloc_count_median=med('alloc_count'),alloc_bytes_median=med('alloc_bytes'),peak_live_bytes_median=med('peak_live_bytes'),rss_peak_kib_max=max(int(x['rss_peak_kib']) for x in group)))
(bench/'task-012-reproducibility.txt').write_text('1296 record valid; seluruh metrik simulasi identik dengan TASK010 untuk setiap workload/seed/ukuran/mode/sample. Dua subprocess per versi, urutan ABBA. Timing, allocator dan RSS tidak disyaratkan identik.\n')
