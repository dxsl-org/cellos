from pathlib import Path
import hashlib,json,shutil
R=Path('/home/dmin/cellos');D=R/'.agents/261020-1750-cellbox-cli/reports/s10-4-evidence/runtime/buffered-contract';D.mkdir(parents=True,exist_ok=True)
files={'cells/tools/cellbox/src/executor.rs':[(1875,1895),(1919,1922)],'cells/tools/cellbox/src/typed.rs':[(915,931),(959,1001)],'scripts/phase10_canonical_drive.py':[(815,825),(838,847)],'.agents/261020-1750-cellbox-cli/phase-10-s0-proposal.md':[(1487,1499),(1523,1526)]}
rows=[]
for name,ranges in files.items():
    p=R/name;raw=p.read_bytes();lines=raw.decode().splitlines();rows.append({'path':name,'sha256':hashlib.sha256(raw).hexdigest(),'ranges':[{'first':a,'last':b,'lines':lines[a-1:b]} for a,b in ranges]})
prior=R/'.agents/261020-1750-cellbox-cli/reports/s10-3d-evidence/rerun/lanes/L0-headless-run1/verdict.json';raw=prior.read_bytes();verdict=json.loads(raw);marker=verdict['markers']['P10C-ctrl-c']
assert marker['pass'] and marker['checks']['cat-sink-ctrl-c-130']['pass'] and marker['checks']['endless-sink-flowing']['pass']
assert 'output before Ctrl-C: 0 rows' in marker['notes'][0]
shutil.copy2(prior,D/'qualified-L0-headless-run1-verdict.json')
result={'classification':'observer assumption error, no demonstrated product hang','existing_approved_behavior':'Collected byte sinks buffer and Ctrl-C discards collected bytes/status130; no-cat terminal pipeline streams live.','source_excerpts':rows,'qualified_L0':{'source':str(prior.relative_to(R)),'sha256':hashlib.sha256(raw).hexdigest(),'marker':marker},'disposition':'Main2026-10-28 directs contract-correct finite2frame/status0,10s bufferedcat/noJSON/noearlyprompt/CtrlC130/4squiescence and separate no-cat live>=3rows10s/CtrlC130/4squiescence, no product or acceptance change.'}
(D/'contract-evidence.json').write_text(json.dumps(result,indent=2)+'\n');shutil.copy2(Path(__file__),D/Path(__file__).name)
print(json.dumps({'status':'EXISTING_BUFFERED_CONTRACT_CONFIRMED','qualified_cat_rows_before_CtrlC':0,'qualified_cat_prompt_latency':marker['checks']['cat-sink-ctrl-c-prompt']['detail'],'qualified_cat_130':True,'qualified_separate_no_cat_live':True}))
