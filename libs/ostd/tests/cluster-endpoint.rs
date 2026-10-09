use api::services::cluster::{CellNetId, ClusterId};
use ostd::cluster_endpoint::{
    CellEndpoint, CellMethod, EndpointError, LocalEndpoint, RemoteCallError, RemoteEndpoint,
};
use types::c2c::{RelativeDeadline, RetryClass, ServerEpoch};

struct Ping;

impl CellMethod for Ping {
    type Request = ();
    type Response<'a> = ();

    const SERVICE_ID: u16 = 7;
    const EXPORT_ID: u16 = 9;
    const RETRY_CLASS: RetryClass = RetryClass::Idempotent;
}

#[test]
fn local_endpoint_rejects_zero_tid() {
    assert!(matches!(
        LocalEndpoint::<Ping>::new(0),
        Err(EndpointError::InvalidLocalTid)
    ));
    assert_eq!(LocalEndpoint::<Ping>::new(11).unwrap().tid(), 11);
}

#[test]
fn remote_endpoint_preserves_authenticated_metadata() {
    let node = CellNetId([0x44; 32]);
    let cluster = ClusterId(5);
    let epoch = ServerEpoch::new(6).unwrap();
    let endpoint = RemoteEndpoint::<Ping>::new(node, cluster, epoch).unwrap();
    assert_eq!(endpoint.destination(), node);
    assert_eq!(endpoint.cluster(), cluster);
    assert_eq!(endpoint.server_epoch(), epoch);
    assert_eq!(endpoint.service_id(), 7);
    assert_eq!(endpoint.export_id(), 9);
    assert_eq!(endpoint.retry_class(), RetryClass::Idempotent);
}

#[test]
fn remote_call_fails_explicitly_without_broker_contact() {
    let endpoint = RemoteEndpoint::<Ping>::new(
        CellNetId([0x44; 32]),
        ClusterId(5),
        ServerEpoch::new(6).unwrap(),
    )
    .unwrap();
    let mut response = [0u8; api::ipc::IPC_BUF_SIZE];
    assert_eq!(RelativeDeadline::new(0), None);
    assert_eq!(
        endpoint.call(&(), RelativeDeadline::new(25).unwrap(), &mut response,),
        Err(RemoteCallError::NotSupported)
    );
}

#[test]
fn remote_endpoint_rejects_zero_identity() {
    let epoch = ServerEpoch::new(1).unwrap();
    assert!(matches!(
        RemoteEndpoint::<Ping>::new(CellNetId([0; 32]), ClusterId(1), epoch),
        Err(EndpointError::InvalidRemoteIdentity)
    ));
    assert!(matches!(
        RemoteEndpoint::<Ping>::new(CellNetId([1; 32]), ClusterId(0), epoch),
        Err(EndpointError::InvalidRemoteIdentity)
    ));
}

#[test]
fn cell_endpoint_requires_an_explicit_locality_branch() {
    let endpoint = CellEndpoint::Local(LocalEndpoint::<Ping>::new(3).unwrap());
    let tid = match endpoint {
        CellEndpoint::Local(local) => local.tid(),
        CellEndpoint::Remote(_) => panic!("expected local endpoint"),
    };
    assert_eq!(tid, 3);
}

#[test]
fn local_endpoint_tid_only_constructor_claims_no_provider_identity() {
    // `new` trusts a caller-supplied tid: it resolves nothing, so the endpoint carries no
    // provider identity and cannot classify a failure against the registry. That is a
    // property of this constructor, not of the type — `LocalEndpoint::bind` resolves the
    // live binding and refuses when there is none.
    let endpoint = LocalEndpoint::<Ping>::new(42).unwrap();
    assert_eq!(endpoint.tid(), 42);
    assert_eq!(endpoint.binding(), None);
}

#[test]
fn a_failed_exchange_is_stale_when_the_descriptor_no_longer_matches() {
    use api::service_binding::ServiceBinding;
    use ostd::service::{classify_call_failure, CallFailure};

    let held = ServiceBinding::new(15, 3, 7);

    // The same live endpoint failed the exchange: send, receive or reply-identity.
    assert_eq!(
        classify_call_failure(held, Some(ServiceBinding::new(15, 3, 7))),
        CallFailure::ProviderError
    );

    // The provider Cell re-registered under a new tid: the generation is unchanged, but a
    // call addresses the tid, so the descriptor the caller holds is not the live one.
    assert_eq!(
        classify_call_failure(held, Some(ServiceBinding::new(18, 3, 7))),
        CallFailure::StaleBinding
    );

    // The Cell was replaced by a new incarnation, and nothing is registered at all.
    assert_eq!(
        classify_call_failure(held, Some(ServiceBinding::new(15, 3, 8))),
        CallFailure::StaleBinding
    );
    assert_eq!(classify_call_failure(held, None), CallFailure::StaleBinding);
}
