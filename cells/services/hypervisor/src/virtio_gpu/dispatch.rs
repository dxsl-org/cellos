use crate::virtqueue::DescBuf;

use super::{
    command,
    resource::{ResourceError, ResourceTable},
    scanout::ScanoutBridge,
};

pub fn handle_control(
    resources: &mut ResourceTable,
    scanout: &mut ScanoutBridge,
    bufs: &[DescBuf],
    vm_id: usize,
    width: u32,
    height: u32,
) -> u32 {
    let Some(header) = command::read_header(bufs, vm_id) else {
        return 0;
    };
    match header.command {
        command::GET_DISPLAY_INFO => {
            command::write_display_info(bufs, vm_id, &command::display_info(header, width, height))
        }
        command::RESOURCE_CREATE_2D => {
            let Some(cmd) = command::parse_create_2d(bufs, vm_id) else {
                return command::write_error(bufs, vm_id, header, command::ERR_UNSPEC);
            };
            respond(bufs, vm_id, header, resources.create_2d(cmd))
        }
        command::RESOURCE_UNREF => {
            let Some(cmd) = command::parse_resource_id(bufs, vm_id) else {
                return command::write_error(bufs, vm_id, header, command::ERR_UNSPEC);
            };
            respond(bufs, vm_id, header, resources.unref(cmd.resource_id))
        }
        command::RESOURCE_ATTACH_BACKING => {
            let (_, resource_id, entries) = match command::parse_attach_backing(bufs, vm_id) {
                Some(cmd) => cmd,
                None => return command::write_error(bufs, vm_id, header, command::ERR_UNSPEC),
            };
            respond(
                bufs,
                vm_id,
                header,
                resources.attach_backing(resource_id, entries),
            )
        }
        command::SET_SCANOUT => {
            let Some(cmd) = command::parse_set_scanout(bufs, vm_id) else {
                return command::write_error(bufs, vm_id, header, command::ERR_UNSPEC);
            };
            respond(bufs, vm_id, header, resources.set_scanout(vm_id, cmd))
        }
        command::TRANSFER_TO_HOST_2D => {
            let Some(cmd) = command::parse_transfer_to_host_2d(bufs, vm_id) else {
                return command::write_error(bufs, vm_id, header, command::ERR_UNSPEC);
            };
            respond(bufs, vm_id, header, resources.transfer_to_host(vm_id, cmd))
        }
        command::RESOURCE_FLUSH => {
            let Some(cmd) = command::parse_flush(bufs, vm_id) else {
                return command::write_error(bufs, vm_id, header, command::ERR_UNSPEC);
            };
            respond(
                bufs,
                vm_id,
                header,
                resources
                    .flush_resource(cmd.resource_id, cmd.rect)
                    .map(|_| scanout.notify_damage(cmd.rect)),
            )
        }
        _ => command::write_error(bufs, vm_id, header, command::ERR_UNSPEC),
    }
}

pub fn handle_cursor(resources: &mut ResourceTable, bufs: &[DescBuf], vm_id: usize) -> u32 {
    let Some(header) = command::read_header(bufs, vm_id) else {
        return 0;
    };
    match header.command {
        command::UPDATE_CURSOR => command::parse_cursor(bufs, vm_id, true)
            .ok_or(ResourceError::InvalidParameter)
            .and_then(|cmd| resources.update_cursor(vm_id, cmd)),
        command::MOVE_CURSOR => command::parse_cursor(bufs, vm_id, false)
            .ok_or(ResourceError::InvalidParameter)
            .and_then(|cmd| resources.move_cursor(vm_id, cmd)),
        _ => Err(ResourceError::InvalidParameter),
    }
    .map_or(0, |_| 0)
}

fn respond(
    bufs: &[DescBuf],
    vm_id: usize,
    header: command::CtrlHeader,
    result: Result<(), ResourceError>,
) -> u32 {
    match result {
        Ok(()) => command::write_ok(bufs, vm_id, header),
        Err(ResourceError::InvalidResourceId) => {
            command::write_error(bufs, vm_id, header, command::ERR_INVALID_RESOURCE_ID)
        }
        Err(ResourceError::InvalidParameter) => {
            command::write_error(bufs, vm_id, header, command::ERR_INVALID_PARAMETER)
        }
        Err(ResourceError::OutOfMemory) => {
            command::write_error(bufs, vm_id, header, command::ERR_OUT_OF_MEMORY)
        }
    }
}
