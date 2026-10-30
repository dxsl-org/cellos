// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Shell-owned byte pipelines: children whose fd 0/1/2 are delegated stream
//! endpoints, never the keyboard owner.
//!
//! A [`Pipeline`] is a tree of kernel Operations below one root (a child of the
//! session Operation). The kernel cancels the Operation that holds a producer's
//! endpoint, and everything below it, when the last reader of a stream closes, and
//! an endpoint can only be delegated to a child prepared at or below the Operation
//! it lives in. The tree is laid out so that exactly the right things stop:
//!
//! - the root holds what the shell reads (output and diagnostics) and is never
//!   cancelled by a stage;
//! - stage `i` runs in its own stage Operation, so a stage that was stopped never
//!   takes a neighbour with it;
//! - the stream between stage `i` and `i + 1` lives in ring Operation `i`, and the
//!   ring Operations nest outward toward the last stage (`ring n-2` is a child of the
//!   root, `ring i` a child of `ring i + 1`); stage `i + 1` and every ring and stage
//!   upstream of it sit below `ring i`. Losing the consumer of a stream therefore
//!   cancels everything upstream of it (nothing is producing for nobody), never a
//!   stage downstream, which still reads what was written and ends normally;
//! - the stream the shell feeds stage 0 through lives in stage 0's own Operation.
//!
//! [`Pipeline::spawn`] prepares and commits one child inside its stage Operation
//! through the same `prepare_child -> delegate -> write_context -> commit`
//! sequence a foreground child uses, but without `authorize_input`, input-service
//! receipts or a foreground lease: the child can never own the terminal. Only the
//! VFS prepared-cwd reservation is bound, exactly as `spawn_foreground` does.
//!
//! Ownership: the caller keeps every endpoint copy it passes to `spawn` and
//! must close it afterwards (the child holds its own delegated copies). EOF at a
//! reader is the close of the last writer copy, so callers must close their
//! writer copies once the stages that need them are spawned. A reader passed as
//! stdin is moved to the child at commit (the kernel consumes the caller's copy).
use api::cell_session::{ChildRefV1,KernelOp,ObjectRefV1,Reader,Writer,Status,VfsOp,HEADER_LEN};
use api::service_binding::ServiceBinding;
use ostd::child_context::{ChildContext,Grant,GrantSlot};
use crate::stream::{Endpoint,EndpointRights,Operation,OperationRef,ReceiptRef,ok};

/// Maximum immutable-context argv / cwd / environment sizes (ABI limits).
const ARGV_MAX:usize=512;
const CWD_MAX:usize=4096;
const ENV_MAX:usize=4096;
const ENV_ENTRIES_MAX:usize=64;
const PATH_MAX:usize=4096;
/// Ring capacity of the empty stdin stream (the kernel minimum).
const PLACEHOLDER_CAPACITY:u32=64;
/// Ring capacity of every pipeline stream (one transfer unit is 512 bytes).
const STREAM_CAPACITY:u32=4096;
/// Most stages one pipeline may have (the shell's own pipeline limit).
pub const MAX_STAGES:usize=16;

/// Everything needed to launch one pipeline stage.
#[derive(Debug,Clone,Copy)]
pub struct PipeSpec<'a>{
    pub path:&'a str,
    pub argv:&'a[&'a str],
    /// Raw cwd bytes; empty means no cwd grant. Non-empty requires either
    /// `cwd_source` or a UTF-8 path the VFS can open.
    pub cwd:&'a[u8],
    pub environment:&'a[(&'a str,&'a str)],
    /// Existing VFS directory grant to derive the child's cwd from (never an
    /// ambient path lookup when present).
    pub cwd_source:Option<ObjectRefV1>,
    /// Give the stage a write-only completion-record stream in slot 5 (what a foreground
    /// child gets from `spawn_foreground_with_completion`): the stream is created in the
    /// pipeline's root Operation, so a stage stopped by the kernel never takes a record it
    /// already wrote away, and the reader end stays in the returned [`PipeChild`]
    /// ([`PipeChild::read_completion`]). The child takes it with [`crate::completion_writer`].
    pub completion:bool,
}

