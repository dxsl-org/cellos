"""Approved S10-4.4 host-only runtime support; candidate inputs are read-only."""
import argparse, hashlib, importlib.util, json, os, re, shutil, socket, subprocess, sys, time
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw
R=Path('/home/dmin/cellos'); W=Path('/tmp/cellbox-s10-4')
sys.path.insert(0,str(R/'scripts'))
import phase10_canonical_drive as D
C=Path('/tmp/cellbox-s10-3d-resume/cand/candidate.json')
CAND_SHA='4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821'
GLYPH=Path('/tmp/cellbox-phase10-a3hg7abb/host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs')
GLYPH_SHA='820c92d2dc0aa5c1f50533a42283974b07998111adaff8e416f56dd4481eec79'
FIXTURES={'headless':Path('/tmp/cellbox-phase10-40k50w51/disk-main/disk.img'),'desktop':Path('/tmp/cellbox-phase10-a3hg7abb/disk-main/disk.img')}
class Stop(RuntimeError):pass
def sha(p):
    with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def dump(p,v):Path(p).write_text(json.dumps(v,indent=2)+'\n')
def need(ok,why):
    if not ok:raise Stop(why)
def verify_inputs(profile):
    need(sha(C)==CAND_SHA,'candidate identity changed');c=json.loads(C.read_text());need(not c['failures'],'candidate has failures');p=c['profiles'][profile]
    for key in ['kernel_fs_img','init','kernel','iso']:
        need(sha(p[key]['path'])==p[key]['sha256'],f'{profile} {key} changed')
    need(sha(Path(p['dir'])/'manifest.json')==p['manifest_sha256'],'manifest changed')
    for name,f in p['files'].items():need(sha(f['file'])==f['sha256'],f'packaged file changed: {name}')
    for name in ['autoload','services','system']:
        actual=Path(p['dir'])/'boot-config'/f'{name}.toml'
        need(sha(actual)==p['files'][f'/etc/cellos/{name}.toml']['sha256'],'boot config mismatch')
    if profile=='desktop':need(GLYPH.exists() and sha(GLYPH)==GLYPH_SHA,'qualified glyph oracle missing/changed')
    return p
class StampedSerial(D.Serial):
    def __init__(self,path):super().__init__(path);self.receipts=[]
    def run(self):
        while not self.stop:
            try:data=self.conn.recv(65536)
            except OSError:return
            if not data:return
            stamp=time.monotonic_ns()
            with self.lock:
                self.receipts.append({'monotonic_ns':stamp,'offset':len(self.buf),'bytes':len(data)})
                self.buf.extend(data)
class Machine(D.Machine):
    def __init__(self,profile,work,observed=False):
        self.work=Path(work);self.work.mkdir(mode=0o700,parents=True,exist_ok=False)
        for n in ['evidence','scratch','sock']: (self.work/n).mkdir(mode=0o700)
        self.inputs=verify_inputs(profile); source=FIXTURES[profile];self.source_disk_sha=sha(source)
        disk=self.work/'disk.img'
        subprocess.run(['cp','--reflink=auto','--sparse=always',str(source),str(disk)],check=True)
        need(sha(disk)==self.source_disk_sha,'fixture clone differs')
        a=argparse.Namespace(profile=profile,work=str(work),evidence=str(self.work/'evidence'),iso=self.inputs['iso']['path'],accel='tcg',window=900,glyph_bin=str(GLYPH))
        super().__init__(a,'measurement',str(disk),False)
        self.observed=observed;self.start_ns=None;self.started=False
        if observed:
            self.gdb_path=self.work/'sock/gdb.sock'
            self.argv+=['-chardev',f'socket,path={self.gdb_path},server=on,wait=off,id=gdb0','-gdb','chardev:gdb0','-S']
        dump(self.work/'evidence/input-provenance.json',{'candidate_sha256':CAND_SHA,'profile':profile,'profile_record':self.inputs,'argv':self.argv,'fixture_source':str(source),'fixture_source_sha256':self.source_disk_sha,'fixture_clone_sha256':sha(disk),'glyph_sha256':GLYPH_SHA if profile=='desktop' else None,'mode':'observed' if observed else 'untraced'})
    def start(self):
        self.err=(self.work/'evidence/qemu-stderr.log').open('wb');self.t0=time.time()
        self.start_ns=time.monotonic_ns()
        self.proc=subprocess.Popen(self.argv,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=self.err)
        self.started=True;self.serial=StampedSerial(str(self.serial_path));self.serial.connect();self.serial.start();self.qmp=D.Qmp(str(self.qmp_path))
        return self
    def stop(self):
        raw=super().stop() if self.started else b''
        if self.started:
            dump(self.work/'evidence/serial-receipts.json',self.serial.receipts)
            self.serial.conn.close();self.err.close();self.qmp.file.close();self.qmp.sock.close()
        proof={'candidate_sha256':sha(C),'iso_sha256':sha(self.inputs['iso']['path']),'source_disk_sha256':sha(FIXTURES[self.a.profile])}
        proof['unchanged']=proof['candidate_sha256']==CAND_SHA and proof['iso_sha256']==self.inputs['iso']['sha256'] and proof['source_disk_sha256']==self.source_disk_sha
        dump(self.work/'evidence/final-input-proof.json',proof);need(proof['unchanged'],'measurement input changed')
        return raw
