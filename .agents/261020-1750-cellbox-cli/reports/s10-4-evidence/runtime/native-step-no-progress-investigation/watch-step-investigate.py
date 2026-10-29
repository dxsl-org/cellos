import json,subprocess,time,os
from pathlib import Path
from rsp import RspClient,RspError
W=Path('/tmp/cellbox-s10-4/runtime/watch-step-investigation-ready');W.mkdir(exist_ok=False);os.chmod(W,0o700);rows=[]
for combined in [True,False]:
    name='write-watch-plus-execute' if combined else 'write-watch-only';sock=W/(name+'.sock');log=W/(name+'.rsp.jsonl');err=(W/(name+'.stderr')).open('w')
    argv=['qemu-system-x86_64','-machine','q35','-accel','tcg','-m','32M','-display','none','-serial','none','-monitor','none','-nic','none','-drive','file=/tmp/cellbox-s10-4/runtime/watch-step-repro.img,format=raw,if=floppy,readonly=on','-boot','a','-chardev',f'socket,path={sock},server=on,wait=off,id=dbg','-gdb','chardev:dbg','-S']
    proc=subprocess.Popen(argv,stdout=subprocess.DEVNULL,stderr=err);client=None;record={'variant':name,'argv':argv,'events':[]}
    try:
        until=time.monotonic()+10
        while not sock.exists():
            assert time.monotonic()<until and proc.poll() is None
            time.sleep(.01)
        while True:
            client=RspClient(str(sock),str(log))
            try:client.connect();break
            except ConnectionRefusedError:
                assert time.monotonic()<until and proc.poll() is None
                time.sleep(.01)
        client.add_breakpoint(0x7c0b);client.resume();r=client.wait_stop(timeout=30);assert client.read_register('rip')==0x7c0b
        client.remove_breakpoint(0x7c0b);client.add_watchpoint(0x8000,2)
        if combined:client.add_breakpoint(0x7c10)
        client.resume();r=client.wait_stop(timeout=5);pc=client.read_register('rip');record['events'].append({'point':'after_write','stop':r.decode(),'rip':hex(pc),'word':client.read_memory(0x8000,2).hex()});assert pc==0x7c10 and b'watch:' in r
        if not combined:client.add_breakpoint(0x7c10)
        for index in range(2):
            try:
                result=client.step_over_breakpoint(0x7c10)
                record['events'].append({'point':f'isolated_native_step_{index+1}','stop':result.decode(),'rip':hex(client.read_register('rip')),'word':client.read_memory(0x8000,2).hex()})
            except RspError as e:
                record['events'].append({'point':f'isolated_native_step_{index+1}','error':str(e),'rip':hex(client.read_register('rip')),'word':client.read_memory(0x8000,2).hex()})
            if client.read_register('rip')!=0x7c10:break
        record['status']='COMPLETE'
    except Exception as e:record.update(status='STOP',error=f'{type(e).__name__}: {e}')
    finally:
        if client:client.close()
        proc.terminate()
        try:proc.wait(timeout=5)
        except subprocess.TimeoutExpired:proc.kill();proc.wait()
        err.close();rows.append(record);(W/'result.json').write_text(json.dumps(rows,indent=2)+'\n')
print(json.dumps(rows,indent=2))
