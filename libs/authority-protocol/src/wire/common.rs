use super::payload::{Reader, Writer};
use crate::{
    AuthenticatedBinding, Operation, RequestContext, WireError, AUTHENTICATOR_LEN, DIGEST_LEN,
    ID_LEN, SIGNATURE_LEN,
};

pub(super) fn write_context(
    writer: &mut Writer<'_>,
    context: &RequestContext,
    operation: Operation,
) -> Result<(), WireError> {
    if context.operation != operation {
        return Err(WireError::UnknownOperation);
    }
    writer.put(&context.device_id)?;
    writer.put(&context.authority_id)?;
    writer.u64(context.boot_epoch)?;
    writer.u64(context.sequence)?;
    writer.put(&context.challenge)?;
    writer.u64(context.request_id)?;
    writer.u8(context.operation as u8)?;
    writer.put(&context.payload_digest)?;
    writer.put(&context.authenticator)
}

pub(super) fn read_context(
    reader: &mut Reader<'_>,
    operation: Operation,
) -> Result<RequestContext, WireError> {
    let context = RequestContext {
        device_id: reader.array::<ID_LEN>()?,
        authority_id: reader.array::<ID_LEN>()?,
        boot_epoch: reader.u64()?,
        sequence: reader.u64()?,
        challenge: reader.array::<DIGEST_LEN>()?,
        request_id: reader.u64()?,
        operation: Operation::try_from(reader.u8()?).map_err(|_| WireError::UnknownOperation)?,
        payload_digest: reader.array::<DIGEST_LEN>()?,
        authenticator: reader.array::<AUTHENTICATOR_LEN>()?,
    };
    if context.operation != operation {
        return Err(WireError::UnknownOperation);
    }
    Ok(context)
}

pub(super) fn write_binding(
    writer: &mut Writer<'_>,
    binding: &AuthenticatedBinding,
    operation: Operation,
) -> Result<(), WireError> {
    if binding.operation != operation {
        return Err(WireError::UnknownOperation);
    }
    writer.put(&binding.device_id)?;
    writer.put(&binding.authority_id)?;
    writer.u64(binding.boot_epoch)?;
    writer.u64(binding.request_id)?;
    writer.u8(binding.operation as u8)?;
    writer.put(&binding.payload_digest)?;
    writer.put(&binding.authority_signature)
}

pub(super) fn read_binding(
    reader: &mut Reader<'_>,
    operation: Operation,
) -> Result<AuthenticatedBinding, WireError> {
    let binding = AuthenticatedBinding {
        device_id: reader.array::<ID_LEN>()?,
        authority_id: reader.array::<ID_LEN>()?,
        boot_epoch: reader.u64()?,
        request_id: reader.u64()?,
        operation: Operation::try_from(reader.u8()?).map_err(|_| WireError::UnknownOperation)?,
        payload_digest: reader.array::<DIGEST_LEN>()?,
        authority_signature: reader.array::<SIGNATURE_LEN>()?,
    };
    if binding.operation != operation {
        return Err(WireError::UnknownOperation);
    }
    Ok(binding)
}
