//! Couples a DMA allocation with the device-visible IOVA returned by authorization.

pub struct AuthorizedDma<T> {
    inner: T,
    iova: u64,
}

impl<T> AuthorizedDma<T> {
    /// Authorize `inner` and retain the returned device-visible address verbatim.
    pub fn authorize<E>(inner: T, authorize: impl FnOnce(&T) -> Result<u64, E>) -> Result<Self, E> {
        let iova = authorize(&inner)?;
        Ok(Self { inner, iova })
    }

    #[inline]
    pub fn iova(&self) -> u64 {
        self.iova
    }

    #[inline]
    pub fn inner(&self) -> &T {
        &self.inner
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.inner
    }
}
