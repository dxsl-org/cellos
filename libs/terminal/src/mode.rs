// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Scoped input-owner mode leases. Rendering helpers are not mode authority.
use api::cell_session::{ObjectRefV1,Status};
use crate::{controls::ScreenControls,session::TerminalSession};
use alloc::vec::Vec;
#[derive(Debug,Clone,Copy,PartialEq,Eq)]pub struct TerminalMode{pub raw:bool,pub echo:bool,pub alternate_screen:bool,pub cursor_visible:bool,pub bracketed_paste:bool}
impl Default for TerminalMode{fn default()->Self{Self::CANONICAL}}
impl TerminalMode{
    pub const CANONICAL:Self=Self{raw:false,echo:true,alternate_screen:false,cursor_visible:true,bracketed_paste:false};
    pub const RAW_TUI:Self=Self{raw:true,echo:false,alternate_screen:true,cursor_visible:true,bracketed_paste:true};
    pub fn bits(self)->u32{u32::from(self.raw)|u32::from(self.echo)<<1|u32::from(self.cursor_visible)<<2|u32::from(self.alternate_screen)<<3|u32::from(self.bracketed_paste)<<4}
    pub fn from_bits(bits:u32)->Result<Self,Status>{if bits&!31!=0{return Err(Status::InvalidFlags)}Ok(Self{raw:bits&1!=0,echo:bits&2!=0,cursor_visible:bits&4!=0,alternate_screen:bits&8!=0,bracketed_paste:bits&16!=0})}
    pub fn transition_to(&self,target:&Self,out:&mut Vec<u8>){if self.alternate_screen!=target.alternate_screen{if target.alternate_screen{ScreenControls::enter_alternate_screen(out)}else{ScreenControls::leave_alternate_screen(out)}}if self.cursor_visible!=target.cursor_visible{ScreenControls::set_cursor_visible(target.cursor_visible,out)}if self.bracketed_paste!=target.bracketed_paste{if target.bracketed_paste{ScreenControls::enable_bracketed_paste(out)}else{ScreenControls::disable_bracketed_paste(out)}}}
}
/// Borrowed owner lease prevents a local guard from pretending to restore global
/// state. Drop releases through input; holder death also restores at the owner.
pub struct SessionModeGuard<'a>{session:&'a mut TerminalSession,lease:Option<ObjectRefV1>,effective:u32}
impl<'a> SessionModeGuard<'a>{
    pub(crate)fn acquire(session:&'a mut TerminalSession,mask:u32,value:u32)->Result<Self,Status>{let(lease,effective)=session.acquire_mode(mask,value)?;Ok(Self{session,lease:Some(lease),effective})}
    pub fn effective_mode(&self)->Result<TerminalMode,Status>{TerminalMode::from_bits(self.effective)}
    pub fn session(&mut self)->&mut TerminalSession{self.session}
    /// Explicit release reports errors; Drop never emits input bytes or logs.
    pub fn release(mut self)->Result<(),Status>{let lease=self.lease.take().ok_or(Status::Stale)?;self.session.release_mode(lease)}
}
impl Drop for SessionModeGuard<'_>{fn drop(&mut self){if let Some(lease)=self.lease.take(){let _=self.session.release_mode(lease);}}}
/// Legacy viewport escape-state helper. This grants no input mode or session
/// authority; owner-backed applications use TerminalSession::set_mode instead.
#[derive(Debug,Clone)]
pub struct ModeGuard{baseline:TerminalMode,current:TerminalMode}
impl ModeGuard{
    pub fn new(initial:TerminalMode)->Self{Self{baseline:initial,current:initial}}
    pub fn current(&self)->&TerminalMode{&self.current}
    pub fn baseline(&self)->&TerminalMode{&self.baseline}
    pub fn set_mode(&mut self,next:TerminalMode,out:&mut Vec<u8>){self.current.transition_to(&next,out);self.current=next;}
    pub fn restore(&mut self,out:&mut Vec<u8>){self.current.transition_to(&self.baseline,out);self.current=self.baseline;}
}
