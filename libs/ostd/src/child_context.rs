// SPDX-License-Identifier: MPL-2.0
//! Canonical immutable invocation context; no shared spawn stash or secret logging.
use alloc::vec::Vec;
use api::cell_session::{KernelOp,ObjectRefV1,Reader,Writer,Status,HEADER_LEN,CONTEXT_MAX,CONTEXT_HEADER_LEN,CONTEXT_MAX_GRANTS,GRANT_LEN,GRANT_KIND_INPUT_FRONTEND};

/// Context grant slots. Kind-1 endpoint slots: Stdin/Control readers, Stdout/Stderr and
/// Completion writers. Completion carries the child's completion-record stream; it is
/// outside the stdio projection input inspects. Slots 6..=7 are reserved.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(u16)]
pub enum GrantSlot { Stdin=0, Stdout=1, Stderr=2, Control=3, Cwd=4, Completion=5, Frontend=8 }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Grant { pub kind:u16, pub rights:u16, pub slot:GrantSlot, pub owner_service:u64, pub object:ObjectRefV1 }
impl Grant {
    pub fn endpoint(slot:GrantSlot,object:ObjectRefV1,rights:u16)->Result<Self,Status> {
        let g=Self{kind:1,rights,slot,owner_service:0,object}; g.validate()?; Ok(g)
    }
    pub fn cwd(object:ObjectRefV1)->Result<Self,Status> {
        let g=Self{kind:2,rights:0,slot:GrantSlot::Cwd,owner_service:api::syscall::service::VFS as u64,object}; g.validate()?; Ok(g)
    }
    /// Kind-6 InputFrontend grant: the input-issued frontend Ref, admitted by the
    /// kernel only with the input service's bound receipt for this exact child.
    pub fn frontend(object:ObjectRefV1)->Result<Self,Status> {
        let g=Self{kind:GRANT_KIND_INPUT_FRONTEND,rights:0,slot:GrantSlot::Frontend,owner_service:api::syscall::service::INPUT as u64,object}; g.validate()?; Ok(g)
    }
    fn validate(self)->Result<(),Status> {
        if self.object.id==0 || self.object.generation==0 {return Err(Status::Malformed)}
        match self.kind {
            1=> {
                let dir=match self.slot {GrantSlot::Stdin|GrantSlot::Control=>1,GrantSlot::Stdout|GrantSlot::Stderr|GrantSlot::Completion=>2,_=>return Err(Status::Malformed)};
                if self.owner_service!=0 || self.rights & !31 !=0 || self.rights & 3 !=dir {return Err(Status::Malformed)}
            }
            2=>if self.slot!=GrantSlot::Cwd || self.rights!=0 || self.owner_service!=api::syscall::service::VFS as u64 {return Err(Status::Malformed)},
            3..=5=>return Err(Status::Unsupported),
            GRANT_KIND_INPUT_FRONTEND=>if self.slot!=GrantSlot::Frontend || self.rights!=0 || self.owner_service!=api::syscall::service::INPUT as u64 {return Err(Status::Malformed)},
            _=>return Err(Status::Malformed),
        } Ok(())
    }
    fn encode(self,out:&mut[u8])->Result<(),Status> {
        self.validate()?; let mut w=Writer::new(out); w.u16(self.kind)?;w.u16(self.rights)?;w.u16(self.slot as u16)?;w.u16(0)?;w.u64(self.owner_service)?;w.reference(self.object)?;w.u64(0)
    }
    fn decode(raw:&[u8])->Result<Self,Status> {
        let mut r=Reader::new(raw); let kind=r.u16()?;let rights=r.u16()?;
        let slot=match r.u16()? {0=>GrantSlot::Stdin,1=>GrantSlot::Stdout,2=>GrantSlot::Stderr,3=>GrantSlot::Control,4=>GrantSlot::Cwd,5=>GrantSlot::Completion,6..=7=>return Err(Status::Unsupported),8=>GrantSlot::Frontend,_=>return Err(Status::Malformed)};
        if r.u16()?!=0{return Err(Status::Malformed)} let owner_service=r.u64()?;let object=r.reference()?;if r.u64()?!=0{return Err(Status::Malformed)}r.finish()?;
        let g=Self{kind,rights,slot,owner_service,object};g.validate()?;Ok(g)
    }
}
/// Holds only validated canonical bytes. Deliberately has no Debug implementation.
pub struct ChildContext { bytes:Vec<u8> }
impl ChildContext {
    pub fn new(operation:ObjectRefV1,argv:&[&str],cwd:&[u8],environment:&[(&str,&str)],grants:&[Grant])->Result<Self,Status> {
        if operation.id==0 || operation.generation==0 || cwd.len()>4096 || cwd.contains(&0) || environment.len()>64 || grants.len()>CONTEXT_MAX_GRANTS {return Err(Status::Malformed)}
        let mut argv_len=7usize;
        for a in argv {if a.as_bytes().contains(&0){return Err(Status::Malformed)}argv_len=argv_len.checked_add(a.len()+1).ok_or(Status::Malformed)?;}
        if argv_len>512{return Err(Status::QuotaExceeded)}
        let mut env_len=0usize; let mut previous:Option<&str>=None;
        for &(key,value) in environment {
            if key.is_empty() || key.as_bytes().iter().any(|b|*b==0||*b==b'=') || value.as_bytes().contains(&0) || previous.is_some_and(|p|p>=key) || key.len()>u16::MAX as usize || value.len()>u16::MAX as usize {return Err(Status::Malformed)}
            env_len=env_len.checked_add(4+key.len()+value.len()).ok_or(Status::Malformed)?; previous=Some(key);
        }
        if env_len>4096{return Err(Status::QuotaExceeded)}
        let mut slots=0u16;for &g in grants {g.validate()?;let bit=1u16<<(g.slot as u16);if slots&bit!=0{return Err(Status::Malformed)}slots|=bit;}
        if !cwd.is_empty() && slots&(1<<4)==0{return Err(Status::Denied)}
        let len=CONTEXT_HEADER_LEN+argv_len+cwd.len()+env_len+grants.len()*GRANT_LEN;
        if len>CONTEXT_MAX{return Err(Status::QuotaExceeded)}
        let mut bytes=Vec::new();bytes.try_reserve_exact(len).map_err(|_|Status::QuotaExceeded)?;bytes.resize(len,0);
        let mut w=Writer::new(&mut bytes);w.u32(1)?;w.u32(len as u32)?;w.u32(0)?;w.u32(argv_len as u32)?;w.u16(environment.len() as u16)?;w.u16(grants.len() as u16)?;w.u32(cwd.len() as u32)?;w.u32(env_len as u32)?;w.u32(0)?;w.reference(operation)?;
        w.bytes(b"\0argv1\0")?;for a in argv {w.bytes(a.as_bytes())?;w.u8(0)?;}w.bytes(cwd)?;
        for &(key,value) in environment {w.u16(key.len() as u16)?;w.u16(value.len() as u16)?;w.bytes(key.as_bytes())?;w.bytes(value.as_bytes())?;}
        let mut offset=w.position();for &g in grants {g.encode(&mut bytes[offset..offset+GRANT_LEN])?;offset+=GRANT_LEN;}
        Ok(Self{bytes})
    }
    pub fn from_bytes(bytes:Vec<u8>)->Result<Self,Status> {validate(&bytes)?;Ok(Self{bytes})}
    pub fn as_bytes(&self)->&[u8]{&self.bytes}
    pub fn operation(&self)->ObjectRefV1 {ObjectRefV1::from_bytes(&self.bytes[32..48]).expect("validated context")}
    fn lengths(&self)->(usize,usize,usize,usize) {
        let u32at=|n|u32::from_le_bytes(self.bytes[n..n+4].try_into().expect("validated context")) as usize;
        (u32at(12),u32at(20),u32at(24),u16::from_le_bytes(self.bytes[18..20].try_into().expect("validated context")) as usize)
    }
    pub fn argv(&self)->impl Iterator<Item=&str> {let(a,_,_,_)=self.lengths();core::str::from_utf8(&self.bytes[55..48+a]).expect("validated argv").split_terminator('\0')}
    pub fn cwd(&self)->&[u8]{let(a,c,_,_)=self.lengths();&self.bytes[48+a..48+a+c]}
    pub fn environment(&self)->Environment<'_>{let(a,c,e,_)=self.lengths();Environment{bytes:&self.bytes[48+a+c..48+a+c+e]}}
    pub fn grants(&self)->impl Iterator<Item=Grant>+'_ {let(a,c,e,_)=self.lengths();self.bytes[48+a+c+e..].chunks_exact(40).map(|raw|Grant::decode(raw).expect("validated grant"))}
    pub fn cwd_grant(&self)->Option<ObjectRefV1>{self.grants().find(|g|g.slot==GrantSlot::Cwd).map(|g|g.object)}
    /// The input-issued frontend this child was launched for, if any.
    pub fn frontend_grant(&self)->Option<ObjectRefV1>{self.grants().find(|g|g.slot==GrantSlot::Frontend).map(|g|g.object)}
    /// The kind-1 completion-record writer delegated into slot 5, if any.
    pub fn completion_grant(&self)->Option<ObjectRefV1>{self.grants().find(|g|g.slot==GrantSlot::Completion&&g.kind==1).map(|g|g.object)}
    pub fn read_own()->Result<Self,Status> {
        let mut bytes=Vec::new();let mut offset=0usize;let mut total=None;
        loop {
            let mut body=[0u8;8];body[..4].copy_from_slice(&(offset as u32).to_le_bytes());body[4..].copy_from_slice(&512u32.to_le_bytes());
            let mut reply=[0u8;544];let h=crate::session::kernel_call(KernelOp::ReadOwnContext,ObjectRefV1::ZERO,false,&body,&mut reply)?;
            if h.status!=Status::Ok{return Err(h.status)}let mut r=Reader::new(&reply[HEADER_LEN..h.total_len as usize]);let n=r.u32()? as usize;let declared=r.u32()? as usize;
            if declared<48 || declared>CONTEXT_MAX || n==0 || n>512 || offset+n>declared || total.is_some_and(|t|t!=declared){return Err(Status::Malformed)}
            if total.is_none(){bytes.try_reserve_exact(declared).map_err(|_|Status::QuotaExceeded)?;total=Some(declared);}
            bytes.extend_from_slice(r.take(n)?);r.finish()?;offset+=n;if offset==declared{break}
        } Self::from_bytes(bytes)
    }
}
pub struct Environment<'a>{bytes:&'a[u8]}
impl<'a> Iterator for Environment<'a>{type Item=(&'a str,&'a str);fn next(&mut self)->Option<Self::Item>{if self.bytes.is_empty(){return None}let mut r=Reader::new(self.bytes);let k=r.u16().ok()? as usize;let v=r.u16().ok()? as usize;let key=core::str::from_utf8(r.take(k).ok()?).ok()?;let value=core::str::from_utf8(r.take(v).ok()?).ok()?;self.bytes=&self.bytes[4+k+v..];Some((key,value))}}
fn validate(bytes:&[u8])->Result<(),Status>{
    if bytes.len()<48 || bytes.len()>CONTEXT_MAX{return Err(Status::Malformed)}let mut r=Reader::new(bytes);if r.u32()?!=1{return Err(Status::UnsupportedVersion)}if r.u32()? as usize!=bytes.len()||r.u32()?!=0{return Err(Status::Malformed)}let a=r.u32()? as usize;let ec=r.u16()? as usize;let gc=r.u16()? as usize;let c=r.u32()? as usize;let e=r.u32()? as usize;if r.u32()?!=0{return Err(Status::Malformed)}let op=r.reference()?;
    if op.id==0||op.generation==0||a>512||c>4096||e>4096||ec>64||gc>CONTEXT_MAX_GRANTS||48+a+c+e+gc*40!=bytes.len(){return Err(Status::Malformed)}
    let argv=r.take(a)?;let tail=argv.strip_prefix(b"\0argv1\0").ok_or(Status::Malformed)?;if !tail.is_empty(){if tail.last()!=Some(&0){return Err(Status::Malformed)}for arg in tail[..tail.len()-1].split(|b|*b==0){core::str::from_utf8(arg).map_err(|_|Status::Malformed)?;}}
    if r.take(c)?.contains(&0){return Err(Status::Malformed)}let env=r.take(e)?;let mut er=Reader::new(env);let mut previous:Option<&str>=None;
    for _ in 0..ec {let k=er.u16()? as usize;let v=er.u16()? as usize;let key=core::str::from_utf8(er.take(k)?).map_err(|_|Status::Malformed)?;let value=core::str::from_utf8(er.take(v)?).map_err(|_|Status::Malformed)?;if key.is_empty()||key.as_bytes().iter().any(|b|*b==0||*b==b'=')||value.as_bytes().contains(&0)||previous.is_some_and(|p|p>=key){return Err(Status::Malformed)}previous=Some(key);}er.finish()?;
    let mut slots=0u16;for _ in 0..gc{let g=Grant::decode(r.take(40)?)?;let bit=1u16<<(g.slot as u16);if slots&bit!=0{return Err(Status::Malformed)}slots|=bit;}
    if c!=0&&slots&(1<<4)==0{return Err(Status::Denied)}r.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    const OPERATION:ObjectRefV1=ObjectRefV1{id:3,generation:1};
    const FRONTEND:ObjectRefV1=ObjectRefV1{id:11,generation:2};
    const ENDPOINT:ObjectRefV1=ObjectRefV1{id:12,generation:1};

    #[test]
    fn frontend_grant_round_trips_in_slot_8() {
        let grant=Grant::frontend(FRONTEND).unwrap();
        assert_eq!((grant.kind,grant.rights,grant.slot as u16,grant.owner_service),(6,0,8,api::syscall::service::INPUT as u64));
        let grants=[Grant::endpoint(GrantSlot::Stdin,ENDPOINT,1).unwrap(),Grant::endpoint(GrantSlot::Stdout,ENDPOINT,2).unwrap(),
            Grant::endpoint(GrantSlot::Stderr,ENDPOINT,2).unwrap(),Grant::endpoint(GrantSlot::Control,ENDPOINT,1).unwrap(),
            Grant::cwd(ENDPOINT).unwrap(),grant];
        let context=ChildContext::new(OPERATION,&["cellbox"],b"/",&[],&grants).unwrap();
        assert_eq!(context.frontend_grant(),Some(FRONTEND));
        let decoded=ChildContext::from_bytes(context.as_bytes().to_vec()).unwrap();
        assert_eq!(decoded.grants().collect::<Vec<_>>(),grants);
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&[]).unwrap().frontend_grant(),None);
    }

    #[test]
    fn frontend_kind_and_slot_are_exclusive() {
        let mut forged=Grant::frontend(FRONTEND).unwrap();
        forged.slot=GrantSlot::Cwd;
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&[forged]).err(),Some(Status::Malformed));
        forged=Grant::frontend(FRONTEND).unwrap();forged.rights=1;
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&[forged]).err(),Some(Status::Malformed));
        forged=Grant::frontend(FRONTEND).unwrap();forged.owner_service=api::syscall::service::VFS as u64;
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&[forged]).err(),Some(Status::Malformed));
        assert_eq!(Grant::endpoint(GrantSlot::Frontend,ENDPOINT,1).err(),Some(Status::Malformed));
        assert_eq!(Grant::frontend(ObjectRefV1::ZERO).err(),Some(Status::Malformed));
        let twice=[Grant::frontend(FRONTEND).unwrap();2];
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&twice).err(),Some(Status::Malformed));
        // Reserved slots 6..=7 stay Unsupported on decode; past 8 is Malformed; slot 5
        // decodes but admits only a kind-1 writer, so kind 6 there is Malformed.
        let mut raw=[0u8;40];raw[0..2].copy_from_slice(&6u16.to_le_bytes());raw[8..16].copy_from_slice(&(api::syscall::service::INPUT as u64).to_le_bytes());raw[16..32].copy_from_slice(&FRONTEND.to_bytes());
        for (slot,status) in [(5u16,Status::Malformed),(6,Status::Unsupported),(7,Status::Unsupported),(9,Status::Malformed)] {
            raw[4..6].copy_from_slice(&slot.to_le_bytes());assert_eq!(Grant::decode(&raw).err(),Some(status));
        }
        raw[4..6].copy_from_slice(&8u16.to_le_bytes());assert_eq!(Grant::decode(&raw),Ok(Grant::frontend(FRONTEND).unwrap()));
    }

    #[test]
    fn completion_grant_is_a_write_only_endpoint_in_slot_5() {
        let stdio=[Grant::endpoint(GrantSlot::Stdin,ENDPOINT,17).unwrap(),Grant::endpoint(GrantSlot::Stdout,ENDPOINT,30).unwrap(),
            Grant::endpoint(GrantSlot::Stderr,ENDPOINT,30).unwrap(),Grant::endpoint(GrantSlot::Control,ENDPOINT,17).unwrap()];
        for rights in [2u16,18,30] {
            let completion=Grant::endpoint(GrantSlot::Completion,FRONTEND,rights).unwrap();
            assert_eq!((completion.kind,completion.slot as u16,completion.owner_service),(1,5,0));
            let mut grants=stdio.to_vec();grants.push(completion);
            let context=ChildContext::new(OPERATION,&["ssh"],b"",&[],&grants).unwrap();
            assert_eq!(context.as_bytes().len(),48+7+4+5*40);
            let decoded=ChildContext::from_bytes(context.as_bytes().to_vec()).unwrap();
            assert_eq!(decoded.grants().collect::<Vec<_>>(),grants);
            assert_eq!(decoded.completion_grant(),Some(FRONTEND));
        }
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&stdio).unwrap().completion_grant(),None);
        // Read, bidirectional and out-of-mask rights are refused; so is a second writer.
        for rights in [0u16,1,3,17,29,34] {
            assert_eq!(Grant::endpoint(GrantSlot::Completion,FRONTEND,rights).err(),Some(Status::Malformed),"rights {rights}");
        }
        let one=Grant::endpoint(GrantSlot::Completion,FRONTEND,18).unwrap();
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&[one,one]).err(),Some(Status::Malformed));
        // A forged non-kernel owner or a non-endpoint kind in slot 5 is refused on encode and decode.
        let mut forged=one;forged.owner_service=api::syscall::service::VFS as u64;
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&[forged]).err(),Some(Status::Malformed));
        let mut raw=[0u8;40];raw[0..2].copy_from_slice(&1u16.to_le_bytes());raw[2..4].copy_from_slice(&1u16.to_le_bytes());raw[4..6].copy_from_slice(&5u16.to_le_bytes());raw[16..32].copy_from_slice(&FRONTEND.to_bytes());
        assert_eq!(Grant::decode(&raw).err(),Some(Status::Malformed));
        raw[2..4].copy_from_slice(&18u16.to_le_bytes());assert_eq!(Grant::decode(&raw),Ok(one));
    }

    #[test]
    fn ten_grants_are_refused() {
        let grants=[Grant::frontend(FRONTEND).unwrap();10];
        assert_eq!(ChildContext::new(OPERATION,&[],b"",&[],&grants).err(),Some(Status::Malformed));
    }
}
