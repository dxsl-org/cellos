// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Incremental epoch-bound event/control framing, independent of any GUI toolkit.
use alloc::string::String;
use api::cell_session::{Reader,Status};
#[derive(Clone,PartialEq,Eq)]pub struct Event{pub sequence:u64,pub epoch:u64,pub kind:EventKind}
#[derive(Clone,PartialEq,Eq)]pub enum EventKind{Key{code:u32,modifiers:u32,pressed:u8},Text(String),PasteBegin,PasteEnd,Resize{rows:u16,cols:u16},Interrupt,Eof,Lost(u32),
    /// Owner notice to the session controller: draw the fixed first-trust prompt for this
    /// input-validated descriptor (graphic ASCII only) and stop relaying the child's output.
    TrustPrompt{host:String,port:u16,algorithm:String,fingerprint:String},
    /// Owner notice to the controller: the trust capture ended; resume relaying.
    TrustPromptEnd{accepted:bool}}
// Keystrokes and typed text can be secrets: Debug names the variant (and the
// non-secret structural fields) but NEVER the Key code/modifiers or Text bytes.
impl core::fmt::Debug for EventKind{
    fn fmt(&self,f:&mut core::fmt::Formatter<'_>)->core::fmt::Result{
        match self{
            Self::Key{..}=>f.write_str("Key(<redacted>)"),
            Self::Text(text)=>write!(f,"Text(<redacted {} bytes>)",text.len()),
            Self::PasteBegin=>f.write_str("PasteBegin"),Self::PasteEnd=>f.write_str("PasteEnd"),
            Self::Resize{rows,cols}=>f.debug_struct("Resize").field("rows",rows).field("cols",cols).finish(),
            Self::Interrupt=>f.write_str("Interrupt"),Self::Eof=>f.write_str("Eof"),
            Self::Lost(reason)=>f.debug_tuple("Lost").field(reason).finish(),
            Self::TrustPrompt{host,port,algorithm,fingerprint}=>f.debug_struct("TrustPrompt").field("host",host).field("port",port).field("algorithm",algorithm).field("fingerprint",fingerprint).finish(),
            Self::TrustPromptEnd{accepted}=>f.debug_struct("TrustPromptEnd").field("accepted",accepted).finish(),
        }
    }
}
impl core::fmt::Debug for Event{
    fn fmt(&self,f:&mut core::fmt::Formatter<'_>)->core::fmt::Result{
        f.debug_struct("Event").field("sequence",&self.sequence).field("epoch",&self.epoch).field("kind",&self.kind).finish()
    }
}
impl Event{
    pub fn decode(frame:&[u8])->Result<Self,Status>{
        if frame.len()<24{return Err(Status::Malformed)}let mut r=Reader::new(frame);let sequence=r.u64()?;let epoch=r.u64()?;let kind=r.u16()?;let len=r.u16()? as usize;if r.u32()?!=0||sequence==0||epoch==0||len>512||frame.len()!=24+len{return Err(Status::Malformed)}let payload=r.take(len)?;r.finish()?;
        api::services::session::validate_event(kind,payload)?;
        let mut r=Reader::new(payload);let kind=match kind{
            1=>{let code=r.u32()?;let modifiers=r.u32()?;let pressed=r.u8()?;if r.take(7)?.iter().any(|b|*b!=0){return Err(Status::Malformed)}EventKind::Key{code,modifiers,pressed}},
            2=>EventKind::Text(String::from(core::str::from_utf8(payload).map_err(|_|Status::UnsupportedInput)?)),
            3=>EventKind::PasteBegin,4=>EventKind::PasteEnd,5=>{let rows=r.u16()?;let cols=r.u16()?;EventKind::Resize{rows,cols}},6=>EventKind::Interrupt,7=>EventKind::Eof,8=>EventKind::Lost(r.u32()?),
            9=>{let p=api::cell_session::trust::TrustPromptV1::decode(payload)?;EventKind::TrustPrompt{host:String::from(p.host),port:p.port,algorithm:String::from(p.algorithm),fingerprint:String::from(p.fingerprint)}},
            10=>EventKind::TrustPromptEnd{accepted:api::cell_session::trust::decode_prompt_end(payload)?},
            _=>return Err(Status::UnsupportedInput),
        };Ok(Self{sequence,epoch,kind})
    }
    pub fn is_control(&self)->bool{matches!(self.kind,EventKind::Resize{..}|EventKind::Interrupt|EventKind::Eof|EventKind::Lost(_)|EventKind::TrustPrompt{..}|EventKind::TrustPromptEnd{..})}
    /// An input-owner notice for the session controller (rides the control ring, is never
    /// acknowledged by the reader and never delivered by `next_event`/`next_control`).
    pub fn is_owner_notice(&self)->bool{matches!(self.kind,EventKind::TrustPrompt{..}|EventKind::TrustPromptEnd{..})}
    /// Wipe any typed text this event carries. The SDK scrubs everything it retains; a caller
    /// that receives an event from `next_event`/`next_control` owns it and SHOULD call this
    /// once the text has been consumed (keystrokes can be secrets).
    pub fn scrub(&mut self){self.kind.scrub()}
}
impl EventKind{
    /// Zero the text of a `Text` event in place (its allocation is kept, the string becomes empty).
    pub fn scrub(&mut self){if let Self::Text(text)=self{ostd::session::zeroize_string(text)}}
}
/// Owner notices buffered per session (input queues at most eight per session).
pub const NOTICE_MAX:usize=8;
/// At most one 536-byte frame is buffered; chunk boundaries never split semantics.
pub struct ControlDecoder{bytes:[u8;536],used:usize,expected:usize}
impl Default for ControlDecoder{fn default()->Self{Self{bytes:[0;536],used:0,expected:24}}}
impl ControlDecoder{
    pub fn clear(&mut self){ostd::session::zeroize(&mut self.bytes[..self.used]);self.used=0;self.expected=24;}
    pub fn is_partial(&self)->bool{self.used!=0}
    /// Owner notices of `epoch` go to `notices` (at most [`NOTICE_MAX`]); other controls to
    /// `out`. A notice of another epoch belongs to an ended foreground and is dropped. Other
    /// controls of another epoch are `WrongEpoch` on a reader's own ring; with `drop_stale`
    /// (the controller reading its own ring while a foreground child is the reader, so
    /// nothing current can be on it but notices) they are dropped instead.
    pub fn feed(&mut self,mut bytes:&[u8],epoch:u64,out:&mut alloc::collections::VecDeque<Event>,notices:&mut alloc::collections::VecDeque<Event>,drop_stale:bool)->Result<(),Status>{
        while !bytes.is_empty(){let n=(self.expected-self.used).min(bytes.len());self.bytes[self.used..self.used+n].copy_from_slice(&bytes[..n]);self.used+=n;bytes=&bytes[n..];if self.used!=self.expected{continue}
            if self.expected==24{let len=u16::from_le_bytes(self.bytes[18..20].try_into().map_err(|_|Status::Malformed)?) as usize;if len>512{self.clear();return Err(Status::Malformed)}self.expected=24+len;if self.used!=self.expected{continue}}
            let event=match Event::decode(&self.bytes[..self.expected]){Ok(event)=>event,Err(error)=>{self.clear();return Err(error)}};self.clear();
            if !event.is_control(){return Err(Status::Malformed)}
            if event.is_owner_notice(){if event.epoch==epoch{if notices.len()>=NOTICE_MAX{return Err(Status::QuotaExceeded)}notices.try_reserve(1).map_err(|_|Status::QuotaExceeded)?;notices.push_back(event);}continue}
            if event.epoch!=epoch{if drop_stale{continue}return Err(Status::WrongEpoch)}if out.len()>=128{return Err(Status::QuotaExceeded)}insert_ordered(out,event)?;
        }Ok(())
    }
}
pub(crate)fn insert_ordered(out:&mut alloc::collections::VecDeque<Event>,event:Event)->Result<(),Status>{
    if out.iter().any(|e|e.sequence==event.sequence){return Err(Status::Malformed)}let index=out.iter().position(|e|e.sequence>event.sequence).unwrap_or(out.len());out.try_reserve(1).map_err(|_|Status::QuotaExceeded)?;out.insert(index,event);Ok(())
}
