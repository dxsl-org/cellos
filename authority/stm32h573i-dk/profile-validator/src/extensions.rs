mod parsers;

use crate::{der, der::Reader, Error};
use parsers::*;

const BASIC: &[u8] = &[0x55, 0x1d, 0x13];
const KEY_USAGE: &[u8] = &[0x55, 0x1d, 0x0f];
const EKU: &[u8] = &[0x55, 0x1d, 0x25];
const SAN: &[u8] = &[0x55, 0x1d, 0x11];
const SKI: &[u8] = &[0x55, 0x1d, 0x0e];
const AKI: &[u8] = &[0x55, 0x1d, 0x23];
const NAME_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x1e];
const NODE_ID: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0x83, 0xb2, 0x03, 0x01, 0x01];

#[derive(Clone, Copy)]
pub(crate) struct Extensions<'a> {
    pub ca: Option<(bool, Option<u8>)>,
    pub key_usage: Option<u16>,
    pub eku_client_only: Option<bool>,
    pub san: Option<(&'a [u8], bool)>,
    pub ski: Option<&'a [u8]>,
    pub aki: Option<&'a [u8]>,
    pub node_id: Option<&'a [u8]>,
    pub permitted_dns: [Option<&'a [u8]>; 4],
    pub excluded_dns: [Option<&'a [u8]>; 4],
}

impl<'a> Extensions<'a> {
    pub fn parse(input: &'a [u8]) -> Result<Self, Error> {
        let sequence = der::one(input, 0x30)?;
        let mut reader = Reader::new(sequence.value);
        let mut out = Self::empty();
        let mut seen = [None; 16];
        let mut count = 0;
        while !reader.is_empty() {
            if count == seen.len() {
                return Err(Error::MalformedExtensions);
            }
            let extension = reader.required(0x30)?;
            let mut fields = Reader::new(extension.value);
            let oid = fields.required(0x06)?.value;
            if !der::valid_oid(oid) {
                return Err(Error::MalformedExtensions);
            }
            if seen[..count].contains(&Some(oid)) {
                return Err(Error::DuplicateExtension);
            }
            seen[count] = Some(oid);
            count += 1;
            let next = fields.next()?;
            let (critical, value) = if next.tag == 0x01 {
                if next.value != [0xff] {
                    return Err(Error::MalformedExtensions);
                }
                (true, fields.required(0x04)?.value)
            } else if next.tag == 0x04 {
                (false, next.value)
            } else {
                return Err(Error::MalformedExtensions);
            };
            fields.finish()?;
            out.accept(oid, critical, value)?;
        }
        Ok(out)
    }

    fn empty() -> Self {
        Self {
            ca: None,
            key_usage: None,
            eku_client_only: None,
            san: None,
            ski: None,
            aki: None,
            node_id: None,
            permitted_dns: [None; 4],
            excluded_dns: [None; 4],
        }
    }

    fn accept(&mut self, oid: &[u8], critical: bool, value: &'a [u8]) -> Result<(), Error> {
        if oid == BASIC {
            if !critical {
                return Err(Error::InvalidBasicConstraints);
            }
            self.ca = Some(parse_basic(value)?);
        } else if oid == KEY_USAGE {
            if !critical {
                return Err(Error::InvalidKeyUsage);
            }
            self.key_usage = Some(parse_key_usage(value)?);
        } else if oid == EKU {
            self.eku_client_only = Some(parse_eku(value)?);
        } else if oid == SAN {
            self.san = Some((parse_san(value)?, critical));
        } else if oid == SKI {
            if critical {
                return Err(Error::MalformedExtensions);
            }
            let identifier = der::one(value, 0x04)?.value;
            if identifier.is_empty() {
                return Err(Error::InvalidSubjectKeyIdentifier);
            }
            self.ski = Some(identifier);
        } else if oid == AKI {
            if critical {
                return Err(Error::MalformedExtensions);
            }
            self.aki = Some(parse_aki(value)?);
        } else if oid == NODE_ID {
            if value.len() != 32 {
                return Err(Error::InvalidNodeId);
            }
            self.node_id = Some(value);
        } else if oid == NAME_CONSTRAINTS {
            if !critical {
                return Err(Error::InvalidNameConstraints);
            }
            parse_name_constraints(value, &mut self.permitted_dns, &mut self.excluded_dns)?;
        } else if critical {
            return Err(Error::UnknownCriticalExtension);
        }
        Ok(())
    }
}
