#![allow(unsafe_code)]

use core::future::Future;
use core::task::{Context, Poll};
use core::pin::Pin;
use crate::syscall::{sys_try_recv, SyscallResult};

/// Future that waits for a message to arrive.
/// Returns the Sender ID.
pub struct AsyncRecv<'a> {
    pub mask: usize,
    pub buf: &'a mut [u8],
}

impl<'a> Future for AsyncRecv<'a> {
    type Output = SyscallResult;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
         match sys_try_recv(self.mask, self.buf) {
             SyscallResult::Ok(0) => {
                 // No message yet. Return Pending.
                 // The naive executor will yield and poll again.
                 Poll::Pending
             },
             SyscallResult::Ok(id) => Poll::Ready(SyscallResult::Ok(id)),
             err => Poll::Ready(err),
         }
    }
}

pub fn recv_async<'a>(mask: usize, buf: &'a mut [u8]) -> AsyncRecv<'a> {
    AsyncRecv { mask, buf }
}
