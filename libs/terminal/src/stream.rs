// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Typed handles over the canonical kernel pipe owner, never a local authority table.
use api::cell_session::{ChildRefV1,KernelOp,ObjectRefV1,PrincipalV1,Reader,Writer,Status};

macro_rules! reference {($name:ident)=>{
    #[derive(Debug,Clone,Copy,PartialEq,Eq)] pub struct $name(pub(crate) ObjectRefV1);
    impl $name {pub fn raw(self)->ObjectRefV1{self.0} pub fn from_raw(raw:ObjectRefV1)->Result<Self,Status>{if raw.id==0||raw.generation==0{Err(Status::Malformed)}else{Ok(Self(raw))}}}
};}
reference!(OperationRef); reference!(EndpointRef); reference!(PreparedRef); reference!(ReceiptRef);
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct PrincipalRef(pub(crate) PrincipalV1);
impl PrincipalRef{pub fn raw(self)->PrincipalV1{self.0}}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct ChildRef(pub(crate) ChildRefV1);
impl ChildRef{pub fn raw(self)->ChildRefV1{self.0}}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub enum EndpointDirection{Read,Write}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct EndpointRights(u32);
impl EndpointRights{
    pub const READ_ONLY:Self=Self(17);pub const WRITE_ONLY:Self=Self(18);
    pub const READ_DELEGATE:Self=Self(29);pub const WRITE_DELEGATE:Self=Self(30);
    pub fn from_bits(bits:u32)->Result<Self,Status>{if bits&!31!=0 || !matches!(bits&3,1|2){Err(Status::Malformed)}else{Ok(Self(bits))}}
    pub fn bits(self)->u32{self.0}
}
#[derive(Clone,Copy,Debug)]pub struct Operation{reference:OperationRef}
impl Operation{
    pub fn create(parent:Option<OperationRef>)->Result<Self,Status>{let p=parent.map_or(ObjectRefV1::ZERO,|v|v.0);let mut reply=[0u8;40];let h=ostd::session::kernel_call(KernelOp::CreateOperation,p,true,&p.to_bytes(),&mut reply)?;ok(h.status)?;let reference=OperationRef::from_raw(ObjectRefV1::from_bytes(&reply[24..h.total_len as usize])?)?;Ok(Self{reference})}
    pub fn from_ref(reference:OperationRef)->Self{Self{reference}}
    pub fn reference(self)->OperationRef{self.reference}
    pub fn create_stream(self,capacity:u32)->Result<(Endpoint,Endpoint),Status>{let mut body=[0u8;24];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(capacity)?;w.u32(0)?;let mut reply=[0u8;56];let h=call(self.reference,KernelOp::CreateStream,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..h]);let a=Endpoint::new(EndpointRef::from_raw(r.reference()?)?,self.reference);let b=Endpoint::new(EndpointRef::from_raw(r.reference()?)?,self.reference);r.finish()?;Ok((a,b))}
    pub fn cancel(self,reason:u32)->Result<(),Status>{let mut body=[0u8;24];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(reason)?;w.u32(0)?;call(self.reference,KernelOp::CancelOperation,true,&body,&mut[0u8;24]).map(|_|())}
    pub fn close(self)->Result<(),Status>{call(self.reference,KernelOp::CloseOperation,true,&self.reference.0.to_bytes(),&mut[0u8;24]).map(|_|())}
    pub fn query(self)->Result<OperationState,Status>{let mut reply=[0u8;40];let n=call(self.reference,KernelOp::QueryOperation,false,&self.reference.0.to_bytes(),&mut reply)?;let mut r=Reader::new(&reply[24..n]);let state=r.u32()?;let holders=r.u32()?;let children=r.u32()?;if r.u32()?!=0||state>2{return Err(Status::Malformed)}r.finish()?;Ok(OperationState{state,holders,children})}
    pub fn prepare_child(self,path:&str,context_len:usize)->Result<PreparedChild,Status>{
        if path.is_empty()||path.len()>4096||path.as_bytes().contains(&0)||!(api::cell_session::CONTEXT_HEADER_LEN..=api::cell_session::CONTEXT_MAX).contains(&context_len){return Err(Status::Malformed)}
        let mut body=[0u8;4128];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(1)?;w.u32(path.len() as u32)?;w.u32(context_len as u32)?;w.u32(0)?;w.bytes(path.as_bytes())?;let n=w.position();let mut reply=[0u8;72];let n=call(self.reference,KernelOp::PrepareChild,true,&body[..n],&mut reply)?;let mut r=Reader::new(&reply[24..n]);let reference=PreparedRef::from_raw(r.reference()?)?;let child=ChildRef(r.child()?);r.finish()?;Ok(PreparedChild{reference,child,operation:self.reference,terminal:false})
    }
}
#[derive(Clone,Copy,Debug)]pub struct OperationState{pub state:u32,pub holders:u32,pub children:u32}
/// A handle's cached lineage selects correlation only. The kernel authenticates it.
#[derive(Clone,Copy,Debug)]pub struct Endpoint{reference:EndpointRef,operation:OperationRef}
impl Endpoint{
    pub fn new(reference:EndpointRef,operation:OperationRef)->Self{Self{reference,operation}}
    pub fn reference(self)->EndpointRef{self.reference}
    pub fn operation(self)->OperationRef{self.operation}
    pub fn read(self,out:&mut[u8],deadline_ns:u64)->Result<usize,Status>{
        if out.is_empty(){return Err(Status::Malformed)}let max=out.len().min(512);let mut body=[0u8;32];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(max as u32)?;w.u32(0)?;w.u64(deadline_ns)?;let mut reply=ostd::session::Scrub::<544>::new();let n=call(self.operation,KernelOp::ReadStream,true,&body,&mut reply)?;let mut r=Reader::new(&reply[24..n]);let count=r.u32()? as usize;if r.u32()?!=0||count==0||count>max{return Err(Status::Malformed)}out[..count].copy_from_slice(r.take(count)?);r.finish()?;Ok(count)
    }
    pub fn write(self,data:&[u8],deadline_ns:u64)->Result<usize,Status>{
        if data.is_empty(){return Err(Status::Malformed)}let count=data.len().min(512);let mut body=ostd::session::Scrub::<544>::new();let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(count as u32)?;w.u32(0)?;w.u64(deadline_ns)?;w.bytes(&data[..count])?;let n=w.position();let mut reply=[0u8;32];let n=call(self.operation,KernelOp::WriteStream,true,&body[..n],&mut reply)?;let mut r=Reader::new(&reply[24..n]);let actual=r.u32()? as usize;if r.u32()?!=0||actual==0||actual>count{return Err(Status::Malformed)}r.finish()?;Ok(actual)
    }
    pub fn duplicate(self,rights:EndpointRights)->Result<Self,Status>{let mut body=[0u8;24];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(rights.bits())?;w.u32(0)?;let mut reply=[0u8;40];let n=call(self.operation,KernelOp::DuplicateEndpoint,true,&body,&mut reply)?;Ok(Self::new(EndpointRef::from_raw(ObjectRefV1::from_bytes(&reply[24..n])?)?,self.operation))}
    /// Only a kernel-issued PreparedChild can be a delegation target.
    pub fn delegate(self,target:&PreparedChild,rights:EndpointRights)->Result<Self,Status>{let mut body=[0u8;56];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.child(target.child.0)?;w.u32(rights.bits())?;w.u32(0)?;let mut reply=[0u8;40];let n=call(self.operation,KernelOp::DelegateEndpoint,true,&body,&mut reply)?;Ok(Self::new(EndpointRef::from_raw(ObjectRefV1::from_bytes(&reply[24..n])?)?,target.operation))}
    pub fn close(self)->Result<(),Status>{call(self.operation,KernelOp::CloseEndpoint,true,&self.reference.0.to_bytes(),&mut[0u8;24]).map(|_|())}
    pub fn inspect(self)->Result<EndpointInfo,Status>{
        let mut reply=[0u8;72];let n=call(self.operation,KernelOp::InspectEndpoint,false,&self.reference.0.to_bytes(),&mut reply)?;
        let mut r=Reader::new(&reply[24..n]);let holder=r.principal()?;let operation=OperationRef::from_raw(r.reference()?)?;let rights=EndpointRights::from_bits(r.u32()?)?;let state=r.u32()?;let direction=match r.u32()?{0=>EndpointDirection::Read,1=>EndpointDirection::Write,_=>return Err(Status::Malformed)};
        if r.u32()?!=0||state>3{return Err(Status::Malformed)}r.finish()?;Ok(EndpointInfo{holder,operation,rights,state,direction})
    }
}
#[derive(Clone,Copy,Debug)]pub struct EndpointInfo{pub holder:PrincipalV1,pub operation:OperationRef,pub rights:EndpointRights,pub state:u32,pub direction:EndpointDirection}
/// Kernel-attested reservation, not a runnable child and not caller-constructible.
pub struct PreparedChild{reference:PreparedRef,child:ChildRef,operation:OperationRef,terminal:bool}
impl PreparedChild{
    pub fn reference(&self)->PreparedRef{self.reference}pub fn child(&self)->ChildRefV1{self.child.0}pub fn operation(&self)->OperationRef{self.operation}
    pub fn write_context(&mut self,context:&ostd::child_context::ChildContext)->Result<(),Status>{if context.operation()!=self.operation.0{return Err(Status::WrongSession)}for(offset,chunk)in context.as_bytes().chunks(512).enumerate(){let offset=offset*512;let mut body=ostd::session::Scrub::<536>::new();let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u32(offset as u32)?;w.u32(chunk.len() as u32)?;w.bytes(chunk)?;let n=w.position();let mut reply=[0u8;32];let n=call(self.operation,KernelOp::WriteChildContext,true,&body[..n],&mut reply)?;let mut r=Reader::new(&reply[24..n]);if r.u32()? as usize!=offset+chunk.len()||r.u32()?!=0{return Err(Status::Malformed)}r.finish()?;}Ok(())}
    pub fn bind_receipt(&mut self,kind:u16,slot:u16,issuer:api::service_binding::ServiceBinding,receipt:ReceiptRef)->Result<(),Status>{let mut body=[0u8;64];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.u16(kind)?;w.u16(slot)?;w.u32(0)?;w.binding(issuer)?;w.reference(receipt.0)?;call(self.operation,KernelOp::BindOwnerReceipt,true,&body,&mut[0u8;24]).map(|_|())}
    pub fn authorize_input(&mut self,issuer:api::service_binding::ServiceBinding,stdin_writer:Endpoint,control_writer:Endpoint)->Result<(),Status>{let mut body=[0u8;72];let mut w=Writer::new(&mut body);w.reference(self.reference.0)?;w.binding(issuer)?;w.reference(stdin_writer.reference.0)?;w.reference(control_writer.reference.0)?;let mut reply=[0u8;40];call(self.operation,KernelOp::AuthorizeInputBind,true,&body,&mut reply).map(|_|())}
    pub fn commit(&mut self)->Result<ChildRefV1,Status>{
        let mut reply=[0u8;56];
        // An uncertain commit may have published a real child. Re-issuing the identical
        // call makes the SDK resolve the retained outcome by query (it is never re-sent),
        // so the Drop abort below only runs for a commit proven (or left) unpublished.
        let mut attempts=0u32;
        let n=loop{match call(self.operation,KernelOp::CommitChild,true,&self.reference.0.to_bytes(),&mut reply){
            Err(Status::Indeterminate) if attempts<2=>{attempts+=1}
            other=>break other?,
        }};
        let child=ChildRefV1::from_bytes(&reply[24..n])?;if child!=self.child.0{return Err(Status::InvalidChild)}self.terminal=true;Ok(child)
    }
    pub fn abort(&mut self)->Result<(),Status>{call(self.operation,KernelOp::AbortChild,true,&self.reference.0.to_bytes(),&mut[0u8;24])?;self.terminal=true;Ok(())}
}
impl Drop for PreparedChild{fn drop(&mut self){if !self.terminal{let _=self.abort();}}}
pub(crate)fn call(operation:OperationRef,op:KernelOp,mutating:bool,body:&[u8],reply:&mut[u8])->Result<usize,Status>{let h=ostd::session::kernel_call(op,operation.0,mutating,body,reply)?;ok(h.status)?;Ok(h.total_len as usize)}
pub(crate)fn ok(status:Status)->Result<(),Status>{if status==Status::Ok{Ok(())}else{Err(status)}}