/// Pure pre-effect validation. Returns the exact immutable context length.
/// Size violations are `QuotaExceeded`, malformed bytes `Malformed`, a path-only
/// cwd that is not UTF-8 `UnsupportedInput` (same classes as `spawn_foreground`).
pub(crate) fn validate(spec:&PipeSpec<'_>)->Result<usize,Status>{
    if spec.path.is_empty()||spec.path.len()>PATH_MAX||spec.path.as_bytes().contains(&0){return Err(Status::Malformed)}
    let argv_len=spec.argv.iter().try_fold(7usize,|n,a|n.checked_add(a.len()+1)).ok_or(Status::Malformed)?;
    let env_len=spec.environment.iter().try_fold(0usize,|n,(k,v)|n.checked_add(4+k.len()+v.len())).ok_or(Status::Malformed)?;
    if argv_len>ARGV_MAX||spec.cwd.len()>CWD_MAX||env_len>ENV_MAX||spec.environment.len()>ENV_ENTRIES_MAX{return Err(Status::QuotaExceeded)}
    if spec.argv.iter().any(|a|a.as_bytes().contains(&0))||spec.cwd.contains(&0){return Err(Status::Malformed)}
    if !spec.cwd.is_empty()&&spec.cwd_source.is_none()&&core::str::from_utf8(spec.cwd).is_err(){return Err(Status::UnsupportedInput)}
    let grant_count=3+usize::from(!spec.cwd.is_empty())+usize::from(spec.completion);
    Ok(48+argv_len+spec.cwd.len()+env_len+grant_count*40)
}

/// Kernel child state: terminal record complete (`QueryChild` `state`).
const CHILD_TERMINAL:u32=3;
/// `QueryChild` result body after the result header: state, reason, exit code,
/// reserved, terminal sequence.
const CHILD_STATUS_LEN:usize=24;
/// Upper bound on one [`PipeChild::wait`]. A child that never reaches its terminal
/// record inside it yields `Status::TimedOut`; it is never reported as exited.
const WAIT_BUDGET_NS:u64=30_000_000_000;
/// Polls that only yield before the wait starts sleeping one tick per poll.
const WAIT_YIELDS:u32=8;

/// The kernel's terminal record of a committed child, as its preparer sees it.
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub struct ChildStatus{
    /// 1 live, 2 retiring, 3 terminal (the only state with a final `exit_code`).
    pub state:u32,
    pub reason:u32,
    pub exit_code:i32,
    pub terminal_sequence:u64,
}
impl ChildStatus{
    pub fn is_terminal(&self)->bool{self.state==CHILD_TERMINAL}
    /// Decode the 24-byte `QueryChild` body (after the 24-byte result header).
    /// Any other length, a nonzero reserved word or an unknown state is `Malformed`.
    pub(crate) fn decode(body:&[u8])->Result<Self,Status>{
        if body.len()!=CHILD_STATUS_LEN{return Err(Status::Malformed)}
        let mut r=Reader::new(body);
        let state=r.u32()?;let reason=r.u32()?;let exit_code=r.i32()?;let reserved=r.u32()?;let terminal_sequence=r.u64()?;
        r.finish()?;
        if reserved!=0||state>CHILD_TERMINAL{return Err(Status::Malformed)}
        Ok(Self{state,reason,exit_code,terminal_sequence})
    }
}

/// How [`PipeChild::wait`] spends the gap before poll number `attempt` (0-based).
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
enum Pause{Yield,Tick}
fn pause_for(attempt:u32)->Pause{if attempt<WAIT_YIELDS{Pause::Yield}else{Pause::Tick}}
/// Absolute deadline for a wait that starts at `now_ns`; `Malformed` on overflow.
fn wait_deadline(now_ns:u64)->Result<u64,Status>{now_ns.checked_add(WAIT_BUDGET_NS).ok_or(Status::Malformed)}

/// A committed pipeline stage. Keep it alive until the stage is reaped and drop it
/// before closing its stage Operation: it keeps a dynamically admitted cwd
/// directory open, and dropping it lets VFS settle the child's cwd receipt.
pub struct PipeChild{child:ChildRefV1,operation:OperationRef,_cwd:Option<CwdLease>,completion:Option<Endpoint>}
impl PipeChild{
    pub fn child(&self)->ChildRefV1{self.child}
    pub fn root_tid(&self)->usize{self.child.root_tid as usize}
    /// One `QueryChild` (non-mutating, idempotent): the kernel's current record of
    /// this child. Only the preparer may ask, and only until the pipeline Operation
    /// is closed. Kernel refusals surface as their own status (`WrongOwner`,
    /// `Stale`); a reply that is not exactly header plus 24 bytes is `Malformed`.
    pub fn query(&self)->Result<ChildStatus,Status>{query_child(self.operation,self.child)}
    /// Wait until the kernel's terminal record of the child exists and return its
    /// exit code. Polls [`PipeChild::query`] (a few yields, then one-tick sleeps)
    /// while the child is live or retiring, for at most 30 seconds. Never guesses:
    /// `Status::TimedOut` when the budget ends first (the child may still run, so
    /// cancel the pipeline operation if it must stop), `Status::Unsupported` when
    /// the clock is unavailable, `Stale`/`WrongOwner`/other kernel statuses as
    /// reported. Transient `WouldBlock`/`TimedOut` answers are polled again.
    /// Idempotent: the record stays readable until the operation is closed.
    pub fn wait(&self)->Result<i32,Status>{self.wait_status().map(|status|status.exit_code)}
    /// [`PipeChild::wait`] returning the whole terminal record: `reason` is nonzero
    /// when the kernel stopped the child (a cancelled Operation) instead of the
    /// child exiting by itself, and `exit_code` is then meaningless.
    pub fn wait_status(&self)->Result<ChildStatus,Status>{wait_child_status(self.operation,self.child)}
    /// Nonblocking drain of the stage's completion stream (see [`PipeSpec::completion`]):
    /// copies everything currently buffered, up to `out.len()`, and returns the byte count.
    /// Nothing buffered, writer end-of-stream, and a stage spawned without a completion
    /// stream all read as `Ok(0)`; only real stream errors fail. Read after the stage is
    /// reaped and before [`Pipeline::close`], which releases the stream with its Operation.
    pub fn read_completion(&self,out:&mut[u8])->Result<usize,Status>{
        let Some(reader)=self.completion else{return Ok(0)};
        let mut total=0;
        while total<out.len(){match reader.read(&mut out[total..],0){Ok(n)=>total+=n,Err(Status::WouldBlock|Status::Eof)=>break,Err(e)=>return Err(e)}}
        Ok(total)
    }
}

