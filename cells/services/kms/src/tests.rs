use api::caller_identity::CallerIdentity;
use types::kms::{BrokerBindingPayload, KmsErrorCode, KmsOpcode, KmsRequestV1, KmsResponseV1};

use super::*;

mod authorization;
mod operations;
mod root;
mod root_invariants;
mod root_sequence;
mod storage;
mod tls_signing;

fn caller(cell_id: u64, generation: u64, sender_tid: usize) -> CallerIdentity {
    CallerIdentity {
        cell_id,
        generation,
        sender_tid: sender_tid as u64,
    }
}

fn registry(broker: Option<usize>, supervisor: Option<usize>) -> ServiceRegistrySnapshot {
    ServiceRegistrySnapshot {
        net_broker_tid: broker,
        supervisor_tid: supervisor,
        net_tid: None,
    }
}

fn net_registry(net: Option<usize>) -> ServiceRegistrySnapshot {
    ServiceRegistrySnapshot {
        net_broker_tid: None,
        supervisor_tid: Some(8),
        net_tid: net,
    }
}

fn request(opcode: KmsOpcode, payload: &[u8]) -> [u8; 128] {
    KmsRequestV1::new(opcode, 41, payload).unwrap().to_bytes()
}

fn bind(service: &mut KmsService, tid: usize) -> BrokerBindingPayload {
    let response = service
        .handle(
            &request(KmsOpcode::RegisterBrokerInstance, &[]),
            tid,
            Some(caller(20, 30, tid)),
            registry(Some(tid), Some(8)),
        )
        .unwrap();
    assert_eq!(response.error_code().unwrap(), None);
    BrokerBindingPayload::decode(response.payload().unwrap()).unwrap()
}

fn assert_error(response: KmsResponseV1, expected: KmsErrorCode) {
    assert_eq!(response.error_code().unwrap(), Some(expected));
    assert!(response.payload().unwrap().is_empty());
}
