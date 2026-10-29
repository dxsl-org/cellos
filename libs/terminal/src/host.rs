// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Frontend host (SessionV1 selectors 23–28, Phase 05 S5-3 annex option C): a Cell that
//! draws a terminal (the GUI Terminal, the private Phase04 driver) launches a shell that
//! is the **controller of its own session** and attaches to it only as a frontend.
//!
//! The host never creates or owns a session. It
//! 1. creates the two output rings it drains under its holder operation (`open`);
//! 2. per launch, creates a shell operation, registers an input-owned frontend
//!    capability (`RegisterFrontend`) under it, prepares the shell under that same
//!    operation with the stdout/stderr *writer* ends (kind 1, slots 1/2) and the frontend
//!    (kind 6, slot 8) as context grants, binds the frontend to exactly that prepared
//!    child (`BindFrontendChild`), binds the input receipt to the kernel's kind-6 slot and
//!    commits;
//! 3. waits (`QueryFrontend`) until the shell called `CreateForFrontend` and the frontend
//!    is Attached, then feeds normalized input through the returned attachment
//!    (`SubmitEvents`/paste/`SubmitRaw`/`EndInput`/`Resize`, expected epoch 0: the input
//!    owner substitutes the session's current epoch for a frontend attachment);
//! 4. drains the output rings (the shell relays its own and its children's output there);
//! 5. may release the frontend (`ReleaseFrontend`: the shell keeps a Detached session) and
//!    re-attach a fresh frontend with the holder-bound `resume` capability
//!    (`ReattachFrontend`), or close (cancel the shell, release the operation).
//!
//! No GUI/viUI dependency: callers render the drained bytes. Every method is
//! non-blocking except [`FrontendHost::launch`]/[`FrontendHost::reattach`] (bounded owner
//! RPCs), [`FrontendHost::wait_attached`] (caller-bounded) and [`FrontendHost::close`]
//! (bounded reaping).
use api::cell_session::frontend::{
    BindFrontendChildV1, FrontendStatusV1, ReattachFrontendV1, ReattachResultV1, RegisterFrontendV1,
    FRONTEND_KIND_GUI, FRONTEND_STATE_ATTACHED, FRONTEND_STATE_BOUND, FRONTEND_STATE_CLOSED,
    FRONTEND_STATE_LOST, FRONTEND_STATE_PENDING,
};
use api::cell_session::{ChildRefV1,InputOp,KernelOp,ObjectRefV1,Reader,Writer,GRANT_KIND_INPUT_FRONTEND,SLOT_FRONTEND,HEADER_LEN};
use api::service_binding::ServiceBinding;
use ostd::child_context::{ChildContext,Grant,GrantSlot};
use crate::stream::{call,ok,Endpoint,EndpointRights,Operation,ReceiptRef};
use crate::Status;

/// Largest chunk one `drain_*` call can return (one stream read frame).
pub const DRAIN_MAX:usize=512;
/// Capacity of each output ring the host creates for the shell.
const RING_CAPACITY:u32=4096;
/// Attempts (yield, then one-tick sleeps) before reaping a cancelled shell gives up.
const REAP_ATTEMPTS:u32=256;
/// Attempts that only yield before reaping starts sleeping one tick per attempt.
const REAP_YIELDS:u32=8;
/// `CancelOperation` reason used for every host-initiated cancellation.
const CANCEL_REASON:u32=2;

/// Result of [`FrontendHost::poll_child`].
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ChildPoll{Running,Exited}

/// The frontend as the host last observed it.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FrontendState{
    /// Registered, no shell bound yet.
    Pending,
    /// Bound to the committed shell; the shell has not created its session yet.
    Bound,
    /// The shell's session accepts input through this host's attachment.
    Attached,
    /// Released by this host; the shell keeps its session and `reattach` may resume it.
    Detached,
    /// The shell (the controller) closed its session or ended.
    Closed,
    /// The input owner reports the frontend lost.
    Lost,
}

/// Session and attachment selectors of an Attached frontend.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct Link{session:ObjectRefV1,attachment:ObjectRefV1}

/// Map one `QueryFrontend` answer to the host state and, when Attached, the link and
/// the current `resume` capability.
pub(crate) fn frontend_state(status:&FrontendStatusV1)->Result<(FrontendState,Option<(Link,ObjectRefV1)>),Status>{
    Ok(match status.state{
        FRONTEND_STATE_PENDING=>(FrontendState::Pending,None),
        FRONTEND_STATE_BOUND=>(FrontendState::Bound,None),
        FRONTEND_STATE_ATTACHED=>{
            if status.session.is_zero()||status.attachment.is_zero()||status.resume.is_zero(){return Err(Status::Malformed)}
            (FrontendState::Attached,Some((Link{session:status.session,attachment:status.attachment},status.resume)))
        }
        FRONTEND_STATE_CLOSED=>(FrontendState::Closed,None),
        FRONTEND_STATE_LOST=>(FrontendState::Lost,None),
        _=>return Err(Status::Malformed),
    })
}

