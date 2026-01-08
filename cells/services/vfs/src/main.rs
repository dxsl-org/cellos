#![no_std]
#![no_main]

extern crate alloc;
extern crate driver_disk;

use ostd::prelude::*;
use api::fs::*;
use api::block::ViBlockDevice;
use types::*;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use driver_disk::RamDisk;
use ostd::io::println;

// Embed Binaries
static SHELL_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/shell");
static HELLO_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/hello");
static ECHO_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/echo");
static CAT_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/cat");
static LS_ELF: &[u8] = include_bytes!("../../../../kernel/src/embedded/ls");

// Simple RamFS File Representation
#[derive(Clone)]
struct RamFile {
    name: String,
    data: Vec<u8>,
    is_dir: bool,
    children: BTreeMap<String, Box<RamFile>>,
}

impl RamFile {
    fn new_file(name: &str, data: &[u8]) -> Self {
        Self {
            name: String::from(name),
            data: Vec::from(data),
            is_dir: false,
            children: BTreeMap::new(),
        }
    }

    fn new_dir(name: &str) -> Self {
        Self {
            name: String::from(name),
            data: Vec::new(),
            is_dir: true,
            children: BTreeMap::new(),
        }
    }
}

pub struct VfsManager {
    root: Box<RamFile>,
    disk: RamDisk,
}

impl VfsManager {
    pub fn new() -> Self {
        let mut root = Box::new(RamFile::new_dir("/"));

        root.children.insert(
            String::from("readme.txt"),
            Box::new(RamFile::new_file("readme.txt", b"Welcome to ViOS!\nThis is a file in RamFS.\n"))
        );

        let mut bin = Box::new(RamFile::new_dir("bin"));
        bin.children.insert(String::from("shell"), Box::new(RamFile::new_file("shell", SHELL_ELF)));
        bin.children.insert(String::from("hello"), Box::new(RamFile::new_file("hello", HELLO_ELF)));
        bin.children.insert(String::from("echo"), Box::new(RamFile::new_file("echo", ECHO_ELF)));
        bin.children.insert(String::from("cat"), Box::new(RamFile::new_file("cat", CAT_ELF)));
        bin.children.insert(String::from("ls"), Box::new(RamFile::new_file("ls", LS_ELF)));

        root.children.insert(String::from("bin"), bin);

        Self {
            root,
            disk: RamDisk::new(),
        }
    }

    fn find_node(&self, path: &str) -> Option<&RamFile> {
        if path == "/" {
            return Some(&self.root);
        }
        let mut current = &self.root;
        for component in path.split('/').filter(|c| !c.is_empty()) {
            if let Some(next) = current.children.get(component) {
                current = next;
            } else {
                return None;
            }
        }
        Some(current)
    }

    // Zero-Copy Get File Content
    pub fn get_file_content(&self, path: &str) -> Option<(usize, usize)> {
        if let Some(node) = self.find_node(path) {
            if !node.is_dir {
                return Some((node.data.as_ptr() as usize, node.data.len()));
            }
        }
        None
    }
}

#[no_mangle]
pub fn main() {
    println("VFS Service: Starting...");
    let vfs = VfsManager::new();

    let mut buf = [0u8; 256];
    loop {
        match ostd::syscall::sys_recv(0, &mut buf) {
            ostd::syscall::SyscallResult::Ok(sender) if sender > 0 => {
                // Protocol:
                // 1: Open/GetFile (Path) -> Returns Ptr/Len
                if buf[0] == 1 {
                    let path_len = buf[1] as usize;
                    if let Ok(path) = core::str::from_utf8(&buf[2..2+path_len]) {
                        if let Some((ptr, len)) = vfs.get_file_content(path) {
                            let mut resp = [0u8; 16];
                            unsafe {
                                let ptr_bytes = (ptr as u64).to_le_bytes();
                                let len_bytes = (len as u64).to_le_bytes();
                                resp[0..8].copy_from_slice(&ptr_bytes);
                                resp[8..16].copy_from_slice(&len_bytes);
                            }
                            ostd::syscall::sys_send(sender, &resp);
                        } else {
                            ostd::syscall::sys_send(sender, b"");
                        }
                    }
                }
            },
            _ => {
                ostd::task::yield_now();
            }
        }
    }
}
