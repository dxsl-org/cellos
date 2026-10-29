#!/usr/bin/env python3
"""Complete physical-DMA/event-order proof from the already-exercised controlled boot."""
import json,re,struct
from pathlib import Path
W=Path('/tmp/cellbox-s10-3d-resume');run=W/'controlled/probe-ordered'
serial=(run/'serial.log').read_text(errors='replace')
sq=(run/'admin-sq.bin').read_bytes();cq=(run/'admin-cq.bin').read_bytes();identify=(run/'identify.bin').read_bytes()
scan=serial.find('[platform] ECAM scan complete');cap=serial.find('[nvme] CAP=');dma=serial.find('granted DMA BDF=')
address=int(re.search(r'identify=0x([0-9a-f]+)',serial)[1],16)
cqe=struct.unpack_from('<IIHHHH',cq)
result={'diagnostic_only':True,'scan_complete_before_cap':0<=scan<cap,'scan_complete_before_dma':0<=scan<dma,
        'sq_cdw0':struct.unpack_from('<I',sq)[0],'sq_prp1':struct.unpack_from('<Q',sq,24)[0],
        'sq_cns':struct.unpack_from('<I',sq,40)[0],'expected_identify_address':address,
        'first_cqe':cqe,'identify_nonzero_bytes':sum(x!=0 for x in identify)}
result['pass']=(result['scan_complete_before_cap'] and result['scan_complete_before_dma']
    and result['sq_cdw0']==0x10006 and result['sq_prp1']==address and result['sq_cns']==1
    and cqe[4:]==(1,1) and any(identify))
(W/'controlled/dma-order-proof.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
raise SystemExit(0 if result['pass'] else 1)