/// One `QueryChild` of `child` under its preparing `operation`.
pub(crate) fn query_child(operation:OperationRef,child:ChildRefV1)->Result<ChildStatus,Status>{
    let mut reply=[0u8;HEADER_LEN+CHILD_STATUS_LEN];
    let total=crate::stream::call(operation,KernelOp::QueryChild,false,&child.to_bytes(),&mut reply)?;
    ChildStatus::decode(reply.get(HEADER_LEN..total).ok_or(Status::Malformed)?)
}

/// Bounded poll of [`query_child`] until the terminal record exists (see [`PipeChild::wait`]).
pub(crate) fn wait_child_status(operation:OperationRef,child:ChildRefV1)->Result<ChildStatus,Status>{
    let deadline=wait_deadline(ostd::session::monotonic_ns()?)?;
    let mut attempt=0u32;
    loop{
        match query_child(operation,child){
            Ok(status) if status.is_terminal()=>return Ok(status),
            Ok(_)|Err(Status::WouldBlock|Status::TimedOut)=>{}
            Err(e)=>return Err(e),
        }
        if ostd::session::monotonic_ns()?>=deadline{return Err(Status::TimedOut)}
        match pause_for(attempt){
            Pause::Yield=>ostd::syscall::sys_yield(),
            Pause::Tick=>{let _=ostd::syscall::sys_set_timer(1);}
        }
        attempt=attempt.saturating_add(1);
    }
}

/// Where an Operation of a pipeline hangs.
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
enum Parent{Root,Ring(usize)}
/// The parent of every ring Operation (index = ring) and stage Operation (index =
/// stage) of a `stages`-stage pipeline. Ring `i` carries the stream from stage `i`
/// to stage `i + 1`; see the module documentation for why they nest outward.
#[derive(Debug,PartialEq,Eq)]
struct Plan{rings:alloc::vec::Vec<Parent>,stages:alloc::vec::Vec<Parent>}
fn layout_plan(stages:usize)->Plan{
    let rings=(0..stages.saturating_sub(1)).map(|ring|if ring+2==stages{Parent::Root}else{Parent::Ring(ring+1)}).collect();
    let stage_parents=(0..stages).map(|stage|if stages==1{Parent::Root}else{Parent::Ring(stage.saturating_sub(1))}).collect();
    Plan{rings,stages:stage_parents}
}

