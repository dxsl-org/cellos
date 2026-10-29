"""Host-only all-stop observation of immutable candidate code; never writes guest bytes."""
import json, queue, re, struct, threading, time
from pathlib import Path
from rsp import RspClient, RspError, RspTimeout
from common import W, Stop, dump, need
MASK=0x000ffffffffff000

def elf_layout(path):
    b=Path(path).read_bytes();need(b[:6]==b'\x7fELF\x02\x01','expected ELF64LE')
    phoff=struct.unpack_from('<Q',b,32)[0];phent,phnum=struct.unpack_from('<HH',b,54)
    shoff=struct.unpack_from('<Q',b,40)[0];shent,shnum,shstr=struct.unpack_from('<HHH',b,58)
    sections=[struct.unpack_from('<IIQQQQIIQQ',b,shoff+i*shent) for i in range(shnum)]
    st=sections[shstr];names=b[st[4]:st[4]+st[5]];byname={names[s[0]:].split(b'\0',1)[0].decode():s for s in sections}
    loads=[]
    for i in range(phnum):
        kind,flags,off,va,pa,filesz,memsz,align=struct.unpack_from('<IIQQQQQQ',b,phoff+i*phent)
        if kind==1:loads.append({'va':va,'memsz':memsz,'filesz':filesz,'offset':off,'flags':flags})
    return b,byname,loads

def walk_page(va,cr3,read_physical,cache):
    table=cr3&MASK;witness=[]
    for shift in [39,30,21,12]:
        if table not in cache:cache[table]=read_physical(table,4096)
        idx=(va>>shift)&511;entry=struct.unpack_from('<Q',cache[table],idx*8)[0];witness.append({'table_pa':table,'index':idx,'pte':entry,'shift':shift})
        if not entry&1:return None,witness
        if shift in [30,21] and entry&128:
            return (entry&MASK&~((1<<shift)-1))|(va&((1<<shift)-1)),witness
        table=entry&MASK
    return table|(va&4095),witness

