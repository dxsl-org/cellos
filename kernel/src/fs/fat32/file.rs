use crate::{
    fs::{Inode, InodeId, attr::FileAttr, Result},
};
use alloc::boxed::Box;
use alloc::sync::Arc;
use async_trait::async_trait;

use super::{Cluster, Fat32Operations, reader::Fat32Reader};

pub struct Fat32FileNode<T: Fat32Operations> {
    reader: Fat32Reader<T>,
    attr: FileAttr,
    id: InodeId,
}

impl<T: Fat32Operations> Fat32FileNode<T> {
    pub fn new(fs: Arc<T>, root: Cluster, attr: FileAttr) -> Result<Self> {
        let id = InodeId::from_fsid_and_inodeid(fs.id() as _, root.value() as _);

        Ok(Self {
            reader: Fat32Reader::new(fs, root, attr.size),
            attr,
            id,
        })
    }
}

#[async_trait]
impl<T: Fat32Operations> Inode for Fat32FileNode<T> {
    fn id(&self) -> InodeId {
        self.id
    }

    async fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.reader.read_at(offset, buf).await
    }

    async fn getattr(&self) -> Result<FileAttr> {
        Ok(self.attr.clone())
    }
}