/// The root Operation of a pipeline and the ring and stage Operations below it.
pub struct Pipeline{operation:Operation,rings:alloc::vec::Vec<Operation>,stages:alloc::vec::Vec<Operation>,created:alloc::vec::Vec<Operation>}
impl Pipeline{
    /// One root Operation per pipeline, a child of the session operation.
    pub fn create(parent:OperationRef)->Result<Pipeline,Status>{
        Ok(Pipeline{operation:Operation::create(Some(parent))?,rings:alloc::vec::Vec::new(),stages:alloc::vec::Vec::new(),created:alloc::vec::Vec::new()})
    }
    pub fn operation(&self)->OperationRef{self.operation.reference()}
    /// `(reader, writer)` in the root Operation, the same order as
    /// `Operation::create_stream`: for the ends the shell reads (output and
    /// diagnostics). The root is never cancelled by a stage.
    pub fn stream(&self)->Result<(Endpoint,Endpoint),Status>{self.operation.create_stream(STREAM_CAPACITY)}
    /// Open the ring and stage Operations for `stages` stages (at least one, at most
    /// sixteen). Call once, before any stream or spawn.
    pub fn layout(&mut self,stages:usize)->Result<(),Status>{
        if stages==0||stages>MAX_STAGES||!self.stages.is_empty(){return Err(Status::Malformed)}
        let plan=layout_plan(stages);
        let mut rings:alloc::vec::Vec<Option<Operation>>=alloc::vec![None;plan.rings.len()];
        // Outermost ring first: ring `stages - 2` below the root, ring `i` below ring `i + 1`.
        for ring in (0..plan.rings.len()).rev(){
            let parent=match plan.rings[ring]{Parent::Root=>self.operation,Parent::Ring(outer)=>rings[outer].ok_or(Status::Malformed)?};
            let operation=Operation::create(Some(parent.reference()))?;
            self.created.push(operation);rings[ring]=Some(operation);
        }
        self.rings=rings.into_iter().flatten().collect();
        for parent in plan.stages{
            let parent=match parent{Parent::Root=>self.operation,Parent::Ring(ring)=>self.rings[ring]};
            let operation=Operation::create(Some(parent.reference()))?;
            self.created.push(operation);self.stages.push(operation);
        }
        Ok(())
    }
    /// `(reader, writer)` of the stream between stage `ring` and stage `ring + 1`,
    /// created in that ring's Operation.
    pub fn link(&self,ring:usize)->Result<(Endpoint,Endpoint),Status>{
        self.rings.get(ring).ok_or(Status::Malformed)?.create_stream(STREAM_CAPACITY)
    }
    /// `(reader, writer)` created inside stage `stage`'s own Operation: the stream
    /// the shell feeds that stage through. When the stage stops reading the kernel
    /// cancels only that stage's Operation.
    pub fn stage_stream(&self,stage:usize)->Result<(Endpoint,Endpoint),Status>{
        self.stages.get(stage).ok_or(Status::Malformed)?.create_stream(STREAM_CAPACITY)
    }
    /// Cancel the whole pipeline (root and everything below it).
    pub fn cancel(&self,reason:u32)->Result<(),Status>{self.operation.cancel(reason)}
    /// Release every Operation, children before parents, then the root (bounded retry
    /// on `WouldBlock`; correlation lanes are retired on success and on giving up).
    /// Drop every reaped [`PipeChild`] first: VFS settles a stage's cwd receipt, which
    /// its Operation's release waits for, only when the owner contacts it.
    /// The first failure is returned after every release was attempted.
    pub fn close(self)->Result<(),Status>{
        let mut first=Ok(());
        for operation in self.created.iter().rev(){
            if let Err(e)=close_bounded(operation.reference()){if first.is_ok(){first=Err(e)}}
        }
        let root=close_bounded(self.operation.reference());
        first.and(root)
    }

    /// Prepare and commit one child in stage Operation `stage` whose fd0/fd1/fd2 are
    /// delegated copies of the given endpoints (stdin with reader rights,
    /// stdout/stderr with writer rights). `None` for `stdin` gives the child an
    /// empty stream: its first read sees end of file.
    ///
    /// The caller keeps its own endpoint copies and must close them; this never
    /// closes a caller endpoint. Failure before commit publishes nothing (the
    /// prepared child is aborted and every endpoint made here is closed). A
    /// failure at or after commit may have published a child, so it cancels the
    /// stage Operation (reason 5, the `spawn_foreground` reason) and returns the
    /// error; the Operation itself is released by [`Pipeline::close`].
    pub fn spawn(&self,stage:usize,spec:&PipeSpec<'_>,stdin:Option<Endpoint>,stdout:Endpoint,stderr:Endpoint)->Result<PipeChild,Status>{
        let operation=*self.stages.get(stage).ok_or(Status::Malformed)?;
        let context_len=validate(spec)?;
        // The completion stream hangs from the root, which no stage can cancel.
        let completion=if spec.completion{Some(self.operation.create_stream(STREAM_CAPACITY)?)}else{None};
        let mut committing=false;
        let result=Self::prepare(operation,spec,context_len,(stdin,stdout,stderr),completion,&mut committing);
        if result.is_err(){
            if let Some((reader,writer))=completion{let _=reader.close();let _=writer.close();}
        }
        match result{
            Err(_) if committing=>{let _=operation.cancel(5);}
            // Before commit, a stage whose Operation the kernel cancelled lost its
            // consumer while it was being prepared (the cancellation aborts the
            // prepared child, so any step after it fails): report that as `Cancelled`.
            Err(status) if status!=Status::Cancelled&&operation.query().is_ok_and(|state|state.state!=0)=>return Err(Status::Cancelled),
            _=>{}
        }
        result
    }

