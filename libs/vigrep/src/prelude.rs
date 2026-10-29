//! `alloc` names that `std`'s prelude would otherwise provide to the adapted
//! upstream files. Every adapted file imports this module with a glob.

pub(crate) use alloc::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
