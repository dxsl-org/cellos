#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiscvSdhciProfile {
    pub base: usize,
    pub word_access_only: bool,
    pub minimum_write_spacing_us: u32,
}
