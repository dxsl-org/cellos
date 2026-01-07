//! Path buffer implementation (simplified).

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[derive(Clone, Debug, Default)]
pub struct PathBuf {
    inner: String,
}

impl PathBuf {
    pub fn new() -> Self {
        Self { inner: String::new() }
    }
    
    pub fn from(s: &str) -> Self {
        Self { inner: String::from(s) }
    }
    
    pub fn push(&mut self, path: &str) {
        if !self.inner.ends_with('/') && !path.starts_with('/') && !self.inner.is_empty() {
            self.inner.push('/');
        }
        self.inner.push_str(path);
    }
    
    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

impl fmt::Display for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}
