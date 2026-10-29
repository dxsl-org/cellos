// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Owner-backed headless terminal sessions. Cached refs select owner RPCs only.
use alloc::{collections::VecDeque,vec::Vec};
use api::cell_session::{ChildRefV1,InputOp,KernelOp,ObjectRefV1,PrincipalV1,Reader,Writer,Status,VfsOp};
use api::cell_session::trust::BeginTrustConfirmationV1;
use api::cell_session::frontend::{CreateForFrontendResultV1,CreateForFrontendV1};
use api::service_binding::ServiceBinding;
use ostd::child_context::{ChildContext,Grant,GrantSlot};
use crate::{input::{ControlDecoder,Event,insert_ordered},pipe_child::CwdLease,stream::{Endpoint,EndpointRef,EndpointRights,Operation,OperationRef,ReceiptRef,call,ok}};

#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct SessionRef(ObjectRefV1);
impl SessionRef{pub fn raw(self)->ObjectRefV1{self.0}}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct ForegroundRef(ObjectRefV1);
impl ForegroundRef{pub fn raw(self)->ObjectRefV1{self.0}}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct AttachmentRef(ObjectRefV1);
impl AttachmentRef{pub fn raw(self)->ObjectRefV1{self.0}}
/// The frontend holder's stdout/stderr writer ends a frontend-launched shell relays its
/// session output to (see [`TerminalSession::create_for_frontend`]).
#[derive(Debug,Clone,Copy)]pub struct FrontendOutput{pub stdout:Endpoint,pub stderr:Endpoint}
/// No public mutable authority fields; every transition is authenticated by input.
pub struct TerminalSession{
    reference:SessionRef,owner:ServiceBinding,operation:OperationRef,controller:PrincipalV1,
    foreground:ForegroundRef,epoch:u64,mode:u32,attachment:Option<AttachmentRef>,
    stdin:Endpoint,stdout:Endpoint,stderr:Endpoint,control:Endpoint,
    output:Option<Endpoint>,diagnostic:Option<Endpoint>,
    cursor:u64,delivered:u64,events:VecDeque<Event>,decoder:ControlDecoder,closed:bool,
    /// Input-owner notices for the controller (`TrustPrompt`/`TrustPromptEnd`), current epoch only.
    notices:VecDeque<Event>,
}
impl TerminalSession{
    pub fn create_serial(rows:u16,cols:u16)->Result<Self,Status>{
        if rows==0||rows>256||cols==0||cols>512{return Err(Status::Malformed)}
        Self::bootstrap(|session,operation,bootstrap,reply|{
            let mut body=[0u8;40];let mut w=Writer::new(&mut body);w.reference(operation.raw())?;w.reference(bootstrap)?;w.u8(0)?;w.u8(0)?;w.u16(rows)?;w.u16(cols)?;w.u16(0)?;
            let n=input(session.owner,operation,InputOp::Create,true,&body,reply)?;let mut r=Reader::new(&reply[24..n]);
            session.reference=SessionRef(r.reference()?);session.controller=r.principal()?;session.foreground=ForegroundRef(r.reference()?);session.epoch=r.u64()?;session.mode=r.u32()?;if r.u32()?!=0{return Err(Status::Malformed)}
            for expected in [session.stdin,session.stdout,session.stderr,session.control]{if r.reference()?!=expected.reference().raw(){return Err(Status::Malformed)}}r.finish()?;
            let mut body=[0u8;80];let mut w=Writer::new(&mut body);w.reference(session.reference.0)?;w.principal(session.controller)?;w.reference(operation.raw())?;w.reference(session.stdin.reference().raw())?;w.reference(session.output.ok_or(Status::Malformed)?.reference().raw())?;
            let n=session.request(InputOp::AttachFrontend,true,&body,reply)?;session.attachment=Some(AttachmentRef(ObjectRefV1::from_bytes(&reply[24..n])?));Ok(())
        })
    }
    /// A shell launched by a frontend holder (`FrontendHost`): its own launch context
    /// carries the frontend (kind 6, slot 8) and the holder's stdout/stderr writer ends
    /// (kind 1, slots 1/2). The shell bootstraps and controls its own session exactly like
    /// [`Self::create_serial`], but creates it for that frontend (`CreateForFrontend`, the
    /// frontend's registered size). The attachment belongs to the holder, so the session
    /// has none; its output/diagnostic readers stay with the shell, which relays them to
    /// the returned [`FrontendOutput`].
    pub fn create_for_frontend(context:&ChildContext)->Result<(Self,FrontendOutput),Status>{
        let frontend=context.frontend_grant().ok_or(Status::Denied)?;
        let context_operation=OperationRef::from_raw(context.operation())?;let mut writers=[None;2];
        for grant in context.grants(){if grant.kind==1{match grant.slot{GrantSlot::Stdout=>writers[0]=Some(endpoint(grant.object,context_operation)?),GrantSlot::Stderr=>writers[1]=Some(endpoint(grant.object,context_operation)?),_=>{}}}}
        let output=FrontendOutput{stdout:writers[0].ok_or(Status::Denied)?,stderr:writers[1].ok_or(Status::Denied)?};
        let session=Self::bootstrap(|session,operation,bootstrap,reply|{
            let mut body=[0u8;CreateForFrontendV1::LEN];CreateForFrontendV1{operation:operation.raw(),bootstrap,frontend,rows:0,cols:0}.encode(&mut body)?;
            let n=input(session.owner,operation,InputOp::CreateForFrontend,true,&body,reply)?;let created=CreateForFrontendResultV1::decode(&reply[24..n])?;
            if created.endpoints!=[session.stdin,session.stdout,session.stderr,session.control].map(|e|e.reference().raw())||created.attachment.is_zero(){return Err(Status::Malformed)}
            session.reference=SessionRef(created.session);session.controller=created.controller;session.foreground=ForegroundRef(created.lease);session.epoch=created.epoch;session.mode=created.mode;Ok(())
        })?;
        Ok((session,output))
    }
    /// Kernel bootstrap (session endpoints) under a fresh operation, then the owner's
    /// `create` step. Any failure resolves the bootstrap and cancels the operation.
    fn bootstrap(create:impl FnOnce(&mut Self,OperationRef,ObjectRefV1,&mut[u8])->Result<(),Status>)->Result<Self,Status>{
        let operation=Operation::create(None)?.reference();let owner=ostd::session::service_binding(api::syscall::service::INPUT)?;
        let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(operation.raw())?;w.binding(owner)?;w.u32(4096)?;w.u32(0)?;let mut reply=[0u8;136];let n=call(operation,KernelOp::BootstrapSession,true,&body,&mut reply)?;
        let mut r=Reader::new(&reply[24..n]);let bootstrap=r.reference()?;let stdin=endpoint(r.reference()?,operation)?;let stdout=endpoint(r.reference()?,operation)?;let stderr=endpoint(r.reference()?,operation)?;let control=endpoint(r.reference()?,operation)?;let output=endpoint(r.reference()?,operation)?;let diagnostic=endpoint(r.reference()?,operation)?;r.finish()?;
        let mut session=Self{reference:SessionRef(ObjectRefV1::ZERO),owner,operation,controller:PrincipalV1::default(),foreground:ForegroundRef(ObjectRefV1::ZERO),epoch:0,mode:0,attachment:None,stdin,stdout,stderr,control,output:Some(output),diagnostic:Some(diagnostic),cursor:0,delivered:0,events:VecDeque::new(),decoder:ControlDecoder::default(),closed:false,notices:VecDeque::new()};
        let mut reply=[0u8;4000];
        match create(&mut session,operation,bootstrap,&mut reply){
            Ok(())=>Ok(session),
            Err(status)=>{
                // Never a half-created session: the bootstrap is resolved and the operation cancelled.
                session.closed=true;let mut body=[0u8;40];let mut w=Writer::new(&mut body);w.reference(bootstrap)?;w.u32(2)?;w.u32(0)?;w.reference(ObjectRefV1::ZERO)?;let _=call(operation,KernelOp::ResolveBootstrap,true,&body,&mut[0u8;256]);let _=Operation::from_ref(operation).cancel(5);
                Err(status)
            }
        }
    }
    /// Read kernel-committed identity at the child's first instruction, not a
    /// parent's mutable stash or a caller-invented session principal.
    pub fn from_own_context(context:&ChildContext)->Result<Self,Status>{
        let operation=OperationRef::from_raw(context.operation())?;let mut reply=[0u8;104];let n=call(operation,KernelOp::ReadOwnSessionBinding,false,&[],&mut reply)?;let mut r=Reader::new(&reply[24..n]);let reference=SessionRef(r.reference()?);let foreground=ForegroundRef(r.reference()?);let epoch=r.u64()?;let control_ref=r.reference()?;let owner=r.binding()?;r.finish()?;
        if ostd::session::service_binding(api::syscall::service::INPUT)?!=owner{return Err(Status::Stale)}
        let mut stdio=[None;4];for grant in context.grants(){if grant.kind==1&&(grant.slot as usize)<4{stdio[grant.slot as usize]=Some(endpoint(grant.object,operation)?);}}
        let stdin=stdio[0].ok_or(Status::Denied)?;let stdout=stdio[1].ok_or(Status::Denied)?;let stderr=stdio[2].ok_or(Status::Denied)?;let control=stdio[3].ok_or(Status::Denied)?;if control.reference().raw()!=control_ref{return Err(Status::WrongSession)}
        let s=Self{reference,owner,operation,controller:stdin.inspect()?.holder,foreground,epoch,mode:2,attachment:None,stdin,stdout,stderr,control,output:None,diagnostic:None,cursor:0,delivered:0,events:VecDeque::new(),decoder:ControlDecoder::default(),closed:false,notices:VecDeque::new()};let _=s.capabilities()?;Ok(s)
    }
    pub fn reference(&self)->SessionRef{self.reference}pub fn operation(&self)->OperationRef{self.operation}pub fn epoch(&self)->u64{self.epoch}pub fn foreground(&self)->ForegroundRef{self.foreground}
    pub fn stdin(&self)->Endpoint{self.stdin}pub fn stdout(&self)->Endpoint{self.stdout}pub fn stderr(&self)->Endpoint{self.stderr}pub fn control(&self)->Endpoint{self.control}
    fn request(&self,op:InputOp,mutating:bool,body:&[u8],reply:&mut[u8])->Result<usize,Status>{if self.closed{return Err(Status::Revoked)}input(self.owner,self.operation,op,mutating,body,reply)}
    pub fn write_stdout(&mut self,data:&[u8],deadline_ns:u64)->Result<usize,Status>{self.stdout.write(data,deadline_ns)}
    pub fn write_stderr(&mut self,data:&[u8],deadline_ns:u64)->Result<usize,Status>{self.stderr.write(data,deadline_ns)}
    pub fn drain_stdout(&mut self,out:&mut[u8])->Result<usize,Status>{self.output.ok_or(Status::WrongOwner)?.read(out,0)}
    pub fn drain_stderr(&mut self,out:&mut[u8])->Result<usize,Status>{self.diagnostic.ok_or(Status::WrongOwner)?.read(out,0)}
    pub fn read_input(&mut self,out:&mut[u8],deadline_ns:u64)->Result<usize,Status>{
        if self.mode&1==0{return Err(Status::WrongDirection)}
        loop{self.drain_control()?;match self.stdin.read(out,0){Err(Status::WouldBlock)=>{},result=>return result}if deadline_ns==0{return Err(Status::WouldBlock)}if deadline_ns!=u64::MAX&&ostd::session::monotonic_ns()?>=deadline_ns{return Err(Status::TimedOut)}let _=ostd::syscall::sys_set_timer(1);}
    }
    fn drain_control(&mut self)->Result<(),Status>{self.read_control(false)}
    fn read_control(&mut self,drop_stale:bool)->Result<(),Status>{let mut bytes=[0u8;512];loop{match self.control.read(&mut bytes,0){Ok(n)=>self.decoder.feed(&bytes[..n],self.epoch,&mut self.events,&mut self.notices,drop_stale)?,Err(Status::WouldBlock)=>return Ok(()),Err(Status::Eof)=>return if self.decoder.is_partial(){Err(Status::Malformed)}else{Ok(())},Err(e)=>return Err(e)}}}
    /// The next input-owner notice for this controller, nonblocking. Poll it while a
    /// foreground child holds the session: input then writes only notices of the current
    /// epoch to the controller's own control ring, so earlier-epoch controls still unread
    /// there are stale and dropped. A `TrustPrompt` means: stop relaying the child's output
    /// and draw the fixed prompt from its fields; `TrustPromptEnd` means: end it and resume.
    /// Foreground restoration also ends a prompt (the kernel flushes this ring then).
    pub fn next_owner_notice(&mut self)->Result<Option<Event>,Status>{
        if self.notices.is_empty(){self.read_control(true)?;}
        Ok(self.notices.pop_front())
    }
    /// Tell input that this controller has drawn (`TrustPrompt`) or ended (`TrustPromptEnd`)
    /// the prompt of `notice`. Input arms its capture only after the prompt is acknowledged
    /// and keeps the session's input from the child until the end is acknowledged.
    pub fn acknowledge_owner_notice(&mut self,notice:&Event)->Result<(),Status>{
        if !notice.is_owner_notice()||notice.epoch!=self.epoch{return Err(Status::Malformed)}
        let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.foreground.0)?;w.u64(self.epoch)?;w.u64(notice.sequence)?;
        self.request(InputOp::AcknowledgeControl,true,&body,&mut[0u8;24]).map(|_|())
    }
    /// Ask input to run the first-trust confirmation of `confirmation` (a VFS-registered
    /// challenge naming this Cell) on this session, bound to this reader's exact lease and
    /// epoch. Input then captures the terminal's input itself; the decision goes only to the
    /// registering VFS, never to this caller.
    pub fn begin_trust_confirmation(&mut self,confirmation:ObjectRefV1)->Result<(),Status>{
        let mut body=[0u8;BeginTrustConfirmationV1::LEN];BeginTrustConfirmationV1{session:self.reference.0,lease:self.foreground.0,epoch:self.epoch,confirmation}.encode(&mut body)?;
        self.request(InputOp::BeginTrustConfirmation,true,&body,&mut[0u8;24]).map(|_|())
    }
    pub fn next_event(&mut self,deadline_ns:u64)->Result<Event,Status>{
        if self.mode&1!=0{return Err(Status::WrongDirection)}
        loop{
            self.drain_control()?;
            // Partial preceding controls cannot be overtaken by typed events.
            if self.decoder.is_partial(){
                if deadline_ns==0{return Err(Status::WouldBlock)}
                if deadline_ns!=u64::MAX&&ostd::session::monotonic_ns()?>=deadline_ns{return Err(Status::TimedOut)}
                let _=ostd::syscall::sys_set_timer(1);continue;
            }
            if self.events.front().is_some_and(|event|!event.is_control()){return self.deliver_event()}
            // Control may follow typed records still in the owner log. Fetch
            // a nonblocking batch before choosing the next owner sequence.
            let now=ostd::session::monotonic_ns()?;
            if self.events.is_empty()&&deadline_ns!=0&&deadline_ns!=u64::MAX&&now>=deadline_ns{return Err(Status::TimedOut)}
            let bounded=if deadline_ns==0||!self.events.is_empty(){0}else{now.checked_add(100_000_000).ok_or(Status::Malformed)?.min(deadline_ns)};
            let mut body=[0u8;56];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.foreground.0)?;w.u64(self.cursor)?;w.u16(64)?;w.u16(0)?;w.u32(0)?;w.u64(bounded)?;
            let mut reply=ostd::session::Scrub::<4000>::new();
            match self.request(InputOp::PollEvents,true,&body,&mut reply){
                Ok(n)=>{
                    let mut r=Reader::new(&reply[24..n]);let cursor=r.u64()?;let count=r.u16()? as usize;
                    if r.u16()?!=0||count>64{return Err(Status::Malformed)}
                    let len=r.u32()? as usize;let events=r.take(len)?;r.finish()?;let mut offset=0usize;
                    for _ in 0..count{
                        let header=events.get(offset..offset+24).ok_or(Status::Malformed)?;
                        let len=u16::from_le_bytes(header[18..20].try_into().map_err(|_|Status::Malformed)?) as usize;
                        let end=offset.checked_add(24+len).ok_or(Status::Malformed)?;
                        let event=Event::decode(events.get(offset..end).ok_or(Status::Malformed)?)?;
                        if event.is_control()||event.epoch!=self.epoch{return Err(Status::WrongEpoch)}
                        if self.events.len()>=128{return Err(Status::QuotaExceeded)}
                        insert_ordered(&mut self.events,event)?;offset=end;
                    }
                    if offset!=events.len()||cursor<self.cursor{return Err(Status::Malformed)}self.cursor=cursor;
                }
                Err(Status::WouldBlock|Status::TimedOut)=>{},
                Err(e)=>return Err(e),
            }
            self.drain_control()?;
            if !self.decoder.is_partial()&&!self.events.is_empty(){return self.deliver_event()}
            if deadline_ns==0{return Err(Status::WouldBlock)}
        }
    }
    fn deliver_event(&mut self)->Result<Event,Status>{
        let event=self.events.front().ok_or(Status::WouldBlock)?;
        if event.sequence<=self.delivered||event.epoch!=self.epoch{return Err(Status::WrongEpoch)}
        if event.is_control(){
            let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.foreground.0)?;w.u64(self.epoch)?;w.u64(event.sequence)?;
            self.request(InputOp::AcknowledgeControl,true,&body,&mut[0u8;24])?;
        }
        let event=self.events.pop_front().ok_or(Status::WouldBlock)?;self.delivered=event.sequence;Ok(event)
    }
    pub fn capabilities(&self)->Result<Capabilities,Status>{let mut reply=[0u8;48];let n=self.request(InputOp::QueryCapabilities,false,&self.reference.0.to_bytes(),&mut reply)?;let mut r=Reader::new(&reply[24..n]);let frontend=r.u8()?;if r.u8()?!=0{return Err(Status::Malformed)}let rows=r.u16()?;let cols=r.u16()?;if r.u16()?!=0{return Err(Status::Malformed)}let mode_mask=r.u32()?;let event_mask=r.u32()?;let features=r.u32()?;if r.u32()?!=0{return Err(Status::Malformed)}r.finish()?;Ok(Capabilities{frontend,rows,cols,mode_mask,event_mask,features})}
    pub fn set_mode(&mut self,mask:u32,value:u32)->Result<crate::mode::SessionModeGuard<'_>,Status>{crate::mode::SessionModeGuard::acquire(self,mask,value)}
    pub(crate)fn acquire_mode(&mut self,mask:u32,value:u32)->Result<(ObjectRefV1,u32),Status>{if mask&!31!=0||value&!mask!=0{return Err(Status::InvalidFlags)}let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.foreground.0)?;w.u64(self.epoch)?;w.u32(mask)?;w.u32(value)?;let mut reply=[0u8;48];let n=self.request(InputOp::SetMode,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);let lease=r.reference()?;let mode=r.u32()?;if r.u32()?!=0{return Err(Status::Malformed)}r.finish()?;self.mode=mode;Ok((lease,mode))}
    pub(crate)fn release_mode(&mut self,lease:ObjectRefV1)->Result<(),Status>{let mut body=[0u8;32];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(lease)?;let mut reply=[0u8;32];let n=self.request(InputOp::ReleaseMode,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);self.mode=r.u32()?;if r.u32()?!=0{return Err(Status::Malformed)}r.finish()}
    pub fn resize(&mut self,rows:u16,cols:u16)->Result<u64,Status>{if rows==0||rows>256||cols==0||cols>512{return Err(Status::Malformed)}let mut body=[0u8;40];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.attachment.ok_or(Status::WrongOwner)?.0)?;w.u16(rows)?;w.u16(cols)?;w.u32(0)?;let mut reply=[0u8;32];let n=self.request(InputOp::Resize,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);let seq=r.u64()?;r.finish()?;Ok(seq)}
    /// Submit normalized input without caller-selected sequence/epoch fields.
    pub fn submit_key(&mut self,code:u32,modifiers:u32,pressed:u8)->Result<u64,Status>{
        let mut payload=ostd::session::Scrub::<16>::new();payload[..4].copy_from_slice(&code.to_le_bytes());payload[4..8].copy_from_slice(&modifiers.to_le_bytes());payload[8]=pressed;
        api::services::session::validate_event(1,&payload)?;self.submit_action(1,&payload)
    }
    pub fn submit_text(&mut self,text:&str)->Result<u64,Status>{
        api::services::session::validate_text(text.as_bytes())?;if text.is_empty()||text.len()>512{return Err(Status::Malformed)}self.submit_action(2,text.as_bytes())
    }
    fn submit_action(&mut self,kind:u16,payload:&[u8])->Result<u64,Status>{
        let mut body=ostd::session::Scrub::<584>::new();let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.attachment.ok_or(Status::WrongOwner)?.0)?;w.u64(self.epoch)?;w.u16(1)?;w.u16(0)?;w.u32((24+payload.len()) as u32)?;
        w.u64(0)?;w.u64(0)?;w.u16(kind)?;w.u16(payload.len() as u16)?;w.u32(0)?;w.bytes(payload)?;let len=w.position();
        let mut reply=[0u8;32];let n=self.request(InputOp::SubmitEvents,true,&body[..len],&mut reply)?;let mut r=Reader::new(&reply[24..n]);let seq=r.u64()?;r.finish()?;Ok(seq)
    }
    /// Entire paste validation occurs before the first owner mutation.
    pub fn submit_paste(&mut self,text:&str)->Result<u64,Status>{
        api::services::session::validate_paste(text.as_bytes())?;
        let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.attachment.ok_or(Status::WrongOwner)?.0)?;w.u64(self.epoch)?;w.u32(text.len() as u32)?;w.u32(0)?;
        let mut reply=[0u8;40];let n=self.request(InputOp::PreparePaste,true,&body,&mut reply)?;let paste=ObjectRefV1::from_bytes(&reply[24..n])?;
        let result=(||{for(offset,chunk)in text.as_bytes().chunks(512).enumerate(){let offset=offset*512;let mut body=ostd::session::Scrub::<536>::new();let mut w=Writer::new(&mut body);w.reference(paste)?;w.u32(offset as u32)?;w.u32(chunk.len() as u32)?;w.bytes(chunk)?;let len=w.position();let n=self.request(InputOp::WritePaste,true,&body[..len],&mut reply)?;let mut r=Reader::new(&reply[24..n]);if r.u32()? as usize!=offset+chunk.len()||r.u32()?!=0{return Err(Status::Malformed)}r.finish()?;}
            let n=self.request(InputOp::CommitPaste,true,&paste.to_bytes(),&mut reply)?;let mut r=Reader::new(&reply[24..n]);let seq=r.u64()?;r.finish()?;Ok(seq)})();
        if result.is_err(){let _=self.request(InputOp::AbortPaste,true,&paste.to_bytes(),&mut[0u8;24]);}result
    }
    /// Raw applications consume control separately from unchanged byte stdin.
    pub fn next_control(&mut self,deadline_ns:u64)->Result<Event,Status>{
        loop{self.drain_control()?;if let Some(event)=self.events.front(){if !event.is_control(){return Err(Status::WrongDirection)}}
            if let Some(event)=self.events.pop_front(){let mut body=[0u8;48];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.foreground.0)?;w.u64(self.epoch)?;w.u64(event.sequence)?;self.request(InputOp::AcknowledgeControl,true,&body,&mut[0u8;24])?;self.delivered=event.sequence;return Ok(event)}
            if deadline_ns==0{return Err(Status::WouldBlock)}if deadline_ns!=u64::MAX&&ostd::session::monotonic_ns()?>=deadline_ns{return Err(Status::TimedOut)}let _=ostd::syscall::sys_set_timer(1);
        }
    }
    pub fn submit_raw(&mut self,bytes:&[u8])->Result<usize,Status>{if bytes.is_empty(){return Err(Status::Malformed)}let count=bytes.len().min(512);let mut body=ostd::session::Scrub::<560>::new();let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.attachment.ok_or(Status::WrongOwner)?.0)?;w.u64(self.epoch)?;w.u32(count as u32)?;w.u32(0)?;w.bytes(&bytes[..count])?;let len=w.position();let mut reply=[0u8;32];let n=self.request(InputOp::SubmitRaw,true,&body[..len],&mut reply)?;let mut r=Reader::new(&reply[24..n]);let n=r.u32()? as usize;if r.u32()?!=0||n==0||n>count{return Err(Status::Malformed)}r.finish()?;Ok(n)}
    pub fn end_input(&mut self)->Result<bool,Status>{let mut body=[0u8;40];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(self.attachment.ok_or(Status::WrongOwner)?.0)?;w.u64(self.epoch)?;let mut reply=[0u8;32];let n=self.request(InputOp::EndInput,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);let state=r.u32()?;if r.u32()?!=0||state>1{return Err(Status::Malformed)}r.finish()?;Ok(state==1)}
    pub fn close(&mut self)->Result<(),Status>{if self.closed{return Ok(())}self.request(InputOp::CloseSession,true,&self.reference.0.to_bytes(),&mut[0u8;24])?;self.closed=true;self.scrub_events();self.decoder.clear();ostd::session::retire_operation(self.operation.raw());Ok(())}
    /// Wipe and drop every buffered event: typed text can be a secret.
    fn scrub_events(&mut self){for event in self.events.iter_mut(){event.scrub()}self.events.clear();self.notices.clear();}
}
impl Drop for TerminalSession{fn drop(&mut self){self.scrub_events();self.decoder.clear();}}
#[derive(Debug,Clone,Copy)]pub struct Capabilities{pub frontend:u8,pub rows:u16,pub cols:u16,pub mode_mask:u32,pub event_mask:u32,pub features:u32}
fn endpoint(raw:ObjectRefV1,operation:OperationRef)->Result<Endpoint,Status>{Ok(Endpoint::new(EndpointRef::from_raw(raw)?,operation))}
fn input(owner:ServiceBinding,operation:OperationRef,op:InputOp,mutating:bool,body:&[u8],reply:&mut[u8])->Result<usize,Status>{let h=ostd::session::service_call(owner,api::syscall::service::INPUT,operation.raw(),op as u16,mutating,body,reply)?;ok(h.status)?;Ok(h.total_len as usize)}
fn vfs(owner:ServiceBinding,operation:OperationRef,op:VfsOp,body:&[u8],reply:&mut[u8])->Result<usize,Status>{let h=ostd::session::service_call(owner,api::syscall::service::VFS,operation.raw(),op as u16,true,body,reply)?;ok(h.status)?;Ok(h.total_len as usize)}
/// CloseOperation retried while WouldBlock (receipts are acknowledged
/// asynchronously by input/VFS). Bounded: first yields, then one-tick sleeps.
pub(crate) fn close_operation_bounded(operation:OperationRef)->Result<(),Status>{
    let mut last=Status::WouldBlock;
    for attempt in 0..64u32{
        match Operation::from_ref(operation).close(){
            Ok(())=>return Ok(()),
            Err(Status::WouldBlock)=>{ostd::syscall::sys_yield();if attempt>=8{let _=ostd::syscall::sys_set_timer(1);}}
            Err(e)=>{last=e;break}
        }
    }
    // Gave up (or definite refusal): never let correlation lanes accumulate.
    ostd::session::retire_operation(operation.raw());
    Err(last)
}

