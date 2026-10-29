#!/usr/bin/env python3
"""Static ELF/symbol inspection only. No VM, debugger attach, trace or signing."""
import hashlib,json,re,subprocess
from pathlib import Path
W=Path('/tmp/cellbox-s10-4');R=Path('/home/dmin/cellos');O=W/'observability';O.mkdir(exist_ok=True)
paths={'b1-shell':W/'static/b1/shell','b1-ps':W/'static/b1/ps','b2-shell-unsigned':W/'b2-unsigned/shell','b2-ps-unsigned':W/'b2-unsigned/ps','candidate-headless-cellbox':W/'static/headless/cellbox','candidate-desktop-cellbox':W/'static/desktop/cellbox','candidate-headless-kernel':Path('/tmp/cellbox-s10-3d-resume/cand/headless.kernel.elf'),'candidate-desktop-kernel':Path('/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf')}
pattern=re.compile(r'HEAP|Heap::|heap::|CellAllocator|CUSTOM_HEAP|SCHEDULER|cell_quota.*IN_USE|stack.*watermark|STACK_BASELINE|global.*alloc|__rust_alloc|__rust_dealloc|__rust_realloc|__rg_alloc|__rg_dealloc|__rg_realloc|stack.*Stack')
result={}
for name,path in paths.items():
    nm=subprocess.run(['nm','-anSC',str(path)],capture_output=True,text=True,check=True)
    sections=subprocess.run(['readelf','-SW',str(path)],capture_output=True,text=True,check=True).stdout
    segments=subprocess.run(['readelf','-lW',str(path)],capture_output=True,text=True,check=True).stdout
    (O/f'{name}.nm.txt').write_text(nm.stdout);(O/f'{name}.sections.txt').write_text(sections);(O/f'{name}.segments.txt').write_text(segments)
    result[name]={'path':str(path),'sha256':hashlib.file_digest(path.open('rb'),'sha256').hexdigest(),'symbols':[s for s in nm.stdout.splitlines() if pattern.search(s)],'debug_sections':[s.strip() for s in sections.splitlines() if '.debug' in s or '.zdebug' in s],'symtab_present':'.symtab' in sections,'nm_stderr':nm.stderr}
(O/'inventory.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({n:{'symtab_present':v['symtab_present'],'debug_sections':v['debug_sections'],'symbols':v['symbols']} for n,v in result.items()},indent=2))