    fn prepare(operation:Operation,spec:&PipeSpec<'_>,context_len:usize,stdio:(Option<Endpoint>,Endpoint,Endpoint),completion:Option<(Endpoint,Endpoint)>,committing:&mut bool)->Result<PipeChild,Status>{
        let (stdin,stdout,stderr)=stdio;
        // Endpoint copies owned by this call, declared before `prepared` so an
        // unpublished child is aborted first. `empty_reader` (the reader of the empty
        // stdin stream) outlives the commit; `delegated` is closed only while the
        // child is unpublished.
        let mut empty_reader=Closer::new();let mut delegated=Closer::new();
        let mut prepared=operation.prepare_child(spec.path,context_len)?;
        let stdin_source=match stdin{
            Some(e)=>e,
            None=>{
                // Closing the last reader of a stream stops its producers, so the
                // reader stays open until the child holds its own copy. The writer is
                // closed now: the child reads end of file at once.
                let(reader,writer)=operation.create_stream(PLACEHOLDER_CAPACITY)?;
                empty_reader.push(reader);
                writer.close()?;
                reader
            }
        };
        let stdin_copy=stdin_source.delegate(&prepared,EndpointRights::READ_ONLY)?;delegated.push(stdin_copy);
        let stdout_copy=stdout.delegate(&prepared,EndpointRights::WRITE_DELEGATE)?;delegated.push(stdout_copy);
        let stderr_copy=stderr.delegate(&prepared,EndpointRights::WRITE_DELEGATE)?;delegated.push(stderr_copy);
        // Completion stream: the writer is delegated write-only; the reader stays here.
        let completion_copy=match completion{
            Some((_,writer))=>{let copy=writer.delegate(&prepared,EndpointRights::WRITE_ONLY)?;delegated.push(copy);Some(copy)}
            None=>None,
        };

        let mut cwd_lease=None;let mut reservation=None;let mut child_cwd=None;
        let vfs_owner=if spec.cwd.is_empty(){None}else{Some(ostd::session::service_binding(api::syscall::service::VFS)?)};
        if let Some(owner)=vfs_owner{
            let lease=cwd_lease.insert(CwdLease::new(owner,operation.reference()));
            let source=match spec.cwd_source{
                Some(source)=>source,
                None=>{
                    let path=core::str::from_utf8(spec.cwd).map_err(|_|Status::UnsupportedInput)?;
                    // The dynamically opened directory is granted under the caller's
                    // own principal generation (held by every endpoint it owns).
                    let generation=stderr.inspect()?.holder.generation;
                    let mut send=[0u8;4096];let mut recv=[0u8;4096];
                    let request=api::ipc::VfsRequest::OpenRootDir{path};
                    let response=ostd::ipc::service_call_typed_bounded::<_,api::ipc::VfsResponse<'_>>(owner.tid as usize,&request,&mut send,&mut recv,100).map_err(|_|Status::Indeterminate)?;
                    let id=match response{api::ipc::VfsResponse::DirHandle(handle)=>handle.0,_=>return Err(Status::Denied)};
                    // From here the lease closes the handle on every exit.
                    lease.dir=Some(api::dir_handles::ViDirHandle(id));
                    if ostd::session::service_binding(api::syscall::service::VFS)?!=owner{return Err(Status::Stale)}
                    ObjectRefV1{id,generation}
                }
            };
            let mut body=[0u8;32];let mut w=Writer::new(&mut body);w.reference(prepared.reference().raw())?;w.reference(source)?;
            let mut reply=[0u8;56];let n=vfs(owner,operation.reference(),VfsOp::ReserveCwd,&body,&mut reply)?;
            let mut r=Reader::new(&reply[24..n]);reservation=Some(r.reference()?);child_cwd=Some(r.reference()?);r.finish()?;
        }

        let endpoint_grant=|slot:GrantSlot,copy:Endpoint,rights:EndpointRights|Grant::endpoint(slot,copy.reference().raw(),rights.bits() as u16);
        let mut grants=[endpoint_grant(GrantSlot::Stdin,stdin_copy,EndpointRights::READ_ONLY)?,endpoint_grant(GrantSlot::Stdout,stdout_copy,EndpointRights::WRITE_DELEGATE)?,endpoint_grant(GrantSlot::Stderr,stderr_copy,EndpointRights::WRITE_DELEGATE)?,Grant{kind:1,rights:0,slot:GrantSlot::Stdin,owner_service:0,object:ObjectRefV1::ZERO},Grant{kind:1,rights:0,slot:GrantSlot::Stdin,owner_service:0,object:ObjectRefV1::ZERO}];
        // Ascending slot order: stdio (0..=2), cwd (4), completion (5).
        let mut count=3;
        if let Some(grant)=child_cwd{grants[count]=Grant::cwd(grant)?;count+=1;}
        if let Some(copy)=completion_copy{grants[count]=endpoint_grant(GrantSlot::Completion,copy,EndpointRights::WRITE_ONLY)?;count+=1;}
        let context=ChildContext::new(operation.reference().raw(),spec.argv,spec.cwd,spec.environment,&grants[..count])?;
        if context.as_bytes().len()!=context_len{return Err(Status::Malformed)}
        prepared.write_context(&context)?;
        if let(Some(owner),Some(reservation))=(vfs_owner,reservation){
            let mut body=[0u8;32];let mut w=Writer::new(&mut body);w.reference(reservation)?;w.reference(prepared.reference().raw())?;
            let mut reply=[0u8;40];let n=vfs(owner,operation.reference(),VfsOp::FinalizeCwd,&body,&mut reply)?;
            let receipt=ReceiptRef::from_raw(ObjectRefV1::from_bytes(&reply[24..n])?)?;
            if let Some(lease)=cwd_lease.as_mut(){lease.receipt=Some(receipt);}
            prepared.bind_receipt(2,4,owner,receipt)?;
        }
        // From the commit call on, a child may be published (even when the call
        // reports an error); the delegated copies then belong to it.
        *committing=true;delegated.disarm();
        let child=prepared.commit()?;
        // The child holds its own reader of the empty stream now.
        empty_reader.close_all();
        // The child holds its own writer now: it is the sole completion writer.
        if let Some((_,writer))=completion{let _=writer.close();}
        Ok(PipeChild{child,operation:operation.reference(),_cwd:cwd_lease,completion:completion.map(|(reader,_)|reader)})
    }
}