class CheckedDesk(D.Desk):
    def __init__(self,a,m,v,check):super().__init__(a,m,v);self.health_check=check
    def wait_for(self,pred,what,timeout,interval=1.0):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            self.health_check();img=self.grab();got=pred(img)
            if got:return img,got
            time.sleep(interval)
        raise Stop(f'timeout waiting for {what}')
def first_exact_prompt(sh,raw):
    """Judge every received prompt boundary with the existing exact-idle predicate."""
    for match in re.finditer(re.escape(b'Cellos > '),raw):
        end=match.end()
        if sh.idle(raw=raw[:end],count=1):return end
    return None
def exact_echo_output(lines,cmd):
    """Reassemble exact UART echo across existing service/kernel records only."""
    echo='';background=[]
    for index,line in enumerate(lines):
        if D.SERVICE_LINE.match(line) or D.KERNEL_LINE.match(line):
            background.append(line);continue
        need(cmd.startswith(echo+line),f'command echo diverged at {line!r}')
        echo+=line
        if echo==cmd:
            output=background+lines[index+1:]
            for i in range(len(output)-1,-1,-1):
                if output[i]=='Cellos > ':del output[i];break
            return output
    raise Stop(f'incomplete exact command echo {echo!r}')

class Surface:
    def __init__(self,m,observer=None):self.m=m;self.observer=observer;self.sh=D.Shell(m);self.d=None;self.timestamps={};self.events=[]
    def check(self):
        need(self.m.alive(),'owned QEMU exited')
        need(time.monotonic_ns()-self.m.start_ns<900_000_000_000,'900s measurement window expired')
        if self.observer:self.observer.check()
    def wait(self,predicate,timeout,label,interval=.02):
        until=time.monotonic()+timeout
        while time.monotonic()<until:
            self.check()
            value=predicate()
            if value:return value
            time.sleep(interval)
        raise Stop(f'timeout: {label}')
    def pause(self,seconds):
        until=time.monotonic()+seconds
        while time.monotonic()<until:self.check();time.sleep(min(.05,max(0,until-time.monotonic())))
    def phase(self,name):
        self.events.append({'phase':name,'monotonic_ns':time.monotonic_ns()})
        if self.observer:self.observer.set_phase(name)
    def snapshot(self,name):
        if self.observer:self.observer.snapshot(name)
    def ready(self):
        if self.m.a.profile=='headless':
            end=self.wait(lambda:first_exact_prompt(self.sh,self.sh.raw()),900,'exact initial idle prompt prefix')
            with self.m.serial.lock:
                receipt=next(r for r in self.m.serial.receipts if r['offset']<end<=r['offset']+r['bytes'])
            self.timestamps['exact_prompt_ns']=receipt['monotonic_ns']
            self.timestamps['prompt_prefix_end_offset']=end
            self.timestamps['predicate_observed_ns']=time.monotonic_ns()
        else:
            self.wait(lambda:'[desktop] Ready. Entering event loop.' in self.m.text(),900,'desktop ready')
            self.timestamps['desktop_ready_ns']=time.monotonic_ns();self.pause(3)
            v=D.Verdict([]);self.d=CheckedDesk(self.m.a,self.m,v,self.check);d=self.d
            pre=d.shot('desktop-before-terminal');need(pre.size==(1280,800),'desktop resolution');need(d.locate(pre) is None,'Terminal already present')
            self.phase('terminal-launch')
            self.timestamps['terminal_click_ns']=time.monotonic_ns();d.click_px(D.TASKBAR['terminal'],D.TASKBAR_Y);d.park()
            self.wait(lambda:'[terminal] Terminal surface ready. Entering event loop.' in self.m.text(),300,'Terminal surface')
            rows=['',D.BANNER,D.PROMPT];img=d.expect_screen(rows,'runtime initial exact prompt',240)
            self.timestamps['exact_prompt_ns']=time.monotonic_ns();d.shot('prompt',img)
            org=d.org;region=pre.crop((org[0],org[1],org[0]+D.WIN_W,org[1]+D.WIN_H));need(d.diff(region,d.pad(rows),0,D.ROWS-1,d.mcursor(rows))[0]>1000,'desktop prompt negative control')
            _,win=d.window(img);need(d.diff(win,d.pad(rows[:-1]+['Cellos # ']),0,D.ROWS-1,(len(D.PROMPT),2))[0]>=D.MIN_NEGATIVE_DIFF,'wrong-prompt negative control')
            need(d.cursor_of(win)==(len(D.PROMPT),2),'initial GUI cursor')
            d.click_px(org[0]+360,org[1]+300);d.park();self.pause(2);d.model[:]=rows;d.state['img']=img
        self.phase('initial-prompt');self.snapshot('initial-prompt')
    def command(self,name,cmd,timeout=120):
        self.check();start=time.monotonic_ns()
        if self.d:
            rows=self.d.command_read(name,cmd,timeout);out=rows
        else:
            before,at=self.sh.submit(cmd)
            need(self.sh.wait_prompt(before+1,timeout),f'no new prompt after {cmd!r} within {timeout}s')
            out=exact_echo_output(D.logical_lines(D.strip_ansi(self.sh.raw()[at:])),cmd)
        end=time.monotonic_ns();self.check();self.events.append({'command':cmd,'name':name,'start_ns':start,'end_ns':end,'rows':out})
        return out
    def status(self):
        out=self.command('status','echo st=$?',60)
        vals=[line.strip() for line in out if re.fullmatch('st=[0-9]+',line.strip())]
        need(len(vals)==1,f'bad status rows {out!r}');return int(vals[0][3:])
    def submit(self,cmd):
        if self.d:
            d=self.d;before=d.state['img'];typed=d.model[:-1]+[d.model[-1]+cmd];d.type_text(cmd);d.expect_screen(typed,'runtime typed command',120);d.tap('ret');return before
        return self.sh.submit(cmd)
    def gui_fresh(self):
        d=self.d
        def pred(img):
            got=d.read_screen(img)
            if got:
                rows,cursor=got
                if rows and rows[-1]==D.PROMPT.rstrip() and cursor==(len(D.PROMPT),len(rows)-1):return got
        img,got=d.wait_for(pred,'runtime new GUI prompt',10,interval=.1)
        rows,cursor=got;d.model[:]=rows[:-1]+[D.PROMPT];d.state['img']=img;d.shot('recovered',img);return rows
    def interrupt(self,before):
        stamp=time.monotonic_ns()
        if self.d:self.d.ctrl_c();rows=self.gui_fresh()
        else:
            self.sh.send(b'\x03');self.wait(lambda:self.sh.idle(count=before[0]+1),10,'new interrupt prompt');rows=[]
        recovered=time.monotonic_ns();status=self.status();need(status==130,f'interrupt status {status}')
        return {'ctrl_c_ns':stamp,'prompt_ns':recovered,'ctrl_c_to_prompt_ms':(recovered-stamp)/1e6,'status':status,'rows':rows}
    def sample_idle(self,name,seconds=10):
        self.phase(name);start=time.monotonic_ns();stat_before=cpu_ticks(self.m.proc.pid);raw_at=len(self.m.serial.raw());frames=[];prev=None;target=time.monotonic();end=target+seconds
        while time.monotonic()<end:
            self.check();now=time.monotonic_ns()
            if self.d:
                img=self.d.grab();_,win=self.d.window(img);need(win is not None,'idle Terminal window disappeared');cur=self.d.cursor_of(win)
                content=win.copy()
                if cur:
                    box=self.d.cell_box(*cur);ImageDraw.Draw(content).rectangle((box[0],box[1],box[2]-1,box[3]-1),fill=D.BG)
                diff=0 if prev is None else sum(ImageChops.difference(prev,content).convert('L').histogram()[1:]);prev=content
                path=self.d.shots/f'{name}-{len(frames):03d}.png';img.save(path);frames.append({'monotonic_ns':now,'changed_pixels':diff,'cursor':cur,'frame':str(path)})
            else:frames.append({'monotonic_ns':now,'serial_bytes':len(self.m.serial.raw())-raw_at})
            target+=.1;time.sleep(max(0,target-time.monotonic()))
        result={'name':name,'start_ns':start,'end_ns':time.monotonic_ns(),'target_hz':10,'samples':frames,'qemu_cpu_ticks_delta':cpu_ticks(self.m.proc.pid)-stat_before,'host_ticks_per_second':os.sysconf('SC_CLK_TCK'),'serial_bytes':len(self.m.serial.raw())-raw_at,'scope':'whole-QEMU CPU incl sampling overhead; content changes not redraw calls'}
        dump(self.m.work/f'evidence/{name}.json',result);self.snapshot(name);return result
    def top_idle(self):
        self.phase('interactive-top');before=self.submit('top')
        if self.d:
            def drawn(img):
                win=self.d.window(img)[1]
                return win is not None and all(self.d.cell_share(win,c,0,D.gui.TOP_ACCENT)>.5 for c in range(D.COLS))
            self.d.wait_for(drawn,'runtime top title bar',180,interval=.2)
        else:self.wait(lambda:D.Headless.cell_row_drawn(self.sh.raw()[before[1]:]),120,'top real cell row')
        result=self.sample_idle('interactive-top-idle');result['interrupt']=self.interrupt(before);return result
    def streams(self):
        self.phase('finite-stream');rows=self.command('finite-stream','top --records -d 1 --frames 2 | to jsonl | cat',120)
        need(self.status()==0,'finite stream failed')
        if not self.d:need(D.Headless.frame_count(D.json_rows(rows))==2,'finite stream not two frames')
        self.snapshot('finite-stream')
        self.phase('endless-collected-cat');cat_command='top --records --frames 100000 | to jsonl | cat';before=self.submit(cat_command);cat_start=time.monotonic_ns();self.pause(10)
        if self.d:
            img=self.d.shot('collected-cat-before-cancel');got=self.d.read_screen(img);need(got is not None,'collected cat screen unreadable');rows,cursor=got
            need(rows and rows[-1]!=D.PROMPT.rstrip(),'collected cat returned premature prompt')
            index=next((i for i,r in enumerate(rows) if cat_command in r),None);need(index is not None,'collected cat command not observed')
            need(not any('{' in r for r in rows[index+1:]),'collected cat emitted JSON before completion')
        else:
            segment=self.sh.raw()[before[1]:];lines=D.logical_lines(D.strip_ansi(segment))
            need(not any(l.startswith('{') for l in lines),'collected cat emitted JSON before completion')
            need(not self.sh.idle(count=before[0]+1),'collected cat returned premature prompt')
        self.snapshot('collected-cat-buffered');cat_cancel=self.interrupt(before);self.quiescent();self.snapshot('collected-cat-cancelled')
        self.phase('endless-terminal-stream');before=self.submit('top --records --frames 100000 | to jsonl');flow_start=time.monotonic_ns()
        if self.d:
            def flowing(img):
                got=self.d.read_screen(img)
                return got if got and sum('{' in row for row in got[0])>=3 else None
            self.d.wait_for(flowing,'runtime live JSON terminal stream',120,interval=.2)
        else:self.wait(lambda:D.strip_ansi(self.sh.raw()[before[1]:]).count(b'USER: {')>=3,120,'live JSON terminal stream')
        live_ns=time.monotonic_ns();self.pause(10);self.snapshot('terminal-stream-live');cancel=self.interrupt(before);self.quiescent();self.snapshot('terminal-stream-cancelled')
        self.phase('rg-large');before=self.submit('rg -F p10-literal-that-occurs-nowhere-0123456789 /mnt/sd/rg/big.log');self.pause(3);rg=self.interrupt(before);self.snapshot('rg-cancelled')
        return {'collected_cat_submit_ns':cat_start,'collected_cat_zero_json_seconds':10,'collected_cat_cancel':cat_cancel,'flow_submit_ns':flow_start,'first_live_observed_ns':live_ns,'terminal_stream_cancel':cancel,'rg_cancel':rg,'quiescence_seconds':4,'fixture_bytes':7340032,'scope':'Collected cat buffers; no-cat terminal sink streams. Separate contract-correct boundedness measures, not generic throughput.'}
    def quiescent(self):
        if self.d:
            image=self.d.grab();win=self.d.window(image)[1];cur=self.d.cursor_of(win);self.pause(4);later=self.d.grab();laterwin=self.d.window(later)[1]
            for im in [win,laterwin]:
                if cur:
                    box=self.d.cell_box(*cur);ImageDraw.Draw(im).rectangle((box[0],box[1],box[2]-1,box[3]-1),fill=D.BG)
            need(ImageChops.difference(win,laterwin).getbbox() is None,'GUI output continued after cancellation')
        else:
            at=len(self.sh.raw());self.pause(4);need(not any(l.startswith('{') for l in D.logical_lines(D.strip_ansi(self.sh.raw()[at:]))),'output continued after cancellation')
def cpu_ticks(pid):
    raw=Path(f'/proc/{pid}/stat').read_text();fields=raw[raw.rfind(')')+2:].split();return int(fields[11])+int(fields[12])
