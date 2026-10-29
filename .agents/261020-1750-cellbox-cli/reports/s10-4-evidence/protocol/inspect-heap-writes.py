#!/usr/bin/env python3
"""Find actual heap-counter instruction sites in unchanged ELFs; static only."""
import hashlib,json,re,subprocess
from pathlib import Path
W=Path('/tmp/cellbox-s10-4');O=W/'observability';inventory=json.loads((O/'inventory.json').read_text());result={}
for label in ['b1-shell','b2-shell-unsigned','candidate-headless-cellbox','candidate-desktop-cellbox']:
    path=Path(inventory[label]['path'])
    text=subprocess.run(['objdump','-d','-C','--no-show-raw-insn',str(path)],capture_output=True,text=True,check=True).stdout
    (O/f'{label}.disassembly.txt').write_text(text)
    instructions=[];func=''
    for line in text.splitlines():
        if re.match(r'^[0-9a-f]+ <.*>:$',line):func=line
        m=re.match(r'^\s*([0-9a-f]+):\s+(\S+)(.*)$',line)
        if m:instructions.append({'pc':int(m[1],16),'instruction':m[2]+m[3],'function':func})
    refs=[]
    for i,row in enumerate(instructions):
        if re.search(r'<ostd::heap::HEAP>$',row['instruction']):
            ref=dict(row);ref['next_pc']=instructions[i+1]['pc'] if i+1<len(instructions) else None;refs.append(ref)
    result[label]={'sha256':hashlib.file_digest(path.open('rb'),'sha256').hexdigest(),'heap_counter_exact_symbol_references':refs,'counter_layout':'Source used field offset0 corroborated by actual global add/sub writes; not inferred from Rust repr layout. Every reference listed for review, not an executed watchpoint proof.'}
(O/'heap-instructions.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