/// Closes every pushed endpoint on drop or on demand; `disarm` forgets them.
struct Closer{list:[Option<Endpoint>;4],len:usize}
impl Closer{
    fn new()->Self{Self{list:[None;4],len:0}}
    fn push(&mut self,endpoint:Endpoint){if self.len<self.list.len(){self.list[self.len]=Some(endpoint);self.len+=1;}}
    fn close_all(&mut self){for slot in self.list.iter_mut().take(self.len){if let Some(endpoint)=slot.take(){let _=endpoint.close();}}self.len=0;}
    fn disarm(&mut self){self.list=[None;4];self.len=0;}
}
impl Drop for Closer{fn drop(&mut self){self.close_all()}}

fn vfs(owner:ServiceBinding,operation:OperationRef,op:VfsOp,body:&[u8],reply:&mut[u8])->Result<usize,Status>{
    let header=ostd::session::service_call(owner,api::syscall::service::VFS,operation.raw(),op as u16,true,body,reply)?;ok(header.status)?;Ok(header.total_len as usize)
}

/// CloseOperation retried while `WouldBlock` (receipt owners settle receipts
/// asynchronously): eight yields, then one-tick sleeps, 64 attempts at most.
/// Correlation lanes are retired on success and on giving up.
fn close_bounded(operation:OperationRef)->Result<(),Status>{
    let mut last=Status::WouldBlock;
    for attempt in 0..64u32{
        match Operation::from_ref(operation).close(){
            Ok(())=>return Ok(()),
            Err(Status::WouldBlock)=>{ostd::syscall::sys_yield();if attempt>=8{let _=ostd::syscall::sys_set_timer(1);}}
            Err(e)=>{last=e;break}
        }
    }
    ostd::session::retire_operation(operation.raw());
    Err(last)
}

/// The owner's side of a child's VFS cwd reservation, dropped once the child is
/// reaped. VFS does not poll a running child that never contacts it: it settles an
/// owner's reservations when that owner sends a request, and closing the child's
/// Operation waits for the settled receipt. Dropping the lease is that request: it
/// closes the dynamically admitted source directory, or else queries the receipt
/// (`ResolveCwd`, action 0). Cleanup only; VFS still validates every handle operation.
pub(crate) struct CwdLease{owner:ServiceBinding,operation:OperationRef,pub(crate) dir:Option<api::dir_handles::ViDirHandle>,pub(crate) receipt:Option<ReceiptRef>}
impl CwdLease{pub(crate) fn new(owner:ServiceBinding,operation:OperationRef)->Self{Self{owner,operation,dir:None,receipt:None}}}
impl Drop for CwdLease{fn drop(&mut self){
    if ostd::session::service_binding(api::syscall::service::VFS).ok()!=Some(self.owner){return}
    if let Some(dir)=self.dir{
        let mut send=[0u8;4096];let mut recv=[0u8;4096];
        let _=ostd::ipc::service_call_typed_bounded::<_,api::ipc::VfsResponse<'_>>(self.owner.tid as usize,&api::ipc::VfsRequest::CloseDir{dir},&mut send,&mut recv,100);
    }else if let Some(receipt)=self.receipt{
        let mut body=[0u8;24];let mut w=Writer::new(&mut body);
        if w.reference(receipt.raw()).and_then(|()|w.u32(0)).and_then(|()|w.u32(0)).is_ok(){
            let _=ostd::session::service_call(self.owner,api::syscall::service::VFS,self.operation.raw(),VfsOp::ResolveCwd as u16,false,&body,&mut[0u8;HEADER_LEN+24]);
        }
    }
}}

