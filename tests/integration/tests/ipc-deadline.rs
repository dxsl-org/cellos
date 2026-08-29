#[path = "../../../libs/ostd/src/ipc/deadline.rs"]
mod deadline;

const IPC_SOURCE: &str = include_str!("../../../libs/ostd/src/ipc.rs");

#[test]
fn public_bounded_transport_uses_nonblocking_send_and_timed_receive() {
    let bounded = IPC_SOURCE
        .split("pub fn service_call_bounded")
        .nth(1)
        .and_then(|tail| tail.split("pub fn service_call_typed_bounded").next())
        .expect("bounded transport source");
    assert!(bounded.contains("sys_try_send("));
    assert!(bounded.contains("sys_recv_timeout("));
    assert!(!bounded.contains("sys_send("));
    assert!(!bounded.contains("sys_recv("));
}
