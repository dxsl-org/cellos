use crate::{ViError, ViResult};
use alloc::vec::Vec;

pub(super) struct FileReadPlan<'a> {
    pub(super) parents: Vec<&'a str>,
    pub(super) file_name: &'a str,
}

impl<'a> FileReadPlan<'a> {
    pub(super) fn parse(path: &'a str) -> ViResult<Self> {
        api::dir_name::validate_dir_path(path.as_bytes()).map_err(|_| ViError::InvalidInput)?;
        let mut components = path[1..]
            .split('/')
            .map(|part| api::dir_name::validate_dir_component(part.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ViError::InvalidInput)?;
        let file_name = components.pop().ok_or(ViError::InvalidInput)?;
        Ok(Self {
            parents: components,
            file_name,
        })
    }
}