#[cfg(test)]
mod tests{
    use super::*;
    fn spec<'a>(argv:&'a[&'a str],cwd:&'a[u8],environment:&'a[(&'a str,&'a str)])->PipeSpec<'a>{
        PipeSpec{path:"/bin/cat",argv,cwd,environment,cwd_source:None,completion:false}
    }
    #[test]
    fn context_length_matches_the_session_formula(){
        // 48 + (7 + 8 + 4) + 0 + 0 + 3 grants.
        assert_eq!(validate(&spec(&["cat","-n"],b"",&[])),Ok(48+7+4+3+3*40));
        // cwd adds its bytes and one grant; env counts 4 + key + value.
        assert_eq!(validate(&spec(&["a"],b"/tmp",&[("K","vv")])),Ok(48+7+2+4+(4+1+2)+4*40));
    }
    #[test]
    fn the_completion_stream_adds_exactly_one_grant(){
        let mut with=spec(&["ssh","h"],b"",&[]);
        let without=with;
        with.completion=true;
        let plain=validate(&without).unwrap();
        assert_eq!(validate(&with),Ok(plain+40));
        // Together with a cwd: two extra grants over the three stdio ones.
        let mut both=spec(&["ssh","h"],b"/tmp",&[]);
        let cwd_only=validate(&both).unwrap();
        both.completion=true;
        assert_eq!(validate(&both),Ok(cwd_only+40));
        assert_eq!(cwd_only,48+7+4+2+4+4*40);
    }
    #[test]
    fn argv_over_512_bytes_is_refused(){
        let largest="x".repeat(512-7-1);
        assert!(validate(&spec(&[largest.as_str()],b"",&[])).is_ok());
        let over="x".repeat(512-7);
        assert_eq!(validate(&spec(&[over.as_str()],b"",&[])),Err(Status::QuotaExceeded));
        let many=["0123456789abcdef";32];
        assert_eq!(validate(&spec(&many,b"",&[])),Err(Status::QuotaExceeded));
    }
    #[test]
    fn environment_over_4096_bytes_or_64_entries_is_refused(){
        let value="v".repeat(4096-4-1);
        assert!(validate(&spec(&["a"],b"",&[("k",value.as_str())])).is_ok());
        let over="v".repeat(4096-4);
        assert_eq!(validate(&spec(&["a"],b"",&[("k",over.as_str())])),Err(Status::QuotaExceeded));
        let entries=[("k","");64];
        assert!(validate(&spec(&["a"],b"",&entries)).is_ok());
        let too_many=[("k","");65];
        assert_eq!(validate(&spec(&["a"],b"",&too_many)),Err(Status::QuotaExceeded));
    }
    #[test]
    fn cwd_over_4096_bytes_is_refused(){
        let cwd=std::vec![b'/';4096];
        assert!(validate(&spec(&["a"],&cwd,&[])).is_ok());
        let over=std::vec![b'/';4097];
        assert_eq!(validate(&spec(&["a"],&over,&[])),Err(Status::QuotaExceeded));
    }
    #[test]
    fn maximum_context_fits_the_prepare_child_window(){
        let argv="x".repeat(512-7-1);
        let value="v".repeat(4096-4-1);
        let cwd=std::vec![b'/';4096];
        let len=validate(&spec(&[argv.as_str()],&cwd,&[("k",value.as_str())])).unwrap();
        assert!((48..=9072).contains(&len));
    }
    #[test]
    fn nul_bytes_and_empty_or_oversize_paths_are_malformed(){
        assert_eq!(validate(&spec(&["a\0b"],b"",&[])),Err(Status::Malformed));
        assert_eq!(validate(&spec(&["a"],b"/a\0b",&[])),Err(Status::Malformed));
        let mut bad=spec(&["a"],b"",&[]);
        bad.path="";assert_eq!(validate(&bad),Err(Status::Malformed));
        bad.path="/bin/a\0";assert_eq!(validate(&bad),Err(Status::Malformed));
        let long="/".repeat(4097);bad.path=long.as_str();assert_eq!(validate(&bad),Err(Status::Malformed));
    }
    #[test]
    fn non_utf8_cwd_needs_an_explicit_source(){
        assert_eq!(validate(&spec(&["a"],b"/\xff",&[])),Err(Status::UnsupportedInput));
        let mut with_source=spec(&["a"],b"/\xff",&[]);
        with_source.cwd_source=Some(ObjectRefV1{id:1,generation:1});
        assert!(validate(&with_source).is_ok());
    }
    /// Whether `node` hangs (at any depth) below ring `ring`.
    fn below_ring(plan:&Plan,mut parent:Parent,ring:usize)->bool{
        loop{
            match parent{
                Parent::Root=>return false,
                Parent::Ring(at) if at==ring=>return true,
                Parent::Ring(at)=>parent=plan.rings[at],
            }
        }
    }
    #[test]
    fn a_lone_stage_hangs_from_the_root(){
        assert_eq!(layout_plan(1),Plan{rings:std::vec![],stages:std::vec![Parent::Root]});
    }
    #[test]
    fn rings_nest_outward_toward_the_last_stage(){
        let plan=layout_plan(4);
        // Ring 2 (stream into the last stage) hangs from the root, ring 1 from ring 2, ring 0 from ring 1.
        assert_eq!(plan.rings,std::vec![Parent::Ring(1),Parent::Ring(2),Parent::Root]);
        assert_eq!(plan.stages,std::vec![Parent::Ring(0),Parent::Ring(0),Parent::Ring(1),Parent::Ring(2)]);
    }
    #[test]
    fn losing_a_consumer_cancels_only_what_is_upstream_of_it(){
        for stages in 2..=MAX_STAGES{
            let plan=layout_plan(stages);
            for ring in 0..stages-1{
                for stage in 0..stages{
                    // Cancelling ring `ring` (consumer stage `ring + 1` lost its reader) stops the
                    // consumer and every stage before it, never a stage after it.
                    assert_eq!(below_ring(&plan,plan.stages[stage],ring),stage<=ring+1,"{stages} stages, ring {ring}, stage {stage}");
                }
                // No ring below it is a later one: nothing downstream hangs from this ring.
                for outer in ring+1..stages-1{assert!(!below_ring(&plan,plan.rings[outer],ring))}
            }
        }
    }
    #[test]
    fn every_stage_is_below_every_ring_it_touches_and_the_root_is_never_cancelled_by_one(){
        for stages in 2..=MAX_STAGES{
            let plan=layout_plan(stages);
            for stage in 0..stages{
                // A stage reads ring `stage - 1` and writes ring `stage`: delegation needs it below both.
                if stage>0{assert!(below_ring(&plan,plan.stages[stage],stage-1))}
                if stage+1<stages{assert!(below_ring(&plan,plan.stages[stage],stage))}
            }
            // Every ring chain ends at the root, so no cancelled ring can reach it.
            assert_eq!(plan.rings.iter().filter(|p|**p==Parent::Root).count(),1);
        }
    }
    #[test]
    fn the_largest_pipeline_needs_thirty_one_operations_below_its_root(){
        let plan=layout_plan(MAX_STAGES);
        assert_eq!(plan.stages.len(),MAX_STAGES);
        assert_eq!(plan.rings.len(),MAX_STAGES-1);
    }
    fn status_body(state:u32,reason:u32,exit_code:i32,reserved:u32,sequence:u64)->[u8;24]{
        let mut body=[0u8;24];let mut w=Writer::new(&mut body);
        w.u32(state).unwrap();w.u32(reason).unwrap();w.i32(exit_code).unwrap();w.u32(reserved).unwrap();w.u64(sequence).unwrap();
        body
    }
    #[test]
    fn child_status_decodes_the_24_byte_record(){
        let body=status_body(3,5,-7,0,0x0102_0304_0506_0708);
        // Fixed little-endian layout: state, reason, exit code, reserved, sequence.
        assert_eq!(&body[..12],&[3,0,0,0,5,0,0,0,0xf9,0xff,0xff,0xff]);
        assert_eq!(&body[16..],&[8,7,6,5,4,3,2,1]);
        let status=ChildStatus::decode(&body).unwrap();
        assert_eq!(status,ChildStatus{state:3,reason:5,exit_code:-7,terminal_sequence:0x0102_0304_0506_0708});
        assert!(status.is_terminal());
        assert!(!ChildStatus::decode(&status_body(1,0,0,0,0)).unwrap().is_terminal());
        assert!(!ChildStatus::decode(&status_body(2,0,0,0,0)).unwrap().is_terminal());
    }
    #[test]
    fn child_status_refuses_malformed_records(){
        assert_eq!(ChildStatus::decode(&status_body(3,0,0,1,0)),Err(Status::Malformed));
        assert_eq!(ChildStatus::decode(&status_body(4,0,0,0,0)),Err(Status::Malformed));
        let body=status_body(3,0,0,0,0);
        assert_eq!(ChildStatus::decode(&body[..23]),Err(Status::Malformed));
        assert_eq!(ChildStatus::decode(&[]),Err(Status::Malformed));
        let mut long=std::vec::Vec::from(body);long.push(0);
        assert_eq!(ChildStatus::decode(&long),Err(Status::Malformed));
    }
    #[test]
    fn wait_pacing_yields_first_then_sleeps(){
        for attempt in 0..WAIT_YIELDS{assert_eq!(pause_for(attempt),Pause::Yield)}
        assert_eq!(pause_for(WAIT_YIELDS),Pause::Tick);
        assert_eq!(pause_for(u32::MAX),Pause::Tick);
    }
    #[test]
    fn wait_deadline_is_bounded_and_overflow_checked(){
        assert_eq!(wait_deadline(0),Ok(WAIT_BUDGET_NS));
        assert_eq!(wait_deadline(10),Ok(10+WAIT_BUDGET_NS));
        assert_eq!(wait_deadline(u64::MAX),Err(Status::Malformed));
    }
}
