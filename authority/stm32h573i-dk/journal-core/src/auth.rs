/// Produces and verifies a fixed tag for canonical full-record bytes.
pub trait RecordAuthenticator {
    /// Authenticate `message`; implementations must bind one protected device.
    fn authenticate(&self, message: &[u8]) -> [u8; 32];
}
