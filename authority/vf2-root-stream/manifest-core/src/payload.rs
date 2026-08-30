use crate::cbor_read::Reader;
use crate::cbor_write::Writer;
use crate::{Component, ComponentKind, Error, Manifest, Result, COMPONENT_COUNT, LANE};

/// Encodes one RFC 8949 core-deterministic payload into `out`.
/// Returns the exact byte count; insufficient output is reported without allocation.
pub fn encode_payload(manifest: &Manifest, out: &mut [u8]) -> Result<usize> {
    basic_shape(manifest)?;
    let mut w = Writer::new(out);
    w.map(10)?;
    w.uint(1)?;
    w.uint(1)?;
    w.uint(2)?;
    w.tstr(LANE)?;
    w.uint(3)?;
    w.bstr(&manifest.device_id)?;
    w.uint(4)?;
    w.bstr(&manifest.authority_id)?;
    w.uint(5)?;
    w.uint(manifest.boot_epoch)?;
    w.uint(6)?;
    w.uint(manifest.request_id)?;
    w.uint(7)?;
    w.bstr(&manifest.approved_loader_sha256)?;
    w.uint(8)?;
    w.uint(manifest.component_region_length)?;
    w.uint(9)?;
    w.uint(manifest.entry_address)?;
    w.uint(10)?;
    w.array(COMPONENT_COUNT as u64)?;
    for component in &manifest.components {
        encode_component(&mut w, component)?;
    }
    Ok(w.len())
}

fn encode_component(w: &mut Writer<'_>, c: &Component) -> Result<()> {
    w.map(5)?;
    w.uint(1)?;
    w.uint(c.kind as u64)?;
    w.uint(2)?;
    w.uint(c.offset)?;
    w.uint(3)?;
    w.uint(c.length)?;
    w.uint(4)?;
    w.uint(c.load_address)?;
    w.uint(5)?;
    w.bstr(&c.sha256)
}

/// Strictly decodes exactly one canonical payload and rejects all trailing data.
pub fn decode_payload(input: &[u8]) -> Result<Manifest> {
    let mut r = Reader::new(input);
    if r.map()? != 10 {
        return Err(Error::WrongSchema);
    }
    key(&mut r, 1)?;
    if r.uint()? != 1 {
        return Err(Error::WrongSchema);
    }
    key(&mut r, 2)?;
    if r.tstr()? != LANE {
        return Err(Error::WrongLane);
    }
    key(&mut r, 3)?;
    let device_id = digest(r.bstr()?)?;
    key(&mut r, 4)?;
    let authority_id = digest(r.bstr()?)?;
    key(&mut r, 5)?;
    let boot_epoch = r.uint()?;
    key(&mut r, 6)?;
    let request_id = r.uint()?;
    key(&mut r, 7)?;
    let approved_loader_sha256 = digest(r.bstr()?)?;
    key(&mut r, 8)?;
    let component_region_length = r.uint()?;
    key(&mut r, 9)?;
    let entry_address = r.uint()?;
    key(&mut r, 10)?;
    if r.array()? != COMPONENT_COUNT as u64 {
        return Err(Error::WrongSchema);
    }
    let components = [
        decode_component(&mut r, 1)?,
        decode_component(&mut r, 2)?,
        decode_component(&mut r, 3)?,
        decode_component(&mut r, 4)?,
    ];
    r.done()?;
    let manifest = Manifest {
        device_id,
        authority_id,
        boot_epoch,
        request_id,
        approved_loader_sha256,
        component_region_length,
        entry_address,
        components,
    };
    basic_shape(&manifest)?;
    Ok(manifest)
}

fn decode_component(r: &mut Reader<'_>, expected: u64) -> Result<Component> {
    if r.map()? != 5 {
        return Err(Error::WrongSchema);
    }
    key(r, 1)?;
    let kind = ComponentKind::from_u64(r.uint()?)?;
    if kind as u64 != expected {
        return Err(Error::WrongComponent);
    }
    key(r, 2)?;
    let offset = r.uint()?;
    key(r, 3)?;
    let length = r.uint()?;
    key(r, 4)?;
    let load_address = r.uint()?;
    key(r, 5)?;
    let sha256 = digest(r.bstr()?)?;
    Ok(Component {
        kind,
        offset,
        length,
        load_address,
        sha256,
    })
}

fn key(r: &mut Reader<'_>, expected: u64) -> Result<()> {
    r.expect_uint(expected)
}
fn digest(value: &[u8]) -> Result<[u8; 32]> {
    if value.len() != 32 {
        return Err(Error::WrongSchema);
    }
    let mut out = [0; 32];
    out.copy_from_slice(value);
    Ok(out)
}
fn basic_shape(m: &Manifest) -> Result<()> {
    if m.boot_epoch == 0 || m.request_id == 0 {
        return Err(Error::WrongFreshness);
    }
    let mut end = 0u64;
    for (index, c) in m.components.iter().enumerate() {
        if c.kind as usize != index + 1 {
            return Err(Error::WrongComponent);
        }
        if c.length == 0 || c.offset != end {
            return Err(Error::WrongRegionLength);
        }
        end = c.offset.checked_add(c.length).ok_or(Error::Overflow)?;
    }
    if end != m.component_region_length {
        return Err(Error::WrongRegionLength);
    }
    Ok(())
}
