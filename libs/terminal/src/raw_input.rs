// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Raw-mode input for an interactive shell that is itself a session's foreground child.
//!
//! In canonical mode the input owner turns Ctrl-C into a cancellation of the committed
//! foreground child's Operation and emits no `Interrupt` event. A shell that runs as that
//! child would therefore be cancelled by every Ctrl-C. A shell that holds the raw-mode
//! lease receives Ctrl-C as the byte `0x03` on stdin instead, and decides for itself what
//! to stop. [`RawInput`] is the SDK's persistent form of that lease plus an event source
//! that gives the shell the same [`Event`] stream the canonical path produces:
//!
//! - printable input -> `Text`; Enter/Backspace/arrows/... -> `Key`;
//! - Ctrl-C (`0x03`) -> `Interrupt`;
//! - a bracketed paste (the lease also requests the bracketed-paste bit) -> `PasteBegin`,
//!   one `Text` with the whole paste, `PasteEnd`, so pasted newlines stay inert text;
//! - resize / end-of-input arrive on the control endpoint exactly as in canonical mode;
//! - stdin end of file -> `Eof`; an undecodable sequence -> `Lost`.
//!
//! No input-service change is involved: the lease is an ordinary `SetMode` the foreground
//! reader is allowed to take, and it is restored by the owner if the holder dies.
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use api::cell_session::{ObjectRefV1,Status};
use crate::codec::{Decoder,KeyAction,KeyState,KeySymbol,Modifiers};
use crate::input::{Event,EventKind};
use crate::session::TerminalSession;

/// Raw routing plus bracketed paste (`TerminalMode` bits 0 and 4).
pub const RAW_SHELL_MODE:u32=1|16;
/// Reason reported by a `Lost` event raised for input the codec cannot decode.
pub const LOST_UNDECODABLE:u32=1;

/// A held raw-mode lease and the incremental decoder for the bytes it routes to stdin.
pub struct RawInput{lease:Option<ObjectRefV1>,decoder:Decoder,pending:VecDeque<Event>,sequence:u64,epoch:u64}

impl RawInput{
    /// Take the raw-mode lease on `session` (the foreground reader's `SetMode`). The
    /// session must have no unread typed input; `WouldBlock` means retry.
    pub fn enter(session:&mut TerminalSession)->Result<Self,Status>{
        let (lease,_)=session.acquire_mode(RAW_SHELL_MODE,RAW_SHELL_MODE)?;
        Ok(Self{lease:Some(lease),decoder:Decoder::new(),pending:VecDeque::new(),sequence:0,epoch:session.epoch()})
    }
    /// Release the lease (idempotent). Buffered input is wiped.
    pub fn leave(&mut self,session:&mut TerminalSession)->Result<(),Status>{
        self.scrub();
        match self.lease.take(){Some(lease)=>session.release_mode(lease),None=>Ok(())}
    }
    fn scrub(&mut self){for event in self.pending.iter_mut(){event.scrub()}self.pending.clear();}

    /// Next input event. `deadline_ns` follows `TerminalSession::next_event`: `0` polls
    /// without blocking (`WouldBlock`), `u64::MAX` waits forever, anything else is an
    /// absolute monotonic deadline (`TimedOut`). Control events (resize, end of input) are
    /// delivered before newly read bytes.
    pub fn next_event(&mut self,session:&mut TerminalSession,deadline_ns:u64)->Result<Event,Status>{
        loop{
            if let Some(event)=self.pending.pop_front(){return Ok(event)}
            match session.next_control(0){
                Ok(event)=>return Ok(event),
                Err(Status::WouldBlock)=>{},
                Err(status)=>return Err(status),
            }
            let now=ostd::session::monotonic_ns()?;
            let mut actions:Vec<KeyAction>=Vec::new();
            let expired=self.decoder.expire(now,&mut actions);
            let mut bytes=ostd::session::Scrub::<512>::new();
            let mut lost=expired.is_err();
            match session.read_input(&mut bytes,0){
                Ok(0)=>return Err(Status::Malformed),
                Ok(count)=>{if self.decoder.feed(&bytes[..count],now,&mut actions).is_err(){lost=true}},
                Err(Status::WouldBlock)=>{},
                Err(Status::Eof)=>{
                    if self.decoder.flush(&mut actions).is_err(){lost=true}
                    self.queue(actions,lost);
                    self.push_kind(EventKind::Eof);
                    continue;
                }
                Err(status)=>return Err(status),
            }
            self.queue(actions,lost);
            if !self.pending.is_empty(){continue}
            if deadline_ns==0{return Err(Status::WouldBlock)}
            if deadline_ns!=u64::MAX&&ostd::session::monotonic_ns()?>=deadline_ns{return Err(Status::TimedOut)}
            let _=ostd::syscall::sys_set_timer(1);
        }
    }
    fn queue(&mut self,actions:Vec<KeyAction>,lost:bool){
        for action in actions{self.convert_action(action)}
        if lost{self.push_lost()}
    }
    fn convert_action(&mut self,action:KeyAction){
        let mut kinds=Vec::new();
        convert(action,&mut kinds);
        for kind in kinds{self.push_kind(kind)}
    }
    fn push_lost(&mut self){self.push_kind(EventKind::Lost(LOST_UNDECODABLE))}
    fn push_kind(&mut self,kind:EventKind){
        self.sequence=self.sequence.saturating_add(1);
        self.pending.push_back(Event{sequence:self.sequence,epoch:self.epoch,kind});
    }
}
impl Drop for RawInput{fn drop(&mut self){self.scrub()}}