class Observer(threading.Thread):
    def __init__(self,m):
        super().__init__(daemon=True);self.m=m;self.profile=m.a.profile;self.phase='boot';self.error=None;self.ready=threading.Event();self.done=threading.Event();self.requests=queue.Queue();self.instances={};self.focus=None;self.entry_ordinal=0
        self.client=RspClient(str(m.gdb_path),str(m.work/'evidence/rsp-traffic.jsonl'),timeout=5)
        self.breaks=set();self.watches=set();self.events=(m.work/'evidence/heap-events.jsonl').open('w');self.coverage=(m.work/'evidence/coverage-events.jsonl').open('w');self.stack_events=[];self.snapshots=[];self.pause_ns=0;self.draw_events=[];self.counters=0
        self.data,self.sections,self.loads=elf_layout(m.inputs['files']['/bin/cellbox']['file'])
        inv=json.loads((W/'observability/heap-instructions.json').read_text())[f'candidate-{self.profile}-cellbox']['heap_counter_exact_symbol_references']
        self.zero_sites={x['pc'] for x in inv if x['instruction'].startswith('movq')};self.zero_next={x['next_pc'] for x in inv if x['instruction'].startswith('movq')}
        self.updates={x['pc']:x['next_pc'] for x in inv if x['instruction'].startswith(('add','sub'))};need(len(self.updates)==2,'unexpected heap arithmetic sites')
        self.posts=set(self.updates.values());self.writer_sites=self.zero_sites|set(self.updates);self.writer_next=self.zero_next|self.posts
        text=self.sections['.text'];self.fingerprints=[]
        reloc=self.sections.get('.rela.dyn');targets=[]
        if reloc:
            for at in range(reloc[4],reloc[4]+reloc[5],24):targets.append(struct.unpack_from('<Q',self.data,at)[0])
        for rel in [0, min(self.updates)-64, max(self.updates)+64]:
            need(0<=rel and rel+48<=text[5],'fingerprint outside text');need(not any(rel<=r<rel+48 for r in targets),'fingerprint includes relocation')
            self.fingerprints.append((rel,self.data[text[4]+rel:text[4]+rel+48]))
        self.draw_rel=0xa0c00 if self.profile=='headless' else 0x9fea0
        self.flush_rel=0xa1040 if self.profile=='headless' else 0xa02e0
    def event(self,f,obj):f.write(json.dumps(obj)+'\n')
    def set_phase(self,name):self.check();self.phase=name
    def check(self):
        if self.error:raise Stop(f'observation blocker: {self.error}')
    def add_bp(self,addr):self.client.add_breakpoint(addr);self.breaks.add(addr)
    def add_wp(self,addr,length):self.client.add_watchpoint(addr,length);self.watches.add((addr,length))
    def remove_wp(self,addr,length):self.client.remove_watchpoint(addr,length);self.watches.remove((addr,length))
    def is_entry(self,pc):return pc>=0x100000000 and (pc-0x100000000)%(32*1024*1024)==0 and (pc-0x100000000)//(32*1024*1024)<512
    def _entry(self,pc):
        try:
            if self.client.read_memory(pc,48)!=self.fingerprints[0][1]:return
            for rel,want in self.fingerprints[1:]:
                if self.client.read_memory(pc+rel,len(want))!=want:return
        except RspError as e:raise Stop(f'entry fingerprint cannot be read: {e}')
        need(pc not in self.instances,'observed slot reuse/restart')
        role='headless' if self.profile=='headless' else ('foreground-terminal' if self.phase=='terminal-launch' else 'autoload-background')
        if role=='headless':need(not self.instances,'unexpected concurrent headless Cellbox')
        elif role=='autoload-background':need(not any(i['role']==role for i in self.instances.values()),'unexpected concurrent background Cellbox')
        else:need(self.focus is None,'unexpected concurrent foreground Cellbox')
        cr3=self.client.read_register('cr3');sp=self.client.read_register('rsp');cs=self.client.read_register('cs');need(cs&3==3,'Cellbox entry not user mode')
        top=(sp+4095)&~4095;bottom=top-65536;cache={};pages={}
        for va in range(bottom,top,4096):
            pa,w=walk_page(va,cr3,self.client.read_physical_memory,cache);need(pa is not None,'user-stack mapping missing');pages[va]=pa&~4095
        for va in [bottom-4096,bottom-8192]:
            pa,w=walk_page(va,cr3,self.client.read_physical_memory,cache);need(pa is None,'stack guard mapping present/extent ambiguous')
        self.entry_ordinal+=1
        inst={'id':self.entry_ordinal,'role':role,'base':pc,'cr3':cr3,'initial_rsp':sp,'stack_top':top,'stack_bottom':bottom,'stack_pages':pages,'touched_pages':[],'heap':pc+0x8a80c0,'peak_aligned_used_bytes':0,'phase_peaks':{},'last_value':None,'pending_writer':None,'coverage_writes':0,'post_events':0,'initial_counter':int.from_bytes(self.client.read_memory(pc+0x8a80c0,8),'little')}
        need(inst['initial_counter']==0,'heap counter unexpectedly nonzero before entry')
        self.instances[pc]=inst
        if role in ['headless','foreground-terminal']:self.focus=pc
        self.add_wp(inst['heap'],8)
        for va in pages:self.add_wp(va,4096)
        for rel in self.posts|{self.draw_rel,self.flush_rel}:self.add_bp(pc+rel)
        dump(self.m.work/f'evidence/instance-{inst["id"]}.json',inst)
    def _counter(self,inst,pc,source):
        value=int.from_bytes(self.client.read_memory(inst['heap'],8),'little');need(0<=value<=6*1024*1024,'allocator-used exceeds arena')
        inst['peak_aligned_used_bytes']=max(inst['peak_aligned_used_bytes'],value);inst['phase_peaks'][self.phase]=max(inst['phase_peaks'].get(self.phase,0),value);inst['last_value']=value
        inst['post_events']+=1;self.counters+=1
        self.event(self.events,{'monotonic_ns':time.monotonic_ns(),'instance':inst['id'],'phase':self.phase,'pc':pc,'source':source,'aligned_used_bytes':value})
    def _handle(self,reply):
        pc=self.client.read_register('rip')
        if self.is_entry(pc):self._entry(pc);return
        watch=re.search(rb'(?:a|r)?watch:([0-9a-fA-F]+);',reply)
        if watch:
            addr=int(watch[1],16)
            for inst in self.instances.values():
                rel=pc-inst['base']
                if addr==inst['heap']:
                    need(rel in self.writer_sites|self.writer_next,'unknown heap writer or watch stop semantics')
                    self.event(self.coverage,{'monotonic_ns':time.monotonic_ns(),'instance':inst['id'],'phase':self.phase,'pc':pc,'rel':rel,'watch':addr})
                    inst['coverage_writes']+=1
                    if rel in self.posts:
                        self._counter(inst,pc,'watch-post');inst['pending_writer']=None;inst['watch_completed_post']=pc
                    elif rel in self.updates:inst['pending_writer']=rel
                    else:
                        init=self.client.read_memory(inst['base']+0x9a80f0,1)
                        need(init==b'\x00' or (rel in self.zero_next and inst['post_events']==0),'unexpected initialized-heap reset')
                    return
                if addr in inst['stack_pages']:
                    cr3=self.client.read_register('cr3');pa,w=walk_page(addr,cr3,self.client.read_physical_memory,{})
                    need(pa is not None and pa&~4095==inst['stack_pages'][addr],'stack watch ownership/translation changed')
                    inst['touched_pages'].append(addr);self.stack_events.append({'monotonic_ns':time.monotonic_ns(),'instance':inst['id'],'phase':self.phase,'page_va':addr,'page_pa':pa&~4095,'pc':pc,'cr3':cr3,'witness':w})
                    self.remove_wp(addr,4096);return
            raise Stop('unattributed watchpoint')
        for inst in self.instances.values():
            rel=pc-inst['base']
            if rel in self.posts:
                if inst.pop('watch_completed_post',None)==pc:return
                need(inst['pending_writer'] is not None,'heap post-update without write-watch coverage')
                self._counter(inst,pc,'execute-post');inst['pending_writer']=None;return
            if rel in [self.draw_rel,self.flush_rel]:
                self.draw_events.append({'instance':inst['id'],'phase':self.phase,'pc':pc,'kind':'try_draw' if rel==self.draw_rel else 'flush','monotonic_ns':time.monotonic_ns()});return
        raise Stop(f'unattributed execute stop pc={pc:#x}, reply={reply!r}')
    def _snapshot(self,name):
        need(self.focus is not None,'no verified foreground instance for snapshot');inst=self.instances[self.focus];cr3=inst['cr3'];cache={};frames=set();witness=[];ranges=[]
        for s in self.loads:
            ranges.extend(range((inst['base']+s['va'])&~4095,(inst['base']+s['va']+s['memsz']+4095)&~4095,4096))
        ranges.extend(inst['stack_pages'])
        for va in sorted(set(ranges)):
            pa,pte=walk_page(va,cr3,self.client.read_physical_memory,cache);need(pa is not None,'snapshot backing mapping missing');frames.add(pa&~4095);witness.append({'va':va,'pa':pa&~4095,'pte_chain':pte})
        value=int.from_bytes(self.client.read_memory(inst['heap'],8),'little')
        snap={'phase':name,'instance':inst['id'],'monotonic_ns':time.monotonic_ns(),'cr3':cr3,'aligned_live_used_bytes':value,'mapped_segment_plus_user_stack_backing_bytes':len(frames)*4096,'distinct_frames':sorted(frames),'witness':witness,'scope':'guest present mapped backing, NOT RSS/working set/whole-cell/kernel stack/quota'}
        dump(self.m.work/f'evidence/mapping-{len(self.snapshots):02d}-{name}.json',snap);self.snapshots.append({k:v for k,v in snap.items() if k not in ['witness','distinct_frames']})
    def _pending(self):
        while True:
            try:request=self.requests.get_nowait()
            except queue.Empty:return
            name,event,result=request
            try:self._snapshot(name)
            except Exception as e:result['error']=str(e);raise
            finally:event.set()
    def snapshot(self,name):
        self.check();event=threading.Event();result={};self.requests.put((name,event,result))
        while not event.wait(.1):self.check()
        self.check();need('error' not in result,result.get('error'))
    def _stopped(self,reply):
        while True:
            if not (reply.startswith(b'T02') or reply.startswith(b'S02')):self._handle(reply)
            self._pending()
            if self.done.is_set():return
            pc=self.client.read_register('rip')
            if pc not in self.breaks:return
            reply=self.client.step_over_breakpoint(pc)
            need(reply.startswith((b'T05',b'S05')),'unexpected signal during native hardware-breakpoint step-over')
            # A watched native write or a newly reached enrolled execute site
            # is processed before any further continuation; guards stay live.
            if re.search(rb'(?:a|r)?watch:',reply) or self.client.read_register('rip') in self.breaks:continue
            return
    def run(self):
        try:
            self.client.connect()
            for slot in range(512):self.add_bp(0x100000000+slot*32*1024*1024)
            self.client.resume();self.ready.set()
            while not self.done.is_set():
                try:reply=self.client.wait_stop(timeout=.1)
                except RspTimeout:
                    if not self.requests.empty() or self.done.is_set():reply=self.client.interrupt()
                    else:continue
                stop_ns=time.monotonic_ns()
                self._stopped(reply)
                self.pause_ns+=time.monotonic_ns()-stop_ns
                if not self.done.is_set():self.client.resume()
            if self.client._state=='running':self.client.interrupt()
            for addr,length in list(self.watches):self.remove_wp(addr,length)
            for addr in list(self.breaks):self.client.remove_breakpoint(addr);self.breaks.remove(addr)
        except Exception as e:self.error=f'{type(e).__name__}: {e}';self.ready.set()
        finally:
            self.client.close();self.events.close();self.coverage.close()
            self.write_results()
    def write_results(self):
        for inst in self.instances.values():
            pages=inst['touched_pages'];depth=inst['stack_top']-min(pages) if pages else 0
            inst['stack_write_attempt_depth_bound_bytes']=[max(0,depth-4096)+int(bool(pages)),depth]
            inst['scope']='aligned allocator event peak; CPU stack write-attempt4KiB bounds, NOT exact stack/RSS/kernel-stack peak'
        result={'status':'STOP_OBSERVATION' if self.error else 'COMPLETE','error':self.error,'profile':self.profile,'instances':list(self.instances.values()),'focus_base':self.focus,'heap_post_event_count':self.counters,'observer_pause_ns':self.pause_ns,'stack_write_attempts':self.stack_events,'snapshots':self.snapshots,'draw_events':self.draw_events,'resident_bytes':None,'exact_stack_peak_bytes':None,'kernel_stack_peak_bytes':None,'historical_runtime_baseline':None}
        dump(self.m.work/'evidence/observation.json',result)
    def shutdown(self):
        self.done.set();self.join(15)
        need(not self.is_alive(),'observer thread did not stop');self.check()
