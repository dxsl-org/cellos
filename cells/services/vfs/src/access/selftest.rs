use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use types::CellId;

use crate::caller::Caller;
use crate::manager::VfsManager;

pub fn run() {
    guest_disk_security_boundary_is_fail_closed();
}

fn guest_disk_security_boundary_is_fail_closed() {
    let mut vfs = VfsManager::new();
    let hv_caller = Caller {
        cell: CellId(60),
        generation: 1,
        sender_tid: 50,
        flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE,
    };
    vfs.access = crate::access::AccessTable::with_service_lookup(|id| {
        (id == api::hypervisor::HYPERVISOR_SERVICE_ID).then_some(50)
    });
    let _ = vfs.dirs.on_contact(hv_caller);
    vfs.dirs.mark_attested(hv_caller);

    assert!(vfs.access.can_write(hv_caller, "/mnt/sd/guest_disk.img"));
    let dead_caller = hv_caller;
    let respawned_caller = Caller {
        cell: CellId(60),
        generation: 2,
        sender_tid: 52,
        flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE,
    };
    vfs.access = crate::access::AccessTable::with_service_lookup(|_| None);
    assert!(!vfs.access.can_write(dead_caller, "/mnt/sd/guest_disk.img"));
    assert!(!vfs
        .access
        .can_write(respawned_caller, "/mnt/sd/guest_disk.img"));
    vfs.access = crate::access::AccessTable::with_service_lookup(|id| {
        (id == api::hypervisor::HYPERVISOR_SERVICE_ID).then_some(52)
    });
    assert!(!vfs.access.can_write(dead_caller, "/mnt/sd/guest_disk.img"));
    assert!(vfs
        .access
        .can_write(respawned_caller, "/mnt/sd/guest_disk.img"));
    vfs.access = crate::access::AccessTable::with_service_lookup(|id| {
        (id == api::hypervisor::HYPERVISOR_SERVICE_ID).then_some(50)
    });

    for protected in [
        "/",
        "/mnt",
        "/mnt/",
        "/mnt/sd",
        "/mnt/sd/",
        "/mnt/sd/guest_disk.img",
    ] {
        assert!(!vfs.access.can_remove_tree(hv_caller, protected));
        assert!(!vfs.access.can_remove_dir(hv_caller, protected));
    }

    assert!(matches!(
        crate::paths::write_file(&mut vfs, hv_caller, "/mnt/sd/guest_disk.img", b"payload"),
        VfsResponse::Err(crate::paths::ERR_DENIED)
    ));
    assert!(matches!(
        crate::paths::unlink_file(&mut vfs, hv_caller, "/mnt/sd/guest_disk.img"),
        VfsResponse::Err(crate::paths::ERR_DENIED)
    ));

    let req = VfsRequest::Append {
        path: "/mnt/sd/guest_disk.img",
        content: b"extra",
    };
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(&req, &mut req_buf).expect("encode");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let resp = crate::dispatch::handle_request(&mut vfs, &req_buf, Some(hv_caller), &mut resp_buf);
    assert!(matches!(resp, VfsResponse::Err(crate::paths::ERR_DENIED)));

    let root = vfs.dirs.open_root(hv_caller, "/mnt/sd").expect("root");
    let file = vfs
        .files
        .insert(hv_caller, "/mnt/sd/guest_disk.img", root.0)
        .expect("insert file");
    assert!(matches!(
        crate::grant_write::write(&mut vfs, hv_caller, file.0, 0, 1, 512),
        VfsResponse::Err(crate::paths::ERR_DENIED)
    ));
    use super::stub::GuestDiskStub;
    use alloc::boxed::Box;
    let stub_idx = vfs.mounts.add_backend(Box::new(GuestDiskStub(1024)));
    vfs.mounts.mount("/mnt/sd/guest_disk.img", stub_idx);
    assert_eq!(vfs.stat("/mnt/sd/guest_disk.img"), Some((1024, false)));

    let over_req = VfsRequest::WriteHandleGrant {
        file,
        offset: 512,
        grant: 0,
        bytes: 1024,
    };
    let mut req_buf = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(&over_req, &mut req_buf).expect("encode");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let resp = crate::dispatch::handle_request(&mut vfs, &req_buf, Some(hv_caller), &mut resp_buf);
    assert!(matches!(resp, VfsResponse::Err(crate::paths::ERR_DENIED)));

    let over_boundary_req = VfsRequest::WriteHandleGrant {
        file,
        offset: 1024,
        grant: 0,
        bytes: 1,
    };
    api::ipc::encode(&over_boundary_req, &mut req_buf).expect("encode");
    let resp = crate::dispatch::handle_request(&mut vfs, &req_buf, Some(hv_caller), &mut resp_buf);
    assert!(matches!(resp, VfsResponse::Err(crate::paths::ERR_DENIED)));

    let within_req = VfsRequest::WriteHandleGrant {
        file,
        offset: 0,
        grant: 0,
        bytes: 512,
    };
    api::ipc::encode(&within_req, &mut req_buf).expect("encode");
    let resp = crate::dispatch::handle_request(&mut vfs, &req_buf, Some(hv_caller), &mut resp_buf);
    assert!(matches!(resp, VfsResponse::Err(crate::paths::ERR_IO)));

    for alias in ["/mnt/sd/GUEST_~1.IMG", "/mnt/sd/GUESTD~1.IMG"] {
        assert!(matches!(
            crate::paths::write_file(&mut vfs, hv_caller, alias, b"payload"),
            VfsResponse::Err(crate::paths::ERR_DENIED)
        ));
        assert!(matches!(
            crate::paths::unlink_file(&mut vfs, hv_caller, alias),
            VfsResponse::Err(crate::paths::ERR_DENIED)
        ));
        let append_alias = VfsRequest::Append {
            path: alias,
            content: b"extra",
        };
        let mut req_b = [0u8; IPC_BUF_SIZE];
        api::ipc::encode(&append_alias, &mut req_b).expect("encode");
        let mut resp_b = [0u8; IPC_BUF_SIZE];
        let resp = crate::dispatch::handle_request(&mut vfs, &req_b, Some(hv_caller), &mut resp_b);
        assert!(matches!(resp, VfsResponse::Err(crate::paths::ERR_DENIED)));
    }

    let other_caller = Caller {
        cell: CellId(77),
        generation: 1,
        sender_tid: 66,
        flags: 0,
    };
    let _ = vfs.dirs.on_contact(other_caller);
    vfs.dirs.mark_attested(other_caller);
    let other_root = vfs
        .dirs
        .open_root(other_caller, "/mnt/sd")
        .expect("other root");

    for alias in [
        "/mnt/sd/guest_disk.img",
        "/mnt/sd/GUEST_~1.IMG",
        "/mnt/sd/GUESTD~1.IMG",
        "/mnt/sd/Guest_Disk.img",
    ] {
        assert!(!vfs.access.can_write(other_caller, alias));
        assert!(matches!(
            crate::paths::write_file(&mut vfs, other_caller, alias, b"payload"),
            VfsResponse::Err(crate::paths::ERR_DENIED)
        ));
        assert!(matches!(
            crate::paths::unlink_file(&mut vfs, other_caller, alias),
            VfsResponse::Err(crate::paths::ERR_DENIED)
        ));

        let other_file = vfs
            .files
            .insert(other_caller, alias, other_root.0)
            .expect("insert other file");
        let grant_req = VfsRequest::WriteHandleGrant {
            file: other_file,
            offset: 0,
            grant: 0,
            bytes: 512,
        };
        let mut req_other = [0u8; IPC_BUF_SIZE];
        api::ipc::encode(&grant_req, &mut req_other).expect("encode");
        let mut resp_other = [0u8; IPC_BUF_SIZE];
        let resp = crate::dispatch::handle_request(
            &mut vfs,
            &req_other,
            Some(other_caller),
            &mut resp_other,
        );
        assert!(matches!(resp, VfsResponse::Err(crate::paths::ERR_DENIED)));
    }
    ostd::io::println("[vfs-guest-disk] fixed-capacity-boundary PASS");
}