/// Input needs a live attachment: a closed host is revoked, a host without one (not yet
/// attached, or released) has nothing to deliver to yet.
pub(crate) fn require_link(closed:bool,link:Option<Link>)->Result<Link,Status>{
    if closed{return Err(Status::Revoked)}
    link.ok_or(Status::WouldBlock)
}

/// `SubmitEvents` body carrying one normalized event through a frontend attachment.
/// The expected epoch is 0: the owner substitutes the session's current epoch.
pub(crate) fn events_body(link:Link,kind:u16,payload:&[u8],out:&mut[u8])->Result<usize,Status>{
    if payload.len()>512{return Err(Status::Malformed)}
    let mut w=Writer::new(out);w.reference(link.session)?;w.reference(link.attachment)?;w.u64(0)?;w.u16(1)?;w.u16(0)?;w.u32((24+payload.len()) as u32)?;
    w.u64(0)?;w.u64(0)?;w.u16(kind)?;w.u16(payload.len() as u16)?;w.u32(0)?;w.bytes(payload)?;Ok(w.position())
}

/// Decode a `QueryChild` result body: `state:u32, reason:u32, exit_code:i32,
/// reserved:u32=0, terminal_sequence:u64`. `Exited` carries the exit code of a child that
/// ended by itself (reason 0) and `None` for a cancelled or faulted one.
pub(crate) fn child_state(body:&[u8])->Result<(ChildPoll,Option<i32>),Status>{
    let mut r=Reader::new(body);let state=r.u32()?;let reason=r.u32()?;let code=r.i32()?;if r.u32()?!=0{return Err(Status::Malformed)}let _sequence=r.u64()?;r.finish()?;
    match state{1|2=>Ok((ChildPoll::Running,None)),3=>Ok((ChildPoll::Exited,if reason==0{Some(code)}else{None})),_=>Err(Status::Malformed)}
}

