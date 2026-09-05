#[path = "../../../cells/drivers/nvme/src/dma.rs"]
mod dma;

use dma::AuthorizedDma;

struct DummyDma {
    physical: u64,
}

#[test]
fn retains_nonidentity_iova_separately_from_cpu_address() {
    let dma = DummyDma {
        physical: 0x0010_0000,
    };
    let authorized = AuthorizedDma::authorize(dma, |_| Ok::<u64, ()>(0x8010_0000))
        .expect("authorization should succeed");

    assert_eq!(authorized.inner().physical, 0x0010_0000);
    assert_eq!(authorized.iova(), 0x8010_0000);
    assert_ne!(authorized.iova(), authorized.inner().physical);
}

#[test]
fn rejection_never_constructs_an_authorized_dma_value() {
    let result =
        AuthorizedDma::authorize(DummyDma { physical: 0x1000 }, |_| Err::<u64, _>("denied"));

    assert!(matches!(result, Err("denied")));
}

#[test]
fn nvme_device_addresses_never_fall_back_to_cpu_physical_addresses() {
    let sources = [
        (
            "controller.rs",
            include_str!("../../../cells/drivers/nvme/src/controller.rs"),
        ),
        (
            "dispatch.rs",
            include_str!("../../../cells/drivers/nvme/src/dispatch.rs"),
        ),
        (
            "main.rs",
            include_str!("../../../cells/drivers/nvme/src/main.rs"),
        ),
        (
            "queue.rs",
            include_str!("../../../cells/drivers/nvme/src/queue.rs"),
        ),
    ];

    for (name, source) in sources {
        assert!(
            !source.contains(".phys()"),
            "{name} must use the IOVA returned by DMA authorization"
        );
        assert!(
            !source.contains(".virt() as u64"),
            "{name} must not cast a CPU mapping into a device address"
        );
    }

    let controller = include_str!("../../../cells/drivers/nvme/src/controller.rs");
    for required in [
        "admin.sq_iova()",
        "admin.cq_iova()",
        "id_buf.iova()",
        "ctrl.io.cq_iova()",
        "ctrl.io.sq_iova()",
    ] {
        assert!(
            controller.contains(required),
            "controller must program authorized address via {required}"
        );
    }

    let dispatch = include_str!("../../../cells/drivers/nvme/src/dispatch.rs");
    assert_eq!(
        dispatch.matches("io_buf.iova()").count(),
        2,
        "read and write PRP1 must both use the authorized I/O-buffer IOVA"
    );
}
