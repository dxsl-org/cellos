import json,subprocess,time,os
from pathlib import Path
from rsp import RspClient,RspError
W=Path('/tmp/cellbox-s10-4/runtime/watch-step-loop-investigation');W.mkdir(exist_ok=False);os.chmod(W,0o700);sock=W/'gdb.sock';err=(W/'stderr').open('w')
argv=['qemu-system-x86_64','-machine','q35','-accel','tcg','-m','32M','-display','none','-serial','none','-monitor','none','-nic','none','-drive','file=/tmp/cellbox-s10-4/runtime/watch-step-loop.img,format=raw,if=floppy,readonly=on','-boot','a','-chardev',f'socket,path={sock},server=on,wait=off,id=dbg','-gdb','chardev:dbg','-S']
p=subprocess.Popen(argv,stdout=subprocess.DEVNULL,stderr=err);c=None;result={'argv':argv,'events':[],'loops':0,'status':'STARTING'}
try:
    end=time.monotonic()+10
    while True:
        assert time.monotonic()<end and p.poll() is None
        if not sock.exists():time.sleep(.01);continue
        c=RspClient(str(sock),str(W/'rsp.jsonl'))
        try:c.connect();break
        except ConnectionRefusedError:time.sleep(.01)
    c.add_breakpoint(0x7c0b);c.resume();c.wait_stop(timeout=30);assert c.read_register('rip')==0x7c0b;c.remove_breakpoint(0x7c0b);c.add_watchpoint(0x8000,2);c.add_breakpoint(0x7c10)
    for index in range(1000):
        c.resume();reply=c.wait_stop(timeout=5);pc=c.read_register('rip');assert pc==0x7c10 and b'watch:' in reply
        try:
            step=c.step_over_breakpoint(pc);after=c.read_register('rip');assert after==0x7c13
        except RspError as e:
            result['events'].append({'iteration':index,'error':str(e),'stop':c.last_stop.decode(),'rip':hex(c.read_register('rip')),'watched_word':c.read_memory(0x8000,2).hex()})
            if str(e)=='single native instruction did not advance current breakpoint':
                again=c.step_over_breakpoint(pc);result['isolated_only_second_step']={'stop':again.decode(),'rip':hex(c.read_register('rip'))}
            result['status']='NO_PROGRESS_REPRODUCED';break
        result['loops']=index+1
    else:result['status']='NO_NO_PROGRESS_IN_1000_ISOLATED_ITERATIONS'
except Exception as e:result.update(status='STOP_DIAGNOSTIC',error=f'{type(e).__name__}: {e}')
finally:
    if c:c.close()
    p.terminate()
    try:p.wait(timeout=5)
    except subprocess.TimeoutExpired:p.kill();p.wait()
    err.close();(W/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
