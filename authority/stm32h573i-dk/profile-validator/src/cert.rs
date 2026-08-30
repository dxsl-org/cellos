use crate::{der, der::Reader, extensions::Extensions, time, Error};

const ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const EC_PUBLIC_KEY: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const PRIME256V1: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];

#[derive(Clone, Copy)]
pub(crate) struct Certificate<'a> {
    pub full: &'a [u8],
    pub tbs: &'a [u8],
    pub serial: &'a [u8],
    pub issuer: &'a [u8],
    pub subject: &'a [u8],
    pub subject_empty: bool,
    pub not_before: i64,
    pub not_after: i64,
    pub spki: &'a [u8],
    pub public_key: &'a [u8],
    pub signature: &'a [u8],
    pub extensions: Extensions<'a>,
}

impl<'a> Certificate<'a> {
    pub fn parse(full: &'a [u8]) -> Result<Self, Error> {
        let certificate = der::one(full, 0x30)?;
        let mut outer = Reader::new(certificate.value);
        let tbs = outer.required(0x30)?;
        let outer_algorithm = outer.required(0x30)?;
        require_signature_algorithm(outer_algorithm.value)?;
        let signature_bits = outer.required(0x03)?.value;
        if signature_bits.len() < 2 || signature_bits[0] != 0 {
            return Err(Error::InvalidSignatureEncoding);
        }
        outer.finish()?;

        let mut fields = Reader::new(tbs.value);
        let version = fields.required(0xa0)?;
        let version = der::one(version.value, 0x02)?;
        if version.value != [2] {
            return Err(Error::UnsupportedCertificateVersion);
        }
        let serial_element = fields.required(0x02)?;
        let serial = der::positive_integer(serial_element.value)?;
        if serial.len() > 20 {
            return Err(Error::InvalidSerial);
        }
        let inner_algorithm = fields.required(0x30)?;
        require_signature_algorithm(inner_algorithm.value)?;
        if inner_algorithm.full != outer_algorithm.full {
            return Err(Error::AlgorithmMismatch);
        }
        let issuer_element = fields.required(0x30)?;
        validate_name(issuer_element.value, false)?;
        let issuer = issuer_element.full;
        let validity = fields.required(0x30)?;
        let (not_before, not_after) = parse_validity(validity.value)?;
        let subject_element = fields.required(0x30)?;
        validate_name(subject_element.value, true)?;
        let subject = subject_element.full;
        let subject_empty = subject_element.value.is_empty();
        let spki = fields.required(0x30)?;
        let public_key = parse_spki(spki.value)?;
        let extensions = fields.required(0xa3)?;
        fields.finish()?;
        let extensions = Extensions::parse(extensions.value)?;
        Ok(Self {
            full,
            tbs: tbs.full,
            serial,
            issuer,
            subject,
            subject_empty,
            not_before,
            not_after,
            spki: spki.full,
            public_key,
            signature: &signature_bits[1..],
            extensions,
        })
    }
}

fn require_signature_algorithm(input: &[u8]) -> Result<(), Error> {
    let mut fields = Reader::new(input);
    if fields.required(0x06)?.value != ECDSA_SHA256 {
        return Err(Error::UnsupportedSignatureAlgorithm);
    }
    fields
        .finish()
        .map_err(|_| Error::UnsupportedSignatureAlgorithm)
}

fn parse_spki(input: &[u8]) -> Result<&[u8], Error> {
    let mut fields = Reader::new(input);
    let algorithm = fields.required(0x30)?;
    let mut identifiers = Reader::new(algorithm.value);
    if identifiers.required(0x06)?.value != EC_PUBLIC_KEY
        || identifiers.required(0x06)?.value != PRIME256V1
    {
        return Err(Error::UnsupportedPublicKey);
    }
    identifiers
        .finish()
        .map_err(|_| Error::UnsupportedPublicKey)?;
    let bits = fields.required(0x03)?.value;
    fields.finish()?;
    if bits.len() != 66 || bits[0] != 0 || bits[1] != 0x04 {
        return Err(Error::UnsupportedPublicKey);
    }
    Ok(&bits[1..])
}

fn parse_validity(input: &[u8]) -> Result<(i64, i64), Error> {
    let mut fields = Reader::new(input);
    let not_before = time::parse(fields.next()?)?;
    let not_after = time::parse(fields.next()?)?;
    fields.finish()?;
    if not_before > not_after {
        return Err(Error::InvalidValidity);
    }
    Ok((not_before, not_after))
}

fn validate_name(input: &[u8], allow_empty: bool) -> Result<(), Error> {
    let mut rdns = Reader::new(input);
    if rdns.is_empty() && !allow_empty {
        return Err(Error::MalformedDer);
    }
    while !rdns.is_empty() {
        let set = rdns.required(0x31)?;
        let mut attributes = Reader::new(set.value);
        if attributes.is_empty() {
            return Err(Error::MalformedDer);
        }
        let mut previous: Option<&[u8]> = None;
        while !attributes.is_empty() {
            let attribute = attributes.required(0x30)?;
            if previous.is_some_and(|value| value >= attribute.full) {
                return Err(Error::MalformedDer);
            }
            previous = Some(attribute.full);
            let mut fields = Reader::new(attribute.value);
            let oid = fields.required(0x06)?.value;
            if !der::valid_oid(oid) {
                return Err(Error::MalformedDer);
            }
            validate_name_value(fields.next()?)?;
            fields.finish()?;
        }
    }
    Ok(())
}

fn validate_name_value(value: der::Element<'_>) -> Result<(), Error> {
    if value.value.is_empty() {
        return Err(Error::MalformedDer);
    }
    let valid = match value.tag {
        0x0c => core::str::from_utf8(value.value).is_ok(),
        0x13 => value.value.iter().all(|byte| {
            matches!(*byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b' ' | b'\'' | b'(' | b')'
            | b'+' | b',' | b'-' | b'.' | b'/' | b':' | b'=' | b'?')
        }),
        0x16 => value.value.is_ascii(),
        0x14 => true,
        0x1e => value.value.len().is_multiple_of(2),
        0x1c => value.value.len().is_multiple_of(4),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::MalformedDer)
    }
}
