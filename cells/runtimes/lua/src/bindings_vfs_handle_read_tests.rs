use super::*;
use alloc::{collections::VecDeque, format, string::String, vec};

#[derive(Debug)]
struct MockOps {
    calls: Vec<String>,
    replies: VecDeque<TestReply>,
}

#[derive(Debug)]
enum TestReply {
    Dir(u32),
    File(u32),
    Data(Vec<u8>),
    Err(u8),
    Ok,
}

impl MockOps {
    fn new(replies: Vec<TestReply>) -> Self {
        Self {
            calls: Vec::new(),
            replies: replies.into(),
        }
    }
}

impl VfsReadOps for MockOps {
    fn call<'a>(
        &mut self,
        req: &VfsRequest<'_>,
        resp_buf: &'a mut [u8; IPC_BUF_SIZE],
    ) -> ViResult<VfsResponse<'a>> {
        self.calls.push(format!("{req:?}"));
        match self.replies.pop_front().expect("missing reply") {
            TestReply::Dir(raw) => Ok(VfsResponse::DirHandle(ViDirHandle(raw))),
            TestReply::File(raw) => Ok(VfsResponse::FileHandle(ViVfsFileHandle(raw))),
            TestReply::Data(bytes) => {
                resp_buf[..bytes.len()].copy_from_slice(&bytes);
                Ok(VfsResponse::Data(&resp_buf[..bytes.len()]))
            }
            TestReply::Err(code) => Ok(VfsResponse::Err(code)),
            TestReply::Ok => Ok(VfsResponse::Ok),
        }
    }
}

#[test]
fn reads_more_than_512_bytes() {
    let mut ops = MockOps::new(vec![
        TestReply::Dir(1),
        TestReply::File(9),
        TestReply::Data(vec![b'x'; 600]),
        TestReply::Data(vec![]),
        TestReply::Ok,
        TestReply::Ok,
    ]);
    let bytes = read_file(&mut ops, "/tmp/big.lua", 64 * 1024).expect("read");
    assert_eq!(bytes.len(), 600);
    assert!(ops.calls[2].contains("max: 4000"));
}

#[test]
fn maps_missing_file_to_io() {
    let mut ops = MockOps::new(vec![TestReply::Dir(1), TestReply::Err(1), TestReply::Ok]);
    let err = read_file(&mut ops, "/tmp/missing.lua", 64 * 1024).expect_err("missing");
    assert_eq!(err, ViError::IO);
}

#[test]
fn allows_exact_cap_then_eof() {
    let mut ops = MockOps::new(vec![
        TestReply::Dir(1),
        TestReply::File(9),
        TestReply::Data(vec![b'a'; 8]),
        TestReply::Data(vec![]),
        TestReply::Ok,
        TestReply::Ok,
    ]);
    let bytes = read_file(&mut ops, "/tmp/exact.lua", 8).expect("exact");
    assert_eq!(bytes.len(), 8);
}

#[test]
fn rejects_bytes_beyond_cap() {
    let mut ops = MockOps::new(vec![
        TestReply::Dir(1),
        TestReply::File(9),
        TestReply::Data(vec![b'a'; 8]),
        TestReply::Data(vec![b'b']),
        TestReply::Ok,
        TestReply::Ok,
    ]);
    let err = read_file(&mut ops, "/tmp/too-big.lua", 8).expect_err("oversize");
    assert_eq!(err, ViError::OutOfMemory);
}