pub struct ForegroundChild{child:ChildRefV1,operation:OperationRef,lease:ObjectRefV1,receipt:ReceiptRef,epoch:u64,baseline_mode:u32,restored:bool,cwd:Option<CwdLease>,activation_error:Option<Status>,completion:Option<Endpoint>}
impl ForegroundChild{pub fn child(&self)->ChildRefV1{self.child}pub fn operation(&self)->OperationRef{self.operation}pub fn receipt(&self)->ReceiptRef{self.receipt}pub fn activation_error(&self)->Option<Status>{self.activation_error}
    /// Nonblocking drain of the child's completion stream (see
    /// [`TerminalSession::spawn_foreground_with_completion`]): copies everything currently
    /// buffered, up to `out.len()`, and returns the byte count. Nothing buffered, writer
    /// end-of-stream, and a child spawned without a completion stream all read as `Ok(0)`;
    /// only real stream errors fail. Read before [`TerminalSession::close_foreground`],
    /// which releases the stream with the child operation.
    pub fn read_completion(&self,out:&mut[u8])->Result<usize,Status>{
        let Some(reader)=self.completion else{return Ok(0)};
        let mut total=0;
        while total<out.len(){match reader.read(&mut out[total..],0){Ok(n)=>total+=n,Err(Status::WouldBlock|Status::Eof)=>break,Err(e)=>return Err(e)}}
        Ok(total)
    }
    /// The kernel's terminal record of this child (bounded `QueryChild` poll, preparer
    /// only, readable until [`TerminalSession::close_foreground`]). Unlike a TID wait it
    /// cannot miss a child that was already reaped.
    pub fn wait_status(&self)->Result<crate::pipe_child::ChildStatus,Status>{crate::pipe_child::wait_child_status(self.operation,self.child)}
}
impl TerminalSession{
    /// Launch `path` as the session's foreground child with the stdio grants (slots
    /// 0..=3) and, for a non-empty `cwd`, a VFS cwd grant (slot 4).
    pub fn spawn_foreground(&mut self,path:&str,argv:&[&str],cwd:&[u8],environment:&[(&str,&str)],cwd_source:Option<ObjectRefV1>)->Result<ForegroundChild,Status>{
        self.launch_foreground(path,argv,cwd,environment,cwd_source,false)
    }
    /// [`Self::spawn_foreground`] plus a 4096-byte kernel completion stream created under
    /// the child operation: its writer end is delegated write-only into context slot 5
    /// (the child takes it with [`completion_writer`]) and its reader end stays in the
    /// returned child ([`ForegroundChild::read_completion`]). The stdio grants input
    /// inspects are unchanged.
    pub fn spawn_foreground_with_completion(&mut self,path:&str,argv:&[&str],cwd:&[u8],environment:&[(&str,&str)],cwd_source:Option<ObjectRefV1>)->Result<ForegroundChild,Status>{
        self.launch_foreground(path,argv,cwd,environment,cwd_source,true)
    }
    fn launch_foreground(&mut self,path:&str,argv:&[&str],cwd:&[u8],environment:&[(&str,&str)],cwd_source:Option<ObjectRefV1>,with_completion:bool)->Result<ForegroundChild,Status>{
        // The completion stream lives under the session's own operation, not the child's:
        // cancelling the child (Ctrl-C) cancels its operation and every endpoint in it,
        // and the record written before that must stay readable.
        let completion=if with_completion{Some(Operation::from_ref(self.operation).create_stream(4096)?)}else{None};
        let release=|completion:Option<(Endpoint,Endpoint)>|if let Some((reader,writer))=completion{let _=reader.close();let _=writer.close();};
        let operation=match Operation::create(Some(self.operation)){Ok(operation)=>operation,Err(status)=>{release(completion);return Err(status)}};
        let result=self.prepare_foreground(operation,path,argv,cwd,environment,cwd_source,completion);
        if result.is_err(){
            // Every failure path (including a late one after publication) cancels,
            // then releases the child operation so the session's child quota and the
            // SDK correlation lanes cannot leak; the completion stream goes with it.
            let _=operation.cancel(5);
            let _=close_operation_bounded(operation.reference());
            release(completion);
        }
        result
    }
    /// Release a reaped, restored foreground's child operation. Input acknowledges its
    /// receipt asynchronously and VFS settles its cwd receipt when [`Self::finish_foreground`]
    /// drops the cwd lease, so CloseOperation answers WouldBlock until then; retry for a
    /// bounded number of yields. Correlation lanes for the operation are dropped on
    /// success and on giving up. The completion reader (an endpoint of the session's
    /// operation) is closed first: read the record before this.
    pub fn close_foreground(&mut self,child:&ForegroundChild)->Result<(),Status>{
        // A repeated close finds the reader already gone; the operation close decides.
        if let Some(reader)=child.completion{let _=reader.close();}
        close_operation_bounded(child.operation)
    }
    fn prepare_foreground(&mut self,operation:Operation,path:&str,argv:&[&str],cwd:&[u8],environment:&[(&str,&str)],cwd_source:Option<ObjectRefV1>,completion:Option<(Endpoint,Endpoint)>)->Result<ForegroundChild,Status>{
        let (len,grant_count)=foreground_context_len(argv,cwd,environment,completion.is_some())?;
        let mut prepared=operation.prepare_child(path,len)?;
        let (stdin_source,stdin_writer)=operation.create_stream(4096)?;let (control_source,control_writer)=operation.create_stream(4096)?;
        let stdin=stdin_source.delegate(&prepared,EndpointRights::READ_ONLY)?;let control=control_source.delegate(&prepared,EndpointRights::READ_ONLY)?;
        let stdout=self.stdout.delegate(&prepared,EndpointRights::WRITE_DELEGATE)?;let stderr=self.stderr.delegate(&prepared,EndpointRights::WRITE_DELEGATE)?;
        // Completion stream: reader kept by this preparer, writer delegated write-only.
        let completion=match completion{Some((reader,writer))=>Some((reader,writer,writer.delegate(&prepared,EndpointRights::WRITE_ONLY)?)),None=>None};
        let vfs_owner=if !cwd.is_empty(){Some(ostd::session::service_binding(api::syscall::service::VFS)?)}else{None};
        let mut cwd_lease=None;let mut reservation=None;let mut child_cwd=None;
        if let Some(owner)=vfs_owner{
            let lease=cwd_lease.insert(CwdLease::new(owner,operation.reference()));
            let source=match cwd_source{Some(s)=>s,None=>{let path=core::str::from_utf8(cwd).map_err(|_|Status::UnsupportedInput)?;let mut send=[0u8;4096];let mut recv=[0u8;4096];let req=api::ipc::VfsRequest::OpenRootDir{path};let response=ostd::ipc::service_call_typed_bounded::<_,api::ipc::VfsResponse<'_>>(owner.tid as usize,&req,&mut send,&mut recv,100).map_err(|_|Status::Indeterminate)?;let id=match response{api::ipc::VfsResponse::DirHandle(handle)=>handle.0,_=>return Err(Status::Denied)};lease.dir=Some(api::dir_handles::ViDirHandle(id));if ostd::session::service_binding(api::syscall::service::VFS)?!=owner{return Err(Status::Stale)}ObjectRefV1{id,generation:self.controller.generation}}};
            let mut body=[0u8;32];let mut w=Writer::new(&mut body);w.reference(prepared.reference().raw())?;w.reference(source)?;let mut reply=[0u8;56];let n=vfs(owner,operation.reference(),VfsOp::ReserveCwd,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);reservation=Some(r.reference()?);child_cwd=Some(r.reference()?);r.finish()?;
        }
        let mut grants=Vec::new();grants.try_reserve_exact(grant_count).map_err(|_|Status::QuotaExceeded)?;
        for(slot,ep,rights)in [(GrantSlot::Stdin,stdin,17),(GrantSlot::Stdout,stdout,30),(GrantSlot::Stderr,stderr,30),(GrantSlot::Control,control,17)]{grants.push(Grant::endpoint(slot,ep.reference().raw(),rights)?);}if let Some(grant)=child_cwd{grants.push(Grant::cwd(grant)?);}
        if let Some((_,_,delegated))=completion{grants.push(Grant::endpoint(GrantSlot::Completion,delegated.reference().raw(),EndpointRights::WRITE_ONLY.bits() as u16)?);}
        let context=ChildContext::new(operation.reference().raw(),argv,cwd,environment,&grants)?;prepared.write_context(&context)?;
        if let(Some(owner),Some(reservation))=(vfs_owner,reservation){let mut body=[0u8;32];let mut w=Writer::new(&mut body);w.reference(reservation)?;w.reference(prepared.reference().raw())?;let mut reply=[0u8;40];let n=vfs(owner,operation.reference(),VfsOp::FinalizeCwd,&body,&mut reply)?;let receipt=ReceiptRef::from_raw(ObjectRefV1::from_bytes(&reply[24..n])?)?;if let Some(lease)=cwd_lease.as_mut(){lease.receipt=Some(receipt);}prepared.bind_receipt(2,4,owner,receipt)?;}
        prepared.authorize_input(self.owner,stdin_writer,control_writer)?;
        let mut body=[0u8;152];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u64(self.epoch)?;w.child(prepared.child())?;for ep in [stdin,stdout,stderr,control,stdin_writer,control_writer]{w.reference(ep.reference().raw())?;}
        let mut reply=[0u8;64];let n=self.request(InputOp::PrepareForeground,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);let lease=r.reference()?;let receipt=ReceiptRef::from_raw(r.reference()?)?;let epoch=r.u64()?;r.finish()?;
        prepared.bind_receipt(1,0,self.owner,receipt)?;let child=prepared.commit()?;
        // The committed child holds its own writer count; dropping the preparer's writer
        // makes the child the sole completion writer (a close failure leaves it to the
        // operation's release).
        if let Some((_,writer,_))=completion{let _=writer.close();}
        let mut body=[0u8;80];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(lease)?;w.child(child)?;w.reference(receipt.raw())?;
        // CommitChild has already published a real child. Even a failed owner
        // reconciliation must return its handle so the caller can cancel/reap.
        let activation_error=self.request(InputOp::CommitForeground,true,&body,&mut[0u8;48]).err();
        self.epoch=epoch;self.scrub_events();self.decoder.clear();Ok(ForegroundChild{child,operation:operation.reference(),lease,receipt,epoch,baseline_mode:self.mode,restored:false,cwd:cwd_lease,activation_error,completion:completion.map(|(reader,_,_)|reader)})
    }
    /// Returns WouldBlock while the canonical child is live. No caller completion
    /// boolean/TID establishes restoration authority; input queries its receipt.
    /// Restored, the child's cwd lease is dropped so VFS settles its cwd receipt
    /// before [`Self::close_foreground`].
    pub fn finish_foreground(&mut self,child:&mut ForegroundChild)->Result<(),Status>{if child.restored{return Ok(())}let mut body=[0u8;72];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.reference(child.lease)?;w.u64(child.epoch)?;w.child(child.child)?;let mut reply=[0u8;48];let n=self.request(InputOp::RestoreForeground,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);let epoch=r.u64()?;let reader=r.principal()?;r.finish()?;if reader!=self.controller||epoch<=child.epoch{return Err(Status::WrongEpoch)}self.epoch=epoch;self.mode=child.baseline_mode;self.cursor=0;self.delivered=0;self.scrub_events();self.decoder.clear();child.restored=true;child.cwd.take();Ok(())}
    pub fn cancel_foreground(&mut self,child:&ForegroundChild,reason:u32)->Result<(),Status>{Operation::from_ref(child.operation).cancel(reason)}
}
/// Exact immutable-context length and grant count of a foreground launch (stdio, optional
/// cwd, optional completion writer), computed before any authority reference exists.
pub(crate) fn foreground_context_len(argv:&[&str],cwd:&[u8],environment:&[(&str,&str)],with_completion:bool)->Result<(usize,usize),Status>{
    let argv_len=argv.iter().try_fold(7usize,|n,a|n.checked_add(a.len()+1)).ok_or(Status::Malformed)?;
    let env_len=environment.iter().try_fold(0usize,|n,(k,v)|n.checked_add(4+k.len()+v.len())).ok_or(Status::Malformed)?;
    if argv_len>512||cwd.len()>4096||env_len>4096||environment.len()>64{return Err(Status::QuotaExceeded)}
    let grant_count=4+usize::from(!cwd.is_empty())+usize::from(with_completion);
    Ok((48+argv_len+cwd.len()+env_len+grant_count*40,grant_count))
}
/// Child side of [`TerminalSession::spawn_foreground_with_completion`]: the write-only
/// completion-record stream delegated into context slot 5, as an endpoint on the
/// context operation. `None` when the parent granted no completion stream.
pub fn completion_writer(context:&ChildContext)->Option<Endpoint>{
    endpoint(context.completion_grant()?,OperationRef::from_raw(context.operation()).ok()?).ok()
}