/// True for Ctrl-C: the control-modified `c` with no other modifier, pressed or repeated.
pub fn is_interrupt(symbol:KeySymbol,modifiers:Modifiers,state:KeyState)->bool{
    matches!(symbol,KeySymbol::Character('c'|'C'))
        &&modifiers==Modifiers::CONTROL
        &&matches!(state,KeyState::Press|KeyState::Repeat)
}

/// One decoded action as the event kinds the canonical path would have produced.
pub fn convert(action:KeyAction,out:&mut Vec<EventKind>){
    match action{
        KeyAction::Text(text)=>out.push(EventKind::Text(text)),
        KeyAction::Paste(text)=>{
            out.push(EventKind::PasteBegin);
            out.push(EventKind::Text(text));
            out.push(EventKind::PasteEnd);
        }
        KeyAction::Key{symbol,modifiers,state}=>{
            if is_interrupt(symbol,modifiers,state){out.push(EventKind::Interrupt);return}
            // Keys without a normalized code (a bare control byte) are dropped, like the
            // canonical path drops what it cannot name.
            if let Ok(code)=symbol.code(){
                out.push(EventKind::Key{code,modifiers:modifiers.bits(),pressed:state as u8});
            }
        }
    }
}

#[cfg(test)]
mod tests{
    extern crate std;
    use super::*;
    use std::string::String;

    fn decode(bytes:&[u8])->Vec<EventKind>{
        let mut decoder=Decoder::new();
        let mut actions=Vec::new();
        decoder.feed(bytes,0,&mut actions).unwrap();
        decoder.flush(&mut actions).unwrap();
        let mut kinds=Vec::new();
        for action in actions{convert(action,&mut kinds)}
        kinds
    }

    #[test]
    fn ctrl_c_byte_is_an_interrupt_and_other_controls_are_not(){
        assert_eq!(decode(b"\x03"),[EventKind::Interrupt]);
        // Ctrl-D, Ctrl-A are ordinary control keys the shell ignores.
        let kinds=decode(b"\x04\x01");
        assert_eq!(kinds.len(),2);
        assert!(kinds.iter().all(|k|matches!(k,EventKind::Key{modifiers:2,..})));
        // Alt-C and Ctrl-Shift-C are not Ctrl-C.
        assert!(matches!(decode(b"\x1bc")[..],[EventKind::Key{modifiers:4,..}]));
    }

    #[test]
    fn typed_text_enter_backspace_and_arrows_match_the_canonical_codes(){
        assert_eq!(decode(b"ls\r"),[
            EventKind::Text(String::from("l")),
            EventKind::Text(String::from("s")),
            EventKind::Key{code:0x110001,modifiers:0,pressed:1},
        ]);
        assert_eq!(decode(b"\x7f"),[EventKind::Key{code:0x110003,modifiers:0,pressed:1}]);
        assert_eq!(decode(b"\x1b[A\x1b[B"),[
            EventKind::Key{code:0x110010,modifiers:0,pressed:1},
            EventKind::Key{code:0x110011,modifiers:0,pressed:1},
        ]);
    }

    #[test]
    fn a_bracketed_paste_is_inert_text_between_markers(){
        assert_eq!(decode(b"\x1b[200~echo a\nrm b\x1b[201~"),[
            EventKind::PasteBegin,
            EventKind::Text(String::from("echo a\nrm b")),
            EventKind::PasteEnd,
        ]);
        // A control byte inside a paste is refused with the whole paste, never executed
        // and never taken for Ctrl-C.
        let mut decoder=Decoder::new();
        let mut actions=Vec::new();
        assert!(decoder.feed(b"\x1b[200~a\x03b\x1b[201~",0,&mut actions).is_err());
        assert!(actions.is_empty());
    }

    #[test]
    fn the_lease_requests_raw_and_bracketed_paste_only(){
        assert_eq!(RAW_SHELL_MODE,0b10001);
        assert_eq!(RAW_SHELL_MODE&2,0,"echo stays with the shell's own echo");
    }
}
