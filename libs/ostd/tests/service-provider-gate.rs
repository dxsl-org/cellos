//! Host tests for the peer-service sender gate NIC driver cells apply to every
//! Tx/Rx/GetMac request (audit H-20): only the tid the kernel's registry names
//! as the live provider may drive the NIC.

use api::service_binding::ServiceBinding;
use ostd::service::binding_names_sender;

const NET_TID: usize = 41;

#[test]
fn admits_the_live_provider_tid() {
    assert!(binding_names_sender(
        Some(ServiceBinding::new(NET_TID as u64, 9, 3)),
        NET_TID
    ));
}

#[test]
fn refuses_any_other_sender() {
    let live = Some(ServiceBinding::new(NET_TID as u64, 9, 3));
    assert!(!binding_names_sender(live, NET_TID + 1));
    assert!(!binding_names_sender(live, 0));
}

#[test]
fn refuses_everyone_while_the_provider_is_unregistered() {
    // Init registers the net service only after spawning it, and a hot-swap hides
    // the binding: no binding must never mean "anyone".
    assert!(!binding_names_sender(None, NET_TID));
}

#[test]
fn refuses_a_binding_that_names_no_live_provider() {
    assert!(!binding_names_sender(
        Some(ServiceBinding::new(NET_TID as u64, 0, 3)),
        NET_TID
    ));
    assert!(!binding_names_sender(
        Some(ServiceBinding::new(NET_TID as u64, 9, 0)),
        NET_TID
    ));
}
