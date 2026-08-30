/// A fail-closed reason that a profile was not accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The profile was empty, exceeded 12,288 bytes, or held more than three certificates.
    ProfileSize,
    /// A DER tag, length, value, or required field was malformed.
    MalformedDer,
    /// Bytes remained after a DER value that must consume its input exactly.
    TrailingData,
    /// A certificate did not use X.509 version 3.
    UnsupportedCertificateVersion,
    /// A serial number was zero, negative, or non-canonical.
    InvalidSerial,
    /// The inner and outer certificate signature algorithms differed.
    AlgorithmMismatch,
    /// A signature algorithm was not ECDSA with SHA-256 and absent parameters.
    UnsupportedSignatureAlgorithm,
    /// A subject public key was not an uncompressed P-256 key.
    UnsupportedPublicKey,
    /// An ECDSA signature BIT STRING or DER signature was invalid.
    InvalidSignatureEncoding,
    /// A certificate signature did not verify under its issuer.
    SignatureVerification,
    /// The extension sequence was malformed or too large.
    MalformedExtensions,
    /// An extension OID occurred more than once.
    DuplicateExtension,
    /// An unrecognized critical extension was present.
    UnknownCriticalExtension,
    /// Basic Constraints were absent or invalid for the certificate's role.
    InvalidBasicConstraints,
    /// Key Usage was absent or invalid for the certificate's role.
    InvalidKeyUsage,
    /// Extended Key Usage was not exactly TLS client authentication.
    InvalidExtendedKeyUsage,
    /// The leaf did not contain exactly the expected DNS SAN.
    InvalidSan,
    /// Authority Key Identifier was missing or malformed.
    InvalidAuthorityKeyIdentifier,
    /// Subject Key Identifier was missing or malformed.
    InvalidSubjectKeyIdentifier,
    /// Issuer/subject names or AKI/SKI did not link exactly.
    ChainLink,
    /// A path-length constraint was exceeded.
    PathLength,
    /// DNS name constraints were malformed or rejected the leaf name.
    InvalidNameConstraints,
    /// A certificate validity timestamp was malformed or non-canonical.
    InvalidValidity,
    /// The trusted signed time was outside a certificate's validity interval.
    CertificateExpired,
    /// The NodeId extension was missing, malformed, or did not bind the leaf SPKI.
    InvalidNodeId,
    /// A profile included a duplicate certificate or the trust anchor itself.
    ForbiddenCertificate,
    /// The leaf NodeId or positive serial appears in policy denylist.
    Denied,
    /// The raw profile SHA-256 digest did not match the pending enrollment.
    ProfileDigestMismatch,
    /// Journal revision, domain identity, epoch, CSR, slot, generation, handle, or length was stale.
    StaleSnapshot,
    /// The pending leaf SPKI digest did not match the certificate.
    SpkiMismatch,
    /// A TPM public-area read failed or exceeded the fixed input bound.
    PendingPublicRead,
    /// Two consecutive reads returned different canonical TPM2B_PUBLIC bytes.
    PendingPublicRace,
    /// TPM2B_PUBLIC framing was non-canonical or empty.
    InvalidTpmPublic,
    /// The canonical TPM2B_PUBLIC digest did not match the pending snapshot.
    TpmPublicDigestMismatch,
}
