use super::super::session::VfsReadOps;
use crate::ViResult;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MockReply {
    Dir(u64),
    File(u64),
    DataLen(usize, u8),
    Data(Vec<u8>),
    Ok,
    Err(u8),
}

pub(super) struct MockOps {
    replies: Vec<MockReply>,
    pub(super) calls: Vec<String>,
}

impl MockOps {
    pub(super) fn new(replies: Vec<MockReply>) -> Self {
        Self {
            replies: replies.into_iter().rev().collect(),
            calls: Vec::new(),
        }
    }

    fn next(&mut self) -> MockReply {
        self.replies.pop().expect("mock reply")
    }
}

impl VfsReadOps for MockOps {
    fn call<'a>(
        &mut self,
        req: &VfsRequest<'_>,
        resp_buf: &'a mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<VfsResponse<'a>> {
        self.calls.push(format!("{req:?}"));
        Ok(match self.next() {
            MockReply::Dir(handle) => VfsResponse::DirHandle(ViDirHandle(handle)),
            MockReply::File(handle) => VfsResponse::FileHandle(ViVfsFileHandle(handle)),
            MockReply::DataLen(len, fill) => {
                resp_buf[..len].fill(fill);
                VfsResponse::Data(&resp_buf[..len])
            }
            MockReply::Data(bytes) => {
                resp_buf[..bytes.len()].copy_from_slice(&bytes);
                VfsResponse::Data(&resp_buf[..bytes.len()])
            }
            MockReply::Ok => VfsResponse::Ok,
            MockReply::Err(code) => VfsResponse::Err(code),
        })
    }
}