/// Non-blocking drain: "nothing yet" and a closed pipe both read as zero bytes;
/// a count the stream could not have produced is a protocol error.
pub(crate) fn classify_drain(result:Result<usize,Status>,capacity:usize)->Result<usize,Status>{
    match result{
        Ok(n) if n<=capacity.min(DRAIN_MAX)=>Ok(n),
        Ok(_)=>Err(Status::Malformed),
        Err(Status::WouldBlock|Status::TimedOut|Status::Eof)=>Ok(0),
        Err(e)=>Err(e),
    }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum ReapStep{Done,Retry{sleep:bool},Fail(Status)}
/// One iteration of a bounded wait (`attempt` is zero based): `Ok(true)` is done,
/// `Ok(false)` and `WouldBlock`/`TimedOut` retry.
pub(crate) fn reap_step(attempt:u32,result:Result<bool,Status>)->ReapStep{
    match result{
        Ok(true)=>ReapStep::Done,
        Ok(false)|Err(Status::WouldBlock|Status::TimedOut)=>if attempt+1>=REAP_ATTEMPTS{ReapStep::Fail(Status::TimedOut)}else{ReapStep::Retry{sleep:attempt>=REAP_YIELDS}},
        Err(e)=>ReapStep::Fail(e),
    }
}

/// Exact context length of the shell launch (argv/environment encodings of
/// `ChildContext`, no cwd, three grants), computed before any reference exists.
pub(crate) fn launch_context_len(argv:&[&str],environment:&[(&str,&str)])->Result<usize,Status>{
    let argv_len=argv.iter().try_fold(7usize,|n,a|n.checked_add(a.len()+1)).ok_or(Status::Malformed)?;
    let env_len=environment.iter().try_fold(0usize,|n,(k,v)|n.checked_add(4+k.len()+v.len())).ok_or(Status::Malformed)?;
    if argv_len>512||env_len>4096||environment.len()>64{return Err(Status::QuotaExceeded)}
    Ok(48+argv_len+env_len+3*40)
}

/// The shell's grants: the two output writers it relays to and the frontend it creates
/// its session for. No stdin/control: the shell's input is its own session.
pub(crate) fn launch_grants(stdout:ObjectRefV1,stderr:ObjectRefV1,frontend:ObjectRefV1)->Result<[Grant;3],Status>{
    let writer=EndpointRights::WRITE_DELEGATE.bits() as u16;
    Ok([Grant::endpoint(GrantSlot::Stdout,stdout,writer)?,Grant::endpoint(GrantSlot::Stderr,stderr,writer)?,Grant::frontend(frontend)?])
}

/// One launched shell: its own operation (child of the holder operation, registered with
/// its frontend: the input owner proves holder liveness through it) and its child.
struct Shell{operation:Operation,child:Option<ChildRefV1>,exit:Option<(ChildPoll,Option<i32>)>}

/// Frontend host: one holder operation with the two output rings, at most one shell (and
/// its frontend) at a time.
pub struct FrontendHost{
    operation:Operation,owner:ServiceBinding,rows:u16,cols:u16,
    output:Endpoint,diagnostic:Endpoint,writers:(Endpoint,Endpoint),
    shell:Option<Shell>,frontend:Option<ObjectRefV1>,
    link:Option<Link>,resume:Option<ObjectRefV1>,state:FrontendState,closed:bool,
}

impl FrontendHost{
    /// Create the holder operation and the output rings for a `rows`×`cols` terminal.
    /// Nothing is registered or launched yet.
    pub fn open(rows:u16,cols:u16)->Result<Self,Status>{
        api::services::session::validate_dimensions(rows,cols)?;
        let operation=Operation::create(None)?;
        let result=(||{
            let owner=ostd::session::service_binding(api::syscall::service::INPUT)?;
            let (output,stdout_writer)=operation.create_stream(RING_CAPACITY)?;let (diagnostic,stderr_writer)=operation.create_stream(RING_CAPACITY)?;
            Ok(Self{operation,owner,rows,cols,output,diagnostic,writers:(stdout_writer,stderr_writer),shell:None,frontend:None,link:None,resume:None,state:FrontendState::Pending,closed:false})
        })();
        if result.is_err(){let _=operation.cancel(5);let _=crate::session::close_operation_bounded(operation.reference());}
        result
    }

    /// Every holder request is correlated under the shell operation its frontend was
    /// registered with (the input owner fences frontend requests there).
    fn input(&self,op:InputOp,body:&[u8],reply:&mut[u8])->Result<usize,Status>{
        let shell=self.shell.as_ref().ok_or(Status::NotFound)?;
        // QueryFrontend is a read: the owner keeps no outcome for it.
        let mutating=op!=InputOp::QueryFrontend;
        let h=ostd::session::service_call(self.owner,api::syscall::service::INPUT,shell.operation.reference().raw(),op as u16,mutating,body,reply)?;ok(h.status)?;Ok(h.total_len as usize)
    }

    fn register(&self,rows:u16,cols:u16)->Result<ObjectRefV1,Status>{
        let shell=self.shell.as_ref().ok_or(Status::NotFound)?;
        let mut body=[0u8;RegisterFrontendV1::LEN];RegisterFrontendV1{operation:shell.operation.reference().raw(),kind:FRONTEND_KIND_GUI,rows,cols}.encode(&mut body)?;
        let mut reply=[0u8;HEADER_LEN+16];let n=self.input(InputOp::RegisterFrontend,&body,&mut reply)?;let frontend=ObjectRefV1::from_bytes(&reply[HEADER_LEN..n])?;
        if frontend.is_zero(){return Err(Status::Malformed)}Ok(frontend)
    }

    /// Register a frontend and launch `path` (the shell) bound to it, under a fresh shell
    /// operation. One live shell per host: a previous shell must have been observed
    /// exited ([`Self::poll_child`]); its operation is released first. The shell starts
    /// without a cwd grant (a cwd grant would path-seal it).
    pub fn launch(&mut self,path:&str,argv:&[&str],environment:&[(&str,&str)])->Result<(),Status>{
        if self.closed{return Err(Status::Revoked)}
        if let Some(previous)=self.shell.as_ref(){
            if previous.child.is_some()&&previous.exit.is_none(){return Err(Status::QuotaExceeded)}
            self.retire_shell()?;
        }
        let operation=Operation::create(Some(self.operation.reference()))?;
        self.shell=Some(Shell{operation,child:None,exit:None});
        let result=self.launch_shell(path,argv,environment);
        if result.is_err(){let _=operation.cancel(5);let _=self.retire_shell();}
        result
    }

    fn launch_shell(&mut self,path:&str,argv:&[&str],environment:&[(&str,&str)])->Result<(),Status>{
        let operation=self.shell.as_ref().ok_or(Status::NotFound)?.operation;
        let frontend=self.register(self.rows,self.cols)?;self.frontend=Some(frontend);self.state=FrontendState::Pending;
        let mut prepared=operation.prepare_child(path,launch_context_len(argv,environment)?)?;
        let stdout=self.writers.0.delegate(&prepared,EndpointRights::WRITE_DELEGATE)?;let stderr=self.writers.1.delegate(&prepared,EndpointRights::WRITE_DELEGATE)?;
        let grants=launch_grants(stdout.reference().raw(),stderr.reference().raw(),frontend)?;
        let context=ChildContext::new(operation.reference().raw(),argv,b"",environment,&grants)?;prepared.write_context(&context)?;
        let mut body=[0u8;BindFrontendChildV1::LEN];BindFrontendChildV1{frontend,child:prepared.child()}.encode(&mut body)?;
        let mut reply=[0u8;HEADER_LEN+16];let n=self.input(InputOp::BindFrontendChild,&body,&mut reply)?;let receipt=ReceiptRef::from_raw(ObjectRefV1::from_bytes(&reply[HEADER_LEN..n])?)?;
        prepared.bind_receipt(GRANT_KIND_INPUT_FRONTEND,SLOT_FRONTEND,self.owner,receipt)?;
        let child=prepared.commit()?;
        if let Some(shell)=self.shell.as_mut(){shell.child=Some(child);}
        self.state=FrontendState::Bound;Ok(())
    }

    /// Release an ended (or never committed) shell's frontend and operation. The input
    /// owner keeps a frontend record (and its kind-6 receipt, which pins the shell
    /// operation) until the holder releases it or its operation ends, so a still
    /// registered frontend is released first (best effort: an owner that already forgot
    /// it answers Stale).
    fn retire_shell(&mut self)->Result<(),Status>{
        if self.shell.is_none(){return Ok(())}
        if let Some(frontend)=self.frontend{let _=self.input(InputOp::ReleaseFrontend,&frontend.to_bytes(),&mut[0u8;HEADER_LEN]);}
        let Some(shell)=self.shell.take()else{return Ok(())};
        self.frontend=None;self.link=None;self.resume=None;self.state=FrontendState::Pending;
        crate::session::close_operation_bounded(shell.operation.reference())
    }

    /// One `QueryFrontend`; caches the attachment and the current `resume` capability.
    pub fn poll_frontend(&mut self)->Result<FrontendState,Status>{
        if self.closed{return Err(Status::Revoked)}
        if self.state==FrontendState::Detached{return Ok(FrontendState::Detached)}
        let frontend=self.frontend.ok_or(Status::Stale)?;
        let mut reply=[0u8;HEADER_LEN+FrontendStatusV1::LEN];let n=self.input(InputOp::QueryFrontend,&frontend.to_bytes(),&mut reply)?;
        let (state,attached)=frontend_state(&FrontendStatusV1::decode(&reply[HEADER_LEN..n])?)?;
        match attached{Some((link,resume))=>{self.link=Some(link);self.resume=Some(resume);}None=>{self.link=None;if matches!(state,FrontendState::Closed|FrontendState::Lost){self.resume=None;}}}
        self.state=state;Ok(state)
    }

    /// Poll until the shell attached this frontend. `Revoked` when the frontend closed or
    /// was lost, `Eof` when the shell ended before attaching, `TimedOut` after `timeout_ms`.
    pub fn wait_attached(&mut self,timeout_ms:u64)->Result<(),Status>{
        let end=ostd::session::monotonic_ns()?.saturating_add(timeout_ms.saturating_mul(1_000_000));let mut spins=0u32;
        loop{
            match self.poll_frontend()?{
                FrontendState::Attached=>return Ok(()),
                FrontendState::Closed|FrontendState::Lost|FrontendState::Detached=>return Err(Status::Revoked),
                FrontendState::Pending|FrontendState::Bound=>{}
            }
            spins=spins.wrapping_add(1);
            if spins%16==0&&self.poll_child()?==ChildPoll::Exited{return Err(Status::Eof)}
            if ostd::session::monotonic_ns()?>=end{return Err(Status::TimedOut)}
            let _=ostd::syscall::sys_set_timer(1);
        }
    }

    pub fn state(&self)->FrontendState{self.state}
    pub fn rows(&self)->u16{self.rows}pub fn cols(&self)->u16{self.cols}
    /// The shell's session selector while Attached (evidence and negative tests only; the
    /// input owner authenticates every use).
    pub fn session(&self)->Option<ObjectRefV1>{self.link.map(|l|l.session)}
    /// The current attachment selector while Attached.
    pub fn attachment(&self)->Option<ObjectRefV1>{self.link.map(|l|l.attachment)}
    /// The shell's committed child reference once launched.
    pub fn child(&self)->Option<ChildRefV1>{self.shell.as_ref().and_then(|s|s.child)}
    /// The current shell's operation (the frontend's registered operation): the
    /// correlation selector of holder requests, for evidence and negative tests only.
    pub fn shell_operation(&self)->Option<crate::stream::OperationRef>{self.shell.as_ref().map(|s|s.operation.reference())}

    fn link(&self)->Result<Link,Status>{require_link(self.closed,self.link)}

    pub fn submit_key(&mut self,code:u32,modifiers:u32,pressed:u8)->Result<(),Status>{
        let mut payload=ostd::session::Scrub::<16>::new();payload[..4].copy_from_slice(&code.to_le_bytes());payload[4..8].copy_from_slice(&modifiers.to_le_bytes());payload[8]=pressed;
        api::services::session::validate_event(1,&payload)?;self.submit_event(1,&payload)
    }
    pub fn submit_text(&mut self,text:&str)->Result<(),Status>{
        api::services::session::validate_text(text.as_bytes())?;if text.is_empty()||text.len()>512{return Err(Status::Malformed)}self.submit_event(2,text.as_bytes())
    }
    fn submit_event(&mut self,kind:u16,payload:&[u8])->Result<(),Status>{
        let link=self.link()?;let mut body=ostd::session::Scrub::<584>::new();let len=events_body(link,kind,payload,&mut body)?;
        let mut reply=[0u8;HEADER_LEN+8];let n=self.input(InputOp::SubmitEvents,&body[..len],&mut reply)?;let mut r=Reader::new(&reply[HEADER_LEN..n]);let _sequence=r.u64()?;r.finish()
    }
    /// Entire paste validation occurs before the first owner mutation.
    pub fn submit_paste(&mut self,text:&str)->Result<(),Status>{
        api::services::session::validate_paste(text.as_bytes())?;let link=self.link()?;
        let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(link.session)?;w.reference(link.attachment)?;w.u64(0)?;w.u32(text.len() as u32)?;w.u32(0)?;
        let mut reply=[0u8;HEADER_LEN+16];let n=self.input(InputOp::PreparePaste,&body,&mut reply)?;let paste=ObjectRefV1::from_bytes(&reply[HEADER_LEN..n])?;
        let result=(||{
            for(index,chunk)in text.as_bytes().chunks(512).enumerate(){
                let offset=index*512;let mut body=ostd::session::Scrub::<536>::new();let mut w=Writer::new(&mut body);w.reference(paste)?;w.u32(offset as u32)?;w.u32(chunk.len() as u32)?;w.bytes(chunk)?;let len=w.position();
                let mut reply=[0u8;HEADER_LEN+8];let n=self.input(InputOp::WritePaste,&body[..len],&mut reply)?;let mut r=Reader::new(&reply[HEADER_LEN..n]);if r.u32()? as usize!=offset+chunk.len()||r.u32()?!=0{return Err(Status::Malformed)}r.finish()?;
            }
            let mut reply=[0u8;HEADER_LEN+8];let n=self.input(InputOp::CommitPaste,&paste.to_bytes(),&mut reply)?;let mut r=Reader::new(&reply[HEADER_LEN..n]);let _sequence=r.u64()?;r.finish()
        })();
        if result.is_err(){let _=self.input(InputOp::AbortPaste,&paste.to_bytes(),&mut[0u8;HEADER_LEN]);}result
    }
    /// Raw bytes for a raw-mode foreground reader. Returns how many bytes were accepted.
    pub fn submit_raw(&mut self,bytes:&[u8])->Result<usize,Status>{
        if bytes.is_empty(){return Err(Status::Malformed)}let link=self.link()?;let count=bytes.len().min(512);
        let mut body=ostd::session::Scrub::<560>::new();let mut w=Writer::new(&mut body);w.reference(link.session)?;w.reference(link.attachment)?;w.u64(0)?;w.u32(count as u32)?;w.u32(0)?;w.bytes(&bytes[..count])?;let len=w.position();
        let mut reply=[0u8;HEADER_LEN+8];let n=self.input(InputOp::SubmitRaw,&body[..len],&mut reply)?;let mut r=Reader::new(&reply[HEADER_LEN..n]);let accepted=r.u32()? as usize;if r.u32()?!=0||accepted==0||accepted>count{return Err(Status::Malformed)}r.finish()?;Ok(accepted)
    }
    /// End of input for the current foreground reader. `true` when it was delivered.
    pub fn end_input(&mut self)->Result<bool,Status>{
        let link=self.link()?;let mut body=[0u8;40];let mut w=Writer::new(&mut body);w.reference(link.session)?;w.reference(link.attachment)?;w.u64(0)?;
        let mut reply=[0u8;HEADER_LEN+8];let n=self.input(InputOp::EndInput,&body,&mut reply)?;let mut r=Reader::new(&reply[HEADER_LEN..n]);let state=r.u32()?;if r.u32()?!=0||state>1{return Err(Status::Malformed)}r.finish()?;Ok(state==1)
    }
    /// New viewport. Remembered while no attachment exists (the next re-attach registers
    /// the new frontend with it); sent to the session while Attached.
    pub fn resize(&mut self,rows:u16,cols:u16)->Result<(),Status>{
        if self.closed{return Err(Status::Revoked)}api::services::session::validate_dimensions(rows,cols)?;self.rows=rows;self.cols=cols;
        let Some(link)=self.link else{return Ok(())};
        let mut body=[0u8;40];let mut w=Writer::new(&mut body);w.reference(link.session)?;w.reference(link.attachment)?;w.u16(rows)?;w.u16(cols)?;w.u32(0)?;
        let mut reply=[0u8;HEADER_LEN+8];let n=self.input(InputOp::Resize,&body,&mut reply)?;let mut r=Reader::new(&reply[HEADER_LEN..n]);let _sequence=r.u64()?;r.finish()
    }

    /// Bounded (at most [`DRAIN_MAX`] bytes), non-blocking; `0` means nothing pending.
    pub fn drain_stdout(&mut self,out:&mut[u8])->Result<usize,Status>{
        if self.closed{return Err(Status::Revoked)}if out.is_empty(){return Ok(0)}classify_drain(self.output.read(out,0),out.len())
    }
    /// Same contract as [`Self::drain_stdout`] for the diagnostic ring.
    pub fn drain_stderr(&mut self,out:&mut[u8])->Result<usize,Status>{
        if self.closed{return Err(Status::Revoked)}if out.is_empty(){return Ok(0)}classify_drain(self.diagnostic.read(out,0),out.len())
    }

    /// The shell's kernel terminal record (`QueryChild`, preparer only). `Exited` also
    /// when nothing was launched. Output written before the exit stays drainable.
    pub fn poll_child(&mut self)->Result<ChildPoll,Status>{
        if self.closed{return Err(Status::Revoked)}
        let Some(shell)=self.shell.as_mut()else{return Ok(ChildPoll::Exited)};
        if let Some((poll,_))=shell.exit{return Ok(poll)}
        let Some(child)=shell.child else{return Ok(ChildPoll::Exited)};
        let mut reply=[0u8;HEADER_LEN+24];let n=call(shell.operation.reference(),KernelOp::QueryChild,false,&child.to_bytes(),&mut reply)?;
        let observed=child_state(&reply[HEADER_LEN..n])?;if observed.0==ChildPoll::Exited{shell.exit=Some(observed);}Ok(observed.0)
    }
    /// Exit code of a shell that ended by itself; `None` while running, when cancelled or
    /// faulted, or before [`Self::poll_child`] observed the exit.
    pub fn exit_code(&self)->Option<i32>{self.shell.as_ref().and_then(|s|s.exit).and_then(|(_,code)|code)}

    /// Cancel the live shell's operation. The exit is then observed by [`Self::poll_child`].
    pub fn cancel_child(&mut self)->Result<(),Status>{
        if self.closed{return Err(Status::Revoked)}
        match self.shell.as_ref(){Some(shell) if shell.child.is_some()&&shell.exit.is_none()=>shell.operation.cancel(CANCEL_REASON),_=>Err(Status::NotFound)}
    }

    /// Detach this frontend (`ReleaseFrontend`). The shell keeps its session (Detached,
    /// `Lost(4)` to its reader); [`Self::reattach`] resumes it while this host lives.
    pub fn release(&mut self)->Result<(),Status>{
        if self.closed{return Err(Status::Revoked)}
        if self.state!=FrontendState::Attached||self.resume.is_none(){return Err(Status::WrongSession)}
        let frontend=self.frontend.ok_or(Status::Stale)?;self.input(InputOp::ReleaseFrontend,&frontend.to_bytes(),&mut[0u8;HEADER_LEN])?;
        self.frontend=None;self.link=None;self.state=FrontendState::Detached;Ok(())
    }

    /// Register a fresh frontend of `rows`×`cols` and resume the Detached session with the
    /// holder-bound `resume` capability. The old attachment stays stale; the owner reissues
    /// `resume`, read back with `QueryFrontend`.
    pub fn reattach(&mut self,rows:u16,cols:u16)->Result<(),Status>{
        if self.closed{return Err(Status::Revoked)}
        if self.state!=FrontendState::Detached{return Err(Status::WrongSession)}
        let resume=self.resume.ok_or(Status::Stale)?;api::services::session::validate_dimensions(rows,cols)?;
        let frontend=self.register(rows,cols)?;
        let mut body=[0u8;ReattachFrontendV1::LEN];ReattachFrontendV1{frontend,resume}.encode(&mut body)?;
        let mut reply=[0u8;HEADER_LEN+ReattachResultV1::LEN];
        let result=match self.input(InputOp::ReattachFrontend,&body,&mut reply){Ok(n)=>ReattachResultV1::decode(&reply[HEADER_LEN..n]),Err(status)=>Err(status)};
        let attached=match result{
            Ok(attached)=>attached,
            Err(status)=>{let _=self.input(InputOp::ReleaseFrontend,&frontend.to_bytes(),&mut[0u8;HEADER_LEN]);return Err(status)}
        };
        self.frontend=Some(frontend);self.rows=rows;self.cols=cols;self.state=FrontendState::Bound;self.resume=None;
        self.link=Some(Link{session:attached.session,attachment:attached.attachment});
        match self.poll_frontend()?{
            FrontendState::Attached if self.link==Some(Link{session:attached.session,attachment:attached.attachment})=>Ok(()),
            FrontendState::Attached=>Err(Status::WrongSession),
            _=>Err(Status::Stale),
        }
    }

    /// Idempotent: cancels a live shell (bounded reaping), releases its operation, then
    /// the holder operation (the output rings with it).
    pub fn close(&mut self)->Result<(),Status>{
        if self.closed{return Ok(())}
        let mut first:Result<(),Status>=Ok(());
        let live=self.shell.as_ref().is_some_and(|s|s.child.is_some()&&s.exit.is_none());
        if let Some(shell)=self.shell.as_ref(){let _=shell.operation.cancel(CANCEL_REASON);}
        if live{
            let mut attempt=0u32;
            first=loop{
                match reap_step(attempt,self.poll_child().map(|p|p==ChildPoll::Exited)){
                    ReapStep::Done=>break Ok(()),
                    ReapStep::Retry{sleep}=>{ostd::syscall::sys_yield();if sleep{let _=ostd::syscall::sys_set_timer(1);}}
                    ReapStep::Fail(status)=>break Err(status),
                }
                attempt+=1;
            };
        }
        let retired=self.retire_shell();
        let _=self.operation.cancel(CANCEL_REASON);
        let closed=crate::session::close_operation_bounded(self.operation.reference());
        self.closed=true;self.state=FrontendState::Closed;
        first.and(retired).and(closed)
    }
}
impl Drop for FrontendHost{fn drop(&mut self){let _=self.close();}}

#[cfg(test)]
mod tests{
    use alloc::vec::Vec;
    use super::*;

    fn r(id:u64,generation:u64)->ObjectRefV1{ObjectRefV1{id,generation}}

    fn status_bytes(state:u32,session:ObjectRefV1,attachment:ObjectRefV1,resume:ObjectRefV1)->[u8;56]{
        let mut out=[0u8;56];FrontendStatusV1{state,session,attachment,resume}.encode(&mut out).unwrap();out
    }

    #[test]
    fn query_frontend_answers_map_to_states_and_only_attached_carries_refs(){
        let z=ObjectRefV1::ZERO;
        for(state,expected)in [(0,FrontendState::Pending),(1,FrontendState::Bound),(3,FrontendState::Closed),(4,FrontendState::Lost)]{
            let decoded=FrontendStatusV1::decode(&status_bytes(state,z,z,z)).unwrap();
            assert_eq!(frontend_state(&decoded),Ok((expected,None)));
        }
        let decoded=FrontendStatusV1::decode(&status_bytes(2,r(1,2),r(3,4),r(5,6))).unwrap();
        assert_eq!(frontend_state(&decoded),Ok((FrontendState::Attached,Some((Link{session:r(1,2),attachment:r(3,4)},r(5,6))))));
        // Refs outside Attached, unknown states and a short record are refused by the decoder.
        assert_eq!(FrontendStatusV1::decode(&status_bytes(1,r(1,2),z,z)),Err(Status::Malformed));
        let mut unknown=status_bytes(0,z,z,z);unknown[0]=5;assert_eq!(FrontendStatusV1::decode(&unknown),Err(Status::Malformed));
        assert!(FrontendStatusV1::decode(&status_bytes(0,z,z,z)[..55]).is_err());
        // Attached without every capability is a protocol error, never a half link.
        for(s,a,res)in [(z,r(3,4),r(5,6)),(r(1,2),z,r(5,6)),(r(1,2),r(3,4),z)]{
            assert_eq!(frontend_state(&FrontendStatusV1{state:2,session:s,attachment:a,resume:res}),Err(Status::Malformed));
        }
    }

    #[test]
    fn input_requires_an_open_host_with_a_live_attachment(){
        let link=Link{session:r(1,1),attachment:r(2,1)};
        assert_eq!(require_link(false,Some(link)),Ok(link));
        assert_eq!(require_link(false,None),Err(Status::WouldBlock));
        assert_eq!(require_link(true,Some(link)),Err(Status::Revoked));
    }

    #[test]
    fn submitted_events_carry_the_attachment_and_a_zero_epoch(){
        let link=Link{session:r(7,1),attachment:r(9,2)};let mut out=[0u8;584];
        let n=events_body(link,2,b"ls",&mut out).unwrap();
        assert_eq!(n,32+8+8+24+2);
        let mut rd=Reader::new(&out[..n]);
        assert_eq!(rd.reference().unwrap(),r(7,1));assert_eq!(rd.reference().unwrap(),r(9,2));
        assert_eq!(rd.u64().unwrap(),0,"expected epoch is 0 for a frontend attachment");
        assert_eq!(rd.u16().unwrap(),1);assert_eq!(rd.u16().unwrap(),0);assert_eq!(rd.u32().unwrap(),26);
        assert_eq!(rd.u64().unwrap(),0);assert_eq!(rd.u64().unwrap(),0);assert_eq!(rd.u16().unwrap(),2);assert_eq!(rd.u16().unwrap(),2);assert_eq!(rd.u32().unwrap(),0);
        assert_eq!(rd.take(2).unwrap(),b"ls");rd.finish().unwrap();
        assert_eq!(events_body(link,2,&[b'x';513],&mut out),Err(Status::Malformed));
    }

    fn child_record(state:u32,reason:u32,code:i32,reserved:u32)->[u8;24]{
        let mut out=[0u8;24];let mut w=Writer::new(&mut out);w.u32(state).unwrap();w.u32(reason).unwrap();w.u32(code as u32).unwrap();w.u32(reserved).unwrap();w.u64(9).unwrap();out
    }

    #[test]
    fn child_records_distinguish_running_exit_and_cancellation(){
        assert_eq!(child_state(&child_record(1,0,0,0)),Ok((ChildPoll::Running,None)));
        assert_eq!(child_state(&child_record(2,0,0,0)),Ok((ChildPoll::Running,None)));
        assert_eq!(child_state(&child_record(3,0,7,0)),Ok((ChildPoll::Exited,Some(7))));
        assert_eq!(child_state(&child_record(3,2,0,0)),Ok((ChildPoll::Exited,None)));
        assert_eq!(child_state(&child_record(0,0,0,0)),Err(Status::Malformed));
        assert_eq!(child_state(&child_record(4,0,0,0)),Err(Status::Malformed));
        assert_eq!(child_state(&child_record(3,0,0,1)),Err(Status::Malformed));
        assert!(child_state(&child_record(3,0,0,0)[..20]).is_err());
    }

    #[test]
    fn the_shell_context_carries_exactly_the_writers_and_the_frontend(){
        let op=r(5,2);let argv=["sh"];let env=[("CELLBOX_STREAMS","merged")];
        let grants=launch_grants(r(11,1),r(12,1),r(13,1)).unwrap();
        let context=ChildContext::new(op,&argv,b"",&env,&grants).unwrap();
        assert_eq!(context.as_bytes().len(),launch_context_len(&argv,&env).unwrap());
        let read=ChildContext::from_bytes(context.as_bytes().to_vec()).unwrap();
        let mut seen:Vec<(u16,GrantSlot,u16,u64,ObjectRefV1)>=read.grants().map(|g|(g.kind,g.slot,g.rights,g.owner_service,g.object)).collect();seen.sort_by_key(|g|g.1 as u16);
        assert_eq!(seen,[
            (1,GrantSlot::Stdout,30,0,r(11,1)),(1,GrantSlot::Stderr,30,0,r(12,1)),
            (GRANT_KIND_INPUT_FRONTEND,GrantSlot::Frontend,0,api::syscall::service::INPUT as u64,r(13,1)),
        ]);
        assert_eq!(read.frontend_grant(),Some(r(13,1)));
        assert_eq!(launch_context_len(&argv,&[]).unwrap(),ChildContext::new(op,&argv,b"",&[],&grants).unwrap().as_bytes().len());
    }

    #[test]
    fn drain_is_bounded_and_empty_reads_are_zero(){
        assert_eq!(classify_drain(Ok(7),64),Ok(7));
        assert_eq!(classify_drain(Ok(DRAIN_MAX),4096),Ok(DRAIN_MAX));
        assert_eq!(classify_drain(Ok(65),64),Err(Status::Malformed));
        assert_eq!(classify_drain(Ok(DRAIN_MAX+1),4096),Err(Status::Malformed));
        assert_eq!(classify_drain(Err(Status::WouldBlock),64),Ok(0));
        assert_eq!(classify_drain(Err(Status::TimedOut),64),Ok(0));
        assert_eq!(classify_drain(Err(Status::Eof),64),Ok(0));
        assert_eq!(classify_drain(Err(Status::Revoked),64),Err(Status::Revoked));
    }

    #[test]
    fn reap_yields_then_sleeps_then_gives_up_boundedly(){
        assert_eq!(reap_step(0,Ok(true)),ReapStep::Done);
        assert_eq!(reap_step(0,Ok(false)),ReapStep::Retry{sleep:false});
        assert_eq!(reap_step(0,Err(Status::WouldBlock)),ReapStep::Retry{sleep:false});
        assert_eq!(reap_step(REAP_YIELDS-1,Err(Status::WouldBlock)),ReapStep::Retry{sleep:false});
        assert_eq!(reap_step(REAP_YIELDS,Err(Status::TimedOut)),ReapStep::Retry{sleep:true});
        assert_eq!(reap_step(REAP_ATTEMPTS-2,Ok(false)),ReapStep::Retry{sleep:true});
        assert_eq!(reap_step(REAP_ATTEMPTS-1,Ok(false)),ReapStep::Fail(Status::TimedOut));
        assert_eq!(reap_step(3,Err(Status::WrongEpoch)),ReapStep::Fail(Status::WrongEpoch));
    }
}
