//! Disposable P01 observer. No runtime authority or allocation lives here.
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering::*};

pub const STOP: u64 = 1 << 63;
pub const CONTENTION: u64 = 1;
pub const FULL: u64 = 2;
pub const LATE: u64 = 4;
pub const INCOMPLETE: u64 = 8;
pub const IDENTITY: u64 = 16;
pub const PROTOCOL: u64 = 32;
pub const COUNTER: u64 = 64;
static FLAGS: AtomicU64 = AtomicU64::new(0);
static GATE: AtomicU64 = AtomicU64::new(STOP);
static INDEX: AtomicU64 = AtomicU64::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);
static BOOT: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static COLLECTOR: AtomicBool = AtomicBool::new(false);
static STAGE: AtomicU64 = AtomicU64::new(0);
static LAST_SAMPLE: AtomicU64 = AtomicU64::new(0);
static START_TICK: AtomicU64 = AtomicU64::new(0);
static CLOSE_TICK: AtomicU64 = AtomicU64::new(0);
static EXPORTED: AtomicBool = AtomicBool::new(false);
static LAST_FLAGS: AtomicU64 = AtomicU64::new(0);
static EXPECTED: AtomicU64 = AtomicU64::new(0);
pub static MANIFEST: &[u8; 96] = include_bytes!(concat!(env!("EMBEDDED_OUT_DIR"), "/p01_manifest.bin"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner { pub tid: u64, pub root: u64, pub cell: u64, pub generation: u64 }
impl Owner {
    pub fn of(task: &crate::task::tcb::Task) -> Self {
        Self { tid: task.id as u64, root: task.root_tid as u64, cell: task.cell_id.0, generation: task.cell_generation }
    }
    fn words(self) -> [u64; 4] { [self.tid, self.root, self.cell, self.generation] }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectId(pub u8);
#[derive(Clone, Copy, Debug)]
pub struct Attempt { pub id: u64, pub parent: Owner, pub route: u8, selection: u8, index: u8 }
impl Attempt {
    pub(crate) fn selected(self) -> bool { self.selection == 2 && self.id != 0 }
}
#[derive(Clone, Copy, Debug)]
pub struct Obligation { pub index: u8, pub token: u64 }

struct Slot { payload: UnsafeCell<[u8; 160]>, ready: AtomicU8 }
// Each successful reservation owns exactly one slot; readers wait for Release ready
// and STOP+Acquire guard drain. A hole is never read, including timeout export.
unsafe impl Sync for Slot {}
static RECORDS: [Slot; 256] = [const { Slot { payload: UnsafeCell::new([0; 160]), ready: AtomicU8::new(0) } }; 256];
struct Subject { words: [AtomicU64; 8], ready: AtomicBool, terminal: AtomicBool, slot: AtomicU8, quota: AtomicU8 }
static SUBJECTS: [Subject; 9] = [const { Subject { words: [const { AtomicU64::new(0) }; 8], ready: AtomicBool::new(false), terminal: AtomicBool::new(false), slot: AtomicU8::new(0), quota: AtomicU8::new(0) } }; 9];
static SUBJECT_COUNT: AtomicU64 = AtomicU64::new(0);
struct Component { words: [AtomicU64; 8], busy: AtomicBool, finalised: AtomicBool }
static COMPONENTS: [[Component; 8]; 9] = [const { [const { Component { words: [const { AtomicU64::new(0) }; 8], busy: AtomicBool::new(false), finalised: AtomicBool::new(false) } }; 8] }; 9];
struct Entry { words: [AtomicU64; 8], state: AtomicU8 }
static OBLIGATIONS: [Entry; 128] = [const { Entry { words: [const { AtomicU64::new(0) }; 8], state: AtomicU8::new(0) } }; 128];
static OBLIGATION_COUNT: AtomicU64 = AtomicU64::new(0);
static SNAPSHOTS: [[[AtomicU64; 3]; 16]; 6] = [const { [const { [const { AtomicU64::new(0) }; 3] }; 16] }; 6];
struct AttemptState { state: AtomicU8, namespace: AtomicU64, error: AtomicU64 }
static ATTEMPTS: [AttemptState; 10] = [const { AttemptState { state: AtomicU8::new(0), namespace: AtomicU64::new(0), error: AtomicU64::new(0) } }; 10];
static ATTEMPT_COUNT: AtomicU64 = AtomicU64::new(0);
static DRIVER_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(0);
static LOGICAL_PENDING: AtomicU64 = AtomicU64::new(0);
static ARM_ONCE: AtomicBool = AtomicBool::new(false);
static SAMPLE_MASK: AtomicU64 = AtomicU64::new(0);
static SAMPLE_TICKS: [AtomicU64;6] = [const { AtomicU64::new(0) };6];
static DENIED: AtomicBool = AtomicBool::new(false);

pub fn invalidate(bits: u64) { FLAGS.fetch_or(bits, Relaxed); }
pub struct Observation { _private: () }
/// Created only by the exclusive collector after STOP and Acquire guard drain.
/// Producers cannot construct this proof or mutate payload after the cutoff.
pub struct Frozen { _private: () }
pub fn observation() -> Option<Observation> {
    let old = GATE.load(Acquire);
    if old & STOP != 0 { return None; }
    if old >= 1024 { invalidate(COUNTER); return None; }
    if GATE.compare_exchange(old, old + 1, AcqRel, Acquire).is_err() { invalidate(CONTENTION); return None; }
    Some(Observation { _private: () })
}
pub fn scoped_observation() -> Option<Observation> {
    let old = GATE.load(Acquire);
    if old & STOP != 0 { invalidate(LATE); return None; }
    if old >= 1024 { invalidate(COUNTER); return None; }
    if let Err(current) = GATE.compare_exchange(old, old + 1, AcqRel, Acquire) {
        invalidate(if current & STOP != 0 { LATE } else { CONTENTION });
        return None;
    }
    Some(Observation { _private: () })
}
impl Drop for Observation { fn drop(&mut self) { let old = GATE.fetch_sub(1, Release); if old & !STOP == 0 { invalidate(COUNTER); } } }
fn reserve(index: &AtomicU64, bound: u64) -> Option<usize> {
    let old = index.load(Acquire);
    if old >= bound { invalidate(FULL); return None; }
    if index.compare_exchange(old, old + 1, AcqRel, Acquire).is_err() { invalidate(CONTENTION); return None; }
    Some(old as usize)
}
fn put64(b: &mut [u8], n: usize, v: u64) { b[n..n+8].copy_from_slice(&v.to_le_bytes()); }
fn put16(b: &mut [u8], n: usize, v: u16) { b[n..n+2].copy_from_slice(&v.to_le_bytes()); }
fn row(kind: u16, owner: Option<Owner>, attempt: Option<Attempt>, key: u64, group: u16, result: u16, detail: u32, values: [u64; 4], digest: Option<[u8; 32]>) -> [u8; 160] {
    let mut b = [0; 160]; let mut flags = 0u16;
    put16(&mut b, 8, kind); put16(&mut b, 12, crate::task::hart_local::current_hart_id() as u16);
    if let Some(o) = owner { flags |= 3; for (i, v) in o.words().into_iter().enumerate() { put64(&mut b, 24+i*8, v); } }
    if let Some(a) = attempt { flags |= 12; b[14] = a.route; b[15] = if a.route == 1 { 1 } else { 2 }; put64(&mut b, 16, a.id); put64(&mut b, 56, a.parent.tid); put64(&mut b, 64, a.parent.cell); put64(&mut b, 72, a.parent.generation); }
    if let Some(d) = digest { flags |= 16; b[128..160].copy_from_slice(&d); }
    if kind == 10 { flags |= 64 | 128; }
    if kind == 8 || kind == 9 { flags |= 256; }
    put16(&mut b, 10, flags); put64(&mut b, 80, key); put16(&mut b, 88, group); put16(&mut b, 90, result); b[92..96].copy_from_slice(&detail.to_le_bytes());
    for (i,v) in values.into_iter().enumerate() { put64(&mut b, 96+i*8, v); } b
}
fn append(_guard: &Observation, b: [u8;160]) { write_slot(b); }
fn append_frozen(_frozen: &Frozen, b: [u8;160]) { write_slot(b); }
fn write_slot(mut b: [u8;160]) {
    let Some(i) = reserve(&INDEX, 256) else { return }; put64(&mut b, 0, i as u64 + 1);
    // SAFETY: reservation unique and never reused, payload not accessed before ready.
    unsafe { *RECORDS[i].payload.get() = b; }
    RECORDS[i].ready.store(1, Release);
}
fn emit(b: [u8;160]) { if let Some(g) = observation() { append(&g,b); } }
fn load_owner(words: &[AtomicU64]) -> Owner { Owner { tid: words[0].load(Relaxed), root: words[1].load(Relaxed), cell: words[2].load(Relaxed), generation: words[3].load(Relaxed) } }
pub fn subject(owner: Owner) -> Option<SubjectId> {
    SUBJECTS.iter().position(|s| s.ready.load(Acquire) && load_owner(&s.words) == owner).map(|i| SubjectId(i as u8))
}
/// Resolve the immutable selected root identity without consulting SCHEDULER.
pub(crate) fn selected_owner(cell: u64, generation: u64) -> Option<Owner> {
    SUBJECTS.iter().filter(|s| s.ready.load(Acquire)).map(|s| load_owner(&s.words))
        .find(|owner| owner.cell == cell && owner.generation == generation
            && owner.tid == owner.root)
}
fn bootstrap() -> Option<Owner> { ARMED.load(Acquire).then(|| load_owner(&BOOT)) }
fn driver() -> Option<Owner> { SUBJECTS[0].ready.load(Acquire).then(|| load_owner(&SUBJECTS[0].words)) }
pub fn arm(owner: Owner, digest: [u8;32]) {
    if ARM_ONCE.compare_exchange(false,true,AcqRel,Acquire).is_err() { invalidate(PROTOCOL); return; }
    for (w,v) in BOOT.iter().zip(owner.words()) { w.store(v,Relaxed); }
    if digest.as_slice() != &MANIFEST[..32] { invalidate(IDENTITY); }
    START_TICK.store(crate::task::system_ticks() as u64,Relaxed);
    // Independent heap-source epoch: no frame/heap lock nesting and no gap in
    // allocation tracking while the ordinary observation gate is still closed.
    crate::memory::heap::p01_owner::arm();
    {
        // No bitmap allocation/free can cross the arm cutoff unrepresented:
        // census preexisting bits while admission is still STOP, then open it
        // before releasing the existing source-owner lock.
        let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        match frames.as_ref() {
            Some(allocator) => allocator.p01_seed_physical(),
            None => crate::p01_retained::physical_fault(),
        }
        GATE.store(0,Release);
        ARMED.store(true,Release);
    }
    emit(row(1,Some(owner),None,0,0,1,0,[0;4],Some(digest)));
}
/// Mint before validation; logical context never holds an observer guard.
pub fn ingress(parent: Owner, route: u8) -> Option<Attempt> {
    if bootstrap() != Some(parent) && driver() != Some(parent) { return None; }
    // A post-freeze bootstrap request may still prove to be unrelated. Carry
    // only a local classification context, without touching frozen payloads.
    if GATE.load(Acquire)&STOP!=0 && bootstrap()==Some(parent) {
        return Some(Attempt{id:0,parent,route,selection:0,index:255});
    }
    let _g = scoped_observation()?;
    let id = reserve(&NEXT_ATTEMPT,u64::MAX-1)? as u64+1;
    reserve(&LOGICAL_PENDING,1024)?;
    Some(Attempt { id,parent,route,selection:0,index:255 })
}
pub fn classify(attempt: &mut Option<Attempt>, path: &str) {
    let Some(a) = attempt.as_mut() else { return };
    let selected = if bootstrap() == Some(a.parent) { path == "/bin/bench" } else { true };
    if !selected { a.selection=4; return; }
    let Some(g) = scoped_observation() else { return };
    if bootstrap() == Some(a.parent) && !matches!(a.route, 1 | 2) { invalidate(IDENTITY); }
    if driver() == Some(a.parent) && a.route != 3 { invalidate(IDENTITY); }
    if bootstrap() == Some(a.parent) {
        if reserve(&DRIVER_ATTEMPTS,2).is_none() { invalidate(PROTOCOL); }
    } else if path == "/bin/vfs" && a.route==3 { DENIED.store(true,Release); }
    else if path != "/bin/bench-probe" { invalidate(IDENTITY); }
    let Some(i)=reserve(&ATTEMPT_COUNT,10) else{return};a.index=i as u8;a.selection=2;ATTEMPTS[i].state.store(2,Release);
    append(&g,row(3,None,Some(*a),0,0,4,0,[0;4],None));
}
pub fn source_error(attempt: Option<Attempt>, error: types::ViError) {
    if let Some(a)=attempt.filter(|a|a.selection==2) { let Some(_g)=scoped_observation() else{return}; let s=&ATTEMPTS[a.index as usize]; s.namespace.store(2,Relaxed); s.error.store(error as u64,Relaxed); }
}
pub fn outcome(attempt: Option<Attempt>, result: crate::task::syscall::SyscallResult) {
    let Some(a)=attempt else{return};
    if a.id==0 && a.selection==4{return;}
    let Some(g)=scoped_observation() else{return};
    if a.id!=0 {LOGICAL_PENDING.fetch_sub(1,Release);}
    if a.selection==4{return;}
    let s=ATTEMPTS.get(a.index as usize);
    if s.is_some_and(|s|s.state.load(Acquire)==3){return;}
    if a.selection==0 { invalidate(IDENTITY); append(&g,row(3,None,Some(a),0,0,4,0,[0;4],None)); }
    match result {
        Ok(_) => { invalidate(INCOMPLETE); if let Some(s)=s{s.state.store(5,Release);} }
        Err(e) => {
            let (ns,raw)=if let Some(s)=s.filter(|s|s.namespace.load(Relaxed)==2){(2,s.error.load(Relaxed))}else{(1,e as u64)};
            let denied=if ns==2 { raw==types::ViError::PermissionDenied as u64 }
                else { matches!(e,crate::task::syscall::SyscallError::PermissionDenied) };
            append(&g,row(4,None,Some(a),0,ns,if denied{2}else{3},0,[raw,0,0,0],None));if let Some(s)=s{s.state.store(3,Release);}
        }
    }
}
/// Called under the existing publication owner before ready, after verified admission.
pub fn publication(owner: Owner, attempt: Option<Attempt>, digest: [u8;32]) {
    let Some(a)=attempt.filter(|a|a.selection==2) else{return}; let Some(g)=scoped_observation() else{return}; let s=&ATTEMPTS[a.index as usize];
    if s.state.load(Acquire)!=2 { invalidate(IDENTITY); return; }
    let is_driver=bootstrap()==Some(a.parent);
    let expected=if is_driver {&MANIFEST[32..64]} else {&MANIFEST[64..96]};
    if digest.as_slice()!=expected || owner.tid!=owner.root || (!is_driver && a.route!=3) { invalidate(IDENTITY); }
    let Some(i)=reserve(&SUBJECT_COUNT,9) else{return};
    if (is_driver && i!=0) || (!is_driver && driver()!=Some(a.parent)) { invalidate(IDENTITY); }
    let entry=&SUBJECTS[i]; let values=[owner.tid,owner.root,owner.cell,owner.generation,a.parent.tid,a.parent.cell,a.parent.generation,a.id];
    for (w,v) in entry.words.iter().zip(values) {w.store(v,Relaxed);} entry.ready.store(true,Release);
    append(&g,row(4,Some(owner),Some(a),i as u64,0,1,0,[0;4],Some(digest)));
    for class in 1..=8 { append(&g,row(8,Some(owner),None,i as u64,class,4,0,[0;4],None)); }
    s.state.store(3,Release);
}
fn lifecycle(owner:Owner,kind:u16,released:bool) {
    let Some(id)=subject(owner) else{return}; let Some(g)=scoped_observation() else{return}; let s=&SUBJECTS[id.0 as usize];
    let once=match kind {5=>s.terminal.compare_exchange(false,true,AcqRel,Acquire).is_ok(),6=>s.slot.compare_exchange(0,if released{1}else{5},AcqRel,Acquire).is_ok(),_=>s.quota.compare_exchange(0,if released{1}else{5},AcqRel,Acquire).is_ok()};
    if !once {invalidate(PROTOCOL);return;} append(&g,row(kind,Some(owner),None,id.0 as u64,0,if released{1}else{5},0,[0;4],None));
}
pub fn task_terminal(owner:Owner){lifecycle(owner,5,true)}
pub fn owner_slot_result(owner:Owner,released:bool){lifecycle(owner,6,released)}
pub fn quota_result(owner:Owner,released:bool){lifecycle(owner,7,released)}
fn with_component(owner:Owner,class:u16,f:impl FnOnce(&Component)) {
    if subject(owner).is_none(){return;} let Some(g)=scoped_observation() else{return};
    with_component_guard(&g,owner,class,f);
}
fn with_component_guard(_g:&Observation,owner:Owner,class:u16,f:impl FnOnce(&Component)) {
    let Some(id)=subject(owner) else{return};
    let Some(c)=COMPONENTS[id.0 as usize].get(class.wrapping_sub(1) as usize) else {invalidate(PROTOCOL);return};
    if c.finalised.load(Acquire){invalidate(LATE);return;}
    if c.busy.compare_exchange(false,true,Acquire,Relaxed).is_err(){invalidate(CONTENTION);return;}
    if c.finalised.load(Acquire){invalidate(LATE);c.busy.store(false,Release);return;}
    f(c); c.busy.store(false,Release);
}
fn add(c:&Component,field:usize,delta:u64) {
    let old=c.words[field].load(Relaxed); if let Some(v)=old.checked_add(delta){c.words[field].store(v,Relaxed)}else{invalidate(COUNTER)}
}
fn sub(c:&Component,field:usize,delta:u64) {
    let old=c.words[field].load(Relaxed); if let Some(v)=old.checked_sub(delta){c.words[field].store(v,Relaxed)}else{invalidate(COUNTER)}
}
pub fn component_retry(owner:Owner,class:u16){with_component(owner,class,|c|{add(c,6,1);add(c,7,1)});}
pub fn component_final(frozen:&Frozen,owner:Owner,class:u16,result:u16) {
    let Some(id)=subject(owner) else{return};
    let Some(c)=COMPONENTS[id.0 as usize].get(class.wrapping_sub(1) as usize) else{invalidate(PROTOCOL);return};
    if c.finalised.swap(true,AcqRel){invalidate(PROTOCOL);return;}
    let values=core::array::from_fn(|i|c.words[i].load(Relaxed));
    append_frozen(frozen,row(9,Some(owner),None,id.0 as u64,class,result,0,values,None));
}
pub fn obligation_start(owner:Owner,class:u16,bytes:u64)->Option<Obligation> {
    subject(owner)?; let _g=scoped_observation()?; let token=reserve(&OBLIGATION_COUNT,u64::MAX-1)? as u64+1;
    // Bounded scan, then exactly one CAS. No retry on a competing observer.
    let Some(i)=OBLIGATIONS.iter().position(|e|matches!(e.state.load(Acquire),0|2)) else{invalidate(FULL);return None};
    let e=&OBLIGATIONS[i];let state=e.state.load(Acquire);
    if !matches!(state,0|2)||e.state.compare_exchange(state,3,AcqRel,Acquire).is_err(){invalidate(CONTENTION);return None;}
    let words=[owner.cell,owner.generation,owner.tid,token,bytes,class as u64 | (4<<16),INDEX.load(Relaxed),0];
    for (w,v) in e.words.iter().zip(words){w.store(v,Relaxed);} e.state.store(1,Release);
    with_component_guard(&_g,owner,class,|c|{add(c,0,1);add(c,4,bytes);add(c,7,1)});
    Some(Obligation{index:i as u8,token})
}
pub fn obligation_finish(token:Obligation,result:u16) {
    let Some(e)=OBLIGATIONS.get(token.index as usize) else{invalidate(IDENTITY);return}; let Some(_g)=scoped_observation() else{return};
    if e.state.compare_exchange(1,3,AcqRel,Acquire).is_err(){invalidate(PROTOCOL);return;}
    if e.words[3].load(Relaxed)!=token.token {invalidate(IDENTITY);e.state.store(1,Release);return;}
    let tid=e.words[2].load(Relaxed); let owner=SUBJECTS.iter().find(|s|s.ready.load(Acquire)&&s.words[0].load(Relaxed)==tid).map(|s|load_owner(&s.words));
    let class=e.words[5].load(Relaxed) as u16; let bytes=e.words[4].load(Relaxed);
    e.words[5].store(class as u64 | ((result as u64)<<16),Relaxed);e.words[7].store(INDEX.load(Relaxed),Relaxed);
    if let Some(owner)=owner {with_component_guard(&_g,owner,class,|c|{sub(c,0,1);sub(c,4,bytes);match result{1=>add(c,1,1),5=>{add(c,2,1);add(c,5,bytes)},_=>add(c,3,1)};add(c,7,1)});}else{invalidate(IDENTITY)}
    e.state.store(2,Release);
}
pub fn component_counts(_frozen:&Frozen,owner:Owner,class:u16)->Option<[u64;8]> {
    let id=subject(owner)?;
    let c=COMPONENTS[id.0 as usize].get(class.checked_sub(1)? as usize)?;
    Some(core::array::from_fn(|i|c.words[i].load(Relaxed)))
}
fn pending()->bool { LOGICAL_PENDING.load(Acquire)!=0 || ATTEMPTS.iter().any(|a|matches!(a.state.load(Acquire),2|5)) || OBLIGATIONS.iter().any(|e|matches!(e.state.load(Acquire),1|3)) }
fn driver_terminal()->bool { SUBJECTS[0].ready.load(Acquire)&&SUBJECTS[0].terminal.load(Acquire)&&SUBJECTS[0].slot.load(Acquire)!=0&&SUBJECTS[0].quota.load(Acquire)!=0 }
fn snapshot(sample:usize) {
    let Some(g)=scoped_observation() else{return};
    let heap_cutoff = if sample == 0 {
        crate::memory::heap::p01_owner::capture_baseline(&g)
    } else {
        crate::memory::heap::p01_owner::capture_stage(sample + 1, &g)
    };
    let mut metrics=crate::p01_topology::capture(sample + 1);
    if let Some((used, free, epoch)) = heap_cutoff {
        metrics[4] = crate::p01_resources::Metric { value: used, epoch_before: epoch, epoch_after: epoch, status: 1, source_owner: 2 };
        metrics[5] = crate::p01_resources::Metric { value: free, epoch_before: epoch, epoch_after: epoch, status: 1, source_owner: 2 };
    } else {
        invalidate(INCOMPLETE);
        metrics[4].status = 4;
        metrics[5].status = 4;
    }
    let now=crate::task::system_ticks() as u64;
    let flags=FLAGS.load(Acquire);
    metrics[15].value=flags;metrics[15].epoch_before=flags;metrics[15].epoch_after=flags;metrics[15].status=1;metrics[15].source_owner=4;
    // Source coherence is independent of lifecycle settlement. Preserve measured
    // nonzero values; component outcomes and stage timing gate qualification.
    SAMPLE_MASK.fetch_or(1<<sample,Relaxed);SAMPLE_TICKS[sample].store(now,Relaxed);
    for (i,m) in metrics.iter().enumerate(){SNAPSHOTS[sample][i][0].store(m.epoch_before,Relaxed);SNAPSHOTS[sample][i][1].store(m.epoch_after,Relaxed);SNAPSHOTS[sample][i][2].store(m.status as u64 | ((m.source_owner as u64)<<32),Relaxed);}
    for group in 0..4 {let mut values=[0;4];let mut presence=0;for j in 0..4{let m=metrics[group*4+j];if m.status==1 && m.source_owner!=0 && m.epoch_before==m.epoch_after {values[j]=m.value;presence|=1<<j;}}
        append(&g,row(10,None,None,sample as u64+1,group as u16,1,presence,values,None));}
}
/// Existing Log only; caller identity was copied under SCHEDULER, lock released.
pub fn stage(owner:Owner,msg:&str) {
    if driver()!=Some(owner){return;} if !msg.starts_with("P01:"){return;}
    if COLLECTOR.compare_exchange(false,true,Acquire,Relaxed).is_err(){invalidate(CONTENTION);return;}
    let n=STAGE.load(Relaxed);let now=crate::task::system_ticks() as u64;
    let next=match (n,msg){(0,"P01:pre count=0")=>Some((1,0)),(0,"P01:pre count=1")=>Some((1,1)),(0,"P01:pre count=4")=>Some((1,4)),(0,"P01:pre count=8")=>Some((1,8)),(1,"P01:live")=>Some((2,0)),(1,"P01:cleanup") if DENIED.load(Acquire)=>Some((3,0)),(2,"P01:cleanup")=>Some((3,0)),(3..=5,"P01:settle")=>Some((n+1,0)),(3,"P01:closing") if DENIED.load(Acquire)=>Some((7,0)),(6,"P01:closing")=>Some((7,0)),_=>None};
    if let Some((next,count))=next {
        if next==1{EXPECTED.store(count,Relaxed);crate::p01_resources::reset_peaks();emit(row(2,Some(owner),None,0,0,1,0,[count,0,0,0],None));observer_fault();}
        if next<=6{snapshot(next as usize-1);LAST_SAMPLE.store(now,Relaxed);}
        if next==7{CLOSE_TICK.store(now,Relaxed);emit(row(11,Some(owner),None,0,0,4,0,[0;4],None));}
        STAGE.store(next,Release);
    }else{invalidate(PROTOCOL);}
    COLLECTOR.store(false,Release);
}
fn observer_fault() {
    match env!("CELLOS_P01_FAULT") {
        "contention" => {
            if let Some(_guard)=observation() {
                // Deliberately stale admission input while the collector owns
                // the open pre boundary. This exercises a real failed strong
                // CAS without leaking an unowned increment or altering runtime.
                if GATE.compare_exchange(STOP,1,AcqRel,Acquire).is_err(){invalidate(CONTENTION);}
            }
        }
        "full" => { INDEX.store(256,Release);invalidate(FULL); }
        "unready" => {
            if let Some(_guard)=observation(){let _reserved_unready=reserve(&INDEX,256);invalidate(INCOMPLETE);}
        }
        _ => {}
    }
}
fn hex(bytes:&[u8],out:&mut [u8]) { const H:&[u8;16]=b"0123456789abcdef"; for(i,b)in bytes.iter().enumerate(){out[2*i]=H[(b>>4)as usize];out[2*i+1]=H[(b&15)as usize];} }
fn line(tag:&str,bytes:&[u8]) {let mut out=[0u8;352];hex(bytes,&mut out);let text=core::str::from_utf8(&out[..bytes.len()*2]).expect("hex ASCII");log::warn!("{} {}",tag,text);}
fn stage_line(tag:&str,sample:usize,bytes:&[u8]) {let mut out=[0u8;352];hex(bytes,&mut out);let text=core::str::from_utf8(&out[..bytes.len()*2]).expect("hex ASCII");log::warn!("{} {} {}",tag,sample,text);}
fn words64(words:&[AtomicU64])->[u8;64]{let mut b=[0;64];for(i,w)in words.iter().enumerate(){put64(&mut b,i*8,w.load(Acquire));}b}
fn static_bytes()->usize {
    core::mem::size_of_val(&RECORDS)+core::mem::size_of_val(&SUBJECTS)
        +core::mem::size_of_val(&COMPONENTS)+core::mem::size_of_val(&OBLIGATIONS)
        +core::mem::size_of_val(&SNAPSHOTS)+core::mem::size_of_val(&ATTEMPTS)
        +core::mem::size_of_val(&BOOT)+core::mem::size_of_val(&SAMPLE_TICKS)
        +16*core::mem::size_of::<AtomicU64>()+5*core::mem::size_of::<AtomicBool>()
        +core::mem::size_of_val(&MANIFEST)+96
}
/// Single nonwaiting collector, called only outside all subsystem locks.
pub fn safe_point() {
    if !ARMED.load(Acquire){return;}if COLLECTOR.compare_exchange(false,true,Acquire,Relaxed).is_err(){return;}
    let now=crate::task::system_ticks() as u64;
    if EXPORTED.load(Acquire){let flags=FLAGS.load(Acquire);if flags!=LAST_FLAGS.swap(flags,AcqRel){log::warn!("P01-LATE flags={}",flags);}COLLECTOR.store(false,Release);return;}
    let closing=STAGE.load(Acquire)==7;let timeout=now.saturating_sub(if closing{CLOSE_TICK.load(Relaxed)}else{START_TICK.load(Relaxed)})>=500;
    if GATE.load(Acquire)&STOP==0 {
        if !timeout && !(closing&&driver_terminal()&&!pending()){COLLECTOR.store(false,Release);return;}
        if timeout{invalidate(INCOMPLETE);}
        GATE.fetch_or(STOP,AcqRel);
    }
    let active=GATE.load(Acquire)&!STOP;
    if active!=0&&!timeout{COLLECTOR.store(false,Release);return;}
    if active!=0{invalidate(INCOMPLETE);} else {
        // STOP cannot reopen: all admitted payload/classification writers are done.
        let frozen=Frozen{_private:()};
        // Ordinary STOP is not a simultaneous multi-source cutoff. Freeze heap
        // census plus actual heap accounting under that source's existing lock.
        crate::memory::heap::p01_owner::freeze(&frozen);
        if pending(){invalidate(INCOMPLETE);}
        let expected=if DENIED.load(Acquire){1}else{EXPECTED.load(Acquire)+1};
        if SUBJECT_COUNT.load(Acquire)!=expected || STAGE.load(Acquire)!=7{invalidate(INCOMPLETE);}
        crate::p01_resources::finalize_all(&frozen);
        append_frozen(&frozen,row(12,driver(),None,0,0,if FLAGS.load(Acquire)==0{1}else{7},0,[FLAGS.load(Acquire),0,0,0],None));
    }
    let reserved=INDEX.load(Acquire);let mut ready=0;
    line("P01-MANIFEST",MANIFEST);
    log::warn!("P01-STORAGE payload=56896 core_static={} resource_static={} exporter_buffer=352",static_bytes(),crate::p01_resources::static_bytes());
    for slot in RECORDS.iter().take(reserved as usize){if slot.ready.load(Acquire)==1{ready+=1;/* SAFETY: immutable ready slot; no future writes to this slot. */line("P01-RECORD",unsafe{&*slot.payload.get()});}}
    if ready!=reserved{invalidate(INCOMPLETE);}
    if active==0 {
        let heap = crate::memory::heap::p01_owner::snapshot(None);
        let accounting_before = crate::memory::heap::p01_owner::accounting();
        if let Some(fault) = crate::memory::heap::p01_owner::copy_first_fault() {
            let mut bytes = [0u8; 48];
            for (i, value) in fault.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
            line("P01-HEAP-FAULT", &bytes);
        }
        if let Some(source) = crate::memory::heap::p01_owner::copy_baseline_source() {
            let mut bytes = [0u8; 96];
            for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
            line("P01-HEAP-BASELINE-SOURCE", &bytes);
            for index in 0..crate::memory::heap::p01_owner::CAPACITY {
                if let Some(entry) = crate::memory::heap::p01_owner::copy_baseline_entry(index) {
                    let mut bytes = [0u8; 160];
                    for (i, value) in entry.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                    line("P01-HEAP-BASELINE-ENTRY", &bytes);
                }
            }
        } else { invalidate(INCOMPLETE); }
        for baseline in [true, false] {
            if let Some(source) = crate::memory::heap::p01_owner::copy_charged_source(baseline) {
                let mut bytes = [0u8; 176];
                for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                line(if baseline { "P01-HEAP-BASELINE-CHARGED-SOURCE" } else { "P01-HEAP-CHARGED-SOURCE" }, &bytes);
                for index in 0..crate::memory::heap::p01_owner::CAPACITY {
                    if let Some(entry) = crate::memory::heap::p01_owner::copy_charged_entry(index, baseline) {
                        let mut bytes = [0u8; 40];
                        for (i, value) in entry.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        line(if baseline { "P01-HEAP-BASELINE-CHARGED-ENTRY" } else { "P01-HEAP-CHARGED-ENTRY" }, &bytes);
                    }
                }
            } else { invalidate(INCOMPLETE); }
        }
        for sample in 2..=6 {
            if let Some(source) = crate::memory::heap::p01_owner::copy_stage_source(sample) {
                let mut bytes = [0u8; 176];
                for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                stage_line("P01-HEAP-STAGE-SOURCE", sample, &bytes);
                for index in 0..crate::memory::heap::p01_owner::CAPACITY {
                    if let Some(entry) = crate::memory::heap::p01_owner::copy_stage_entry(sample, index) {
                        let mut bytes = [0u8; 152];
                        for (i, value) in entry.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-HEAP-STAGE-ENTRY", sample, &bytes);
                    }
                    if let Some(entry) = crate::memory::heap::p01_owner::copy_stage_charged_entry(sample, index) {
                        let mut bytes = [0u8; 40];
                        for (i, value) in entry.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-HEAP-STAGE-CHARGED-ENTRY", sample, &bytes);
                    }
                }
            } else if SAMPLE_MASK.load(Acquire) & (1 << (sample - 1)) != 0 {
                invalidate(INCOMPLETE);
            }
        }
        for sample in 1..=6 {
            if let Some(source) = crate::p01_noncpu_topology::source(sample) {
                let mut bytes = [0u8; 128];
                for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                stage_line("P01-XTREE-SOURCE", sample, &bytes);
                for index in 0..source[5] as usize {
                    if let Some(row) = crate::p01_noncpu_topology::node(sample, index) {
                        let mut bytes = [0u8; 40];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-XTREE-NODE", sample, &bytes);
                    }
                }
                for index in 0..source[6] as usize {
                    if let Some(row) = crate::p01_noncpu_topology::edge(sample, index) {
                        let mut bytes = [0u8; 56];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-XTREE-EDGE", sample, &bytes);
                    }
                }
            } else if SAMPLE_MASK.load(Acquire) & (1 << (sample - 1)) != 0 {
                invalidate(INCOMPLETE);
            }
            if let Some(source) = crate::p01_vtd_topology::source(sample) {
                let mut bytes = [0u8; 128];
                for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                stage_line("P01-VTD-SOURCE", sample, &bytes);
                for index in 0..(source[5] + source[6]) as usize {
                    if let Some(row) = crate::p01_vtd_topology::node(sample, index) {
                        let mut bytes = [0u8; 64];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-VTD-NODE", sample, &bytes);
                    }
                }
                for index in 0..source[7] as usize {
                    if let Some(row) = crate::p01_vtd_topology::edge(sample, index) {
                        let mut bytes = [0u8; 64];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-VTD-EDGE", sample, &bytes);
                    }
                }
            } else if SAMPLE_MASK.load(Acquire) & (1 << (sample - 1)) != 0 {
                invalidate(INCOMPLETE);
            }
            if let Some(source) = crate::p01_noncpu::source(sample) {
                let mut bytes = [0u8; 128];
                for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                stage_line("P01-NONCPU-SOURCE", sample, &bytes);
                for index in 0..source[6] as usize {
                    if let Some(row) = crate::p01_noncpu::root(sample, index) {
                        let mut bytes = [0u8; 64];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-NONCPU-ROOT", sample, &bytes);
                    }
                }
                for index in 0..source[7] as usize {
                    if let Some(row) = crate::p01_noncpu::reference(sample, index) {
                        let mut bytes = [0u8; 40];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-NONCPU-REFERENCE", sample, &bytes);
                    }
                }
            } else if SAMPLE_MASK.load(Acquire) & (1 << (sample - 1)) != 0 {
                invalidate(INCOMPLETE);
            }
            if let Some(source) = crate::p01_topology::source(sample) {
                let mut bytes = [0u8; 128];
                for (i, value) in source.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                stage_line("P01-TOPOLOGY-SOURCE", sample, &bytes);
                for index in 0..source[5] as usize {
                    if let Some(row) = crate::p01_topology::root(sample, index) {
                        let mut bytes = [0u8; 64];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-TOPOLOGY-ROOT", sample, &bytes);
                    }
                }
                for index in 0..source[6] as usize {
                    if let Some(row) = crate::p01_topology::node(sample, index) {
                        let mut bytes = [0u8; 40];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-TOPOLOGY-NODE", sample, &bytes);
                    }
                }
                for index in 0..source[7] as usize {
                    if let Some(row) = crate::p01_topology::edge(sample, index) {
                        let mut bytes = [0u8; 56];
                        for (i, value) in row.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                        stage_line("P01-TOPOLOGY-EDGE", sample, &bytes);
                    }
                }
            } else if SAMPLE_MASK.load(Acquire) & (1 << (sample - 1)) != 0 {
                invalidate(INCOMPLETE);
            }
        }
        let mut heap_bytes = [0u8; 96];
        for (i, value) in [heap.epoch, heap.invalid as u64, heap.fit_proven as u64,
            heap.coverage_proven as u64, heap.live_objects, heap.requested_bytes,
            heap.attempt_objects, heap.kernel_objects, heap.outside_selected_objects,
            heap.unknown_objects, heap.unresolved_attempts,
            crate::memory::heap::p01_owner::CAPACITY as u64].into_iter().enumerate() {
            put64(&mut heap_bytes, i * 8, value);
        }
        line("P01-HEAP-ORIGIN", &heap_bytes);
        let mut heap_count = 0u64;
        let mut binding_count = 0u64;
        for index in 0..crate::memory::heap::p01_owner::CAPACITY {
            if let Some(entry) = crate::memory::heap::p01_owner::copy_entry(index) {
                let mut bytes = [0u8; 152];
                for (i, value) in entry.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                line("P01-HEAP-ENTRY", &bytes);
                if let Some(incarnation) = crate::memory::heap::p01_owner::copy_incarnation(index) {
                    let mut bytes = [0u8; 16];
                    for (i, value) in incarnation.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                    line("P01-HEAP-INCARNATION", &bytes);
                }
                heap_count += 1;
            }
        }
        for index in 0..crate::memory::heap::p01_owner::attempt_capacity() {
            if let Some(binding) = crate::memory::heap::p01_owner::copy_binding(index) {
                let mut bytes = [0u8; 48];
                for (i, value) in binding.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                line("P01-HEAP-BINDING", &bytes);
                binding_count += 1;
            }
        }
        let after = crate::memory::heap::p01_owner::snapshot(None);
        let mut heap_header = [0u8; 48];
        for (i, value) in [heap.epoch, after.epoch, after.invalid as u64,
            heap_count, binding_count, crate::memory::heap::p01_owner::attempt_capacity() as u64]
            .into_iter().enumerate() { put64(&mut heap_header, i * 8, value); }
        line("P01-HEAP-SOURCE", &heap_header);
        let accounting_after = crate::memory::heap::p01_owner::accounting();
        let mut bytes = [0u8; 64];
        for (i, value) in accounting_before.into_iter().chain(accounting_after).enumerate() {
            put64(&mut bytes, i * 8, value);
        }
        line("P01-HEAP-ACCOUNTING", &bytes);
        let before = crate::p01_retained::snapshot_before();
        let mut range_count = 0u64;
        for index in 0..crate::p01_retained::capacity() {
            if let Some(range) = crate::p01_retained::copy_entry(index) {
                let mut bytes = [0u8; 72];
                for (i, value) in [range.token, range.owner.tid, range.owner.root,
                    range.owner.cell, range.owner.generation, range.base, range.end,
                    range.reason as u64, range.origin_reason as u64].into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                line("P01-RETAINED-RANGE", &bytes);
                range_count += 1;
            }
        }
        let range_source = crate::p01_retained::snapshot_after(before);
        let mut range_header = [0u8; 48];
        for (i, value) in [range_source.epoch_before, range_source.epoch_after,
            range_source.status as u64, range_source.source_owner as u64,
            range_count, crate::p01_retained::capacity() as u64].into_iter().enumerate() {
            put64(&mut range_header, i * 8, value);
        }
        line("P01-RETAINED-SOURCE", &range_header);
        let before = crate::p01_retained::physical_snapshot_before();
        let mut physical_count = 0u64;
        let mut physical_live = 0u64;
        for index in 0..crate::p01_retained::physical_capacity() {
            if let Some(words) = crate::p01_retained::copy_physical_entry(index) {
                let mut bytes = [0u8; 96];
                for (i, value) in words.into_iter().enumerate() { put64(&mut bytes, i * 8, value); }
                line("P01-PHYSICAL-ENTRY", &bytes);
                physical_count += 1;
                if words[3] == 0 { physical_live += 1; }
            }
        }
        let physical_source = crate::p01_retained::physical_snapshot_after(before);
        // v1 supported sources: bitmap transitions, SAS map/prune, post-load
        // reserve failure. Gaps remain explicit: semantic owners for generic
        // frames, non-CPU/unpublished roots and global semantic table ownership.
        // These bytes never promote metric13 to a complete selected union.
        let mut physical_header = [0u8; 80];
        for (i, value) in [1, physical_source.epoch_before, physical_source.epoch_after,
            physical_source.status as u64, physical_source.source_owner as u64,
            physical_count, crate::p01_retained::physical_capacity() as u64,
            7, 7, physical_live].into_iter().enumerate() {
            put64(&mut physical_header, i * 8, value);
        }
        line("P01-PHYSICAL-SOURCE", &physical_header);
        for (i,peak) in crate::p01_resources::peaks().into_iter().enumerate() {
            let mut b=[0;32];put64(&mut b,0,peak.value);put64(&mut b,8,peak.epoch_before);put64(&mut b,16,peak.epoch_after);
            put64(&mut b,24,peak.status as u64|((peak.source_owner as u64)<<32));let mut out=[0;64];hex(&b,&mut out);
            log::warn!("P01-PEAK {} {}",i,core::str::from_utf8(&out).unwrap());
        }
        for (i,s) in SUBJECTS.iter().enumerate(){if !s.ready.load(Acquire){continue;}let b=words64(&s.words);let mut out=[0;128];hex(&b,&mut out);log::warn!("P01-SUBJECT {} {}",i,core::str::from_utf8(&out).unwrap());
            for (j,c) in COMPONENTS[i].iter().enumerate(){let b=words64(&c.words);hex(&b,&mut out);log::warn!("P01-COMPONENT {} {} {}",i,j+1,core::str::from_utf8(&out).unwrap());if !c.finalised.load(Acquire){invalidate(INCOMPLETE);}}
        }
        for(i,e)in OBLIGATIONS.iter().enumerate(){if e.state.load(Acquire)==0{continue;}let b=words64(&e.words);let mut out=[0;128];hex(&b,&mut out);log::warn!("P01-OBLIGATION {} {}",i,core::str::from_utf8(&out).unwrap());}
        for sample in 0..6 {if SAMPLE_MASK.load(Acquire)&(1<<sample)==0{continue;}log::warn!("P01-STAGE sample={} tick={}",sample+1,SAMPLE_TICKS[sample].load(Acquire));for metric in 0..16{let mut b=[0;24];for j in 0..3{put64(&mut b,j*8,SNAPSHOTS[sample][metric][j].load(Acquire));}let mut out=[0;48];hex(&b,&mut out);log::warn!("P01-SNAPSHOT {} {} {}",sample+1,metric,core::str::from_utf8(&out).unwrap());}}
    }
    let flags=FLAGS.load(Acquire);LAST_FLAGS.store(flags,Release);log::warn!("P01-FINAL reserved={} ready={} active={} flags={} state=closed",reserved,ready,active,flags);log::warn!("P01-LATE flags={}",flags);EXPORTED.store(true,Release);
    if env!("CELLOS_P01_FAULT")=="late"{invalidate(LATE);let flags=FLAGS.load(Acquire);log::warn!("P01-LATE flags={}",flags);LAST_FLAGS.store(flags,Release);}
    COLLECTOR.store(false,Release);
}
