//! Public deterministic host key and invented fixture policy, NEVER native assignments.
use durable_charge_codec::{schema_id,Limits,Record};
use durable_charge_codec::proof::{self,Bindings,Lane,Owner,Purpose,RecordBinding,Role,Side,Status,VerifyContext,BODY_LEN,CARRIER_LEN,SIGNED_LEN,SIGNING_DOMAIN};
use ed25519_compact::{KeyPair,Seed};
fn owner(n:u8)->Owner {Owner{principal:[n;32],incarnation:[n+1;32],home:[n+2;32],epoch:3}}
fn put(out:&mut[u8],at:usize,bytes:&[u8]) {out[at..at+bytes.len()].copy_from_slice(bytes)}
fn put_owner(out:&mut[u8],at:usize,o:&Owner) {put(out,at,&o.principal);put(out,at+32,&o.incarnation);put(out,at+64,&o.home);put(out,at+96,&o.epoch.to_le_bytes())}
fn put_ref(out:&mut[u8],at:usize,r:&RecordBinding) {put(out,at,&r.encoded_length.to_le_bytes());put(out,at+4,&r.kind.to_le_bytes());put(out,at+8,&r.digest)}
pub fn run(record:&Record<'_>,limits:Limits) {
    let keys=KeyPair::from_seed(Seed::new([11;32]));
    let mut public_key=[0;32];public_key.copy_from_slice(&keys.pk[..]);
    let h=&record.header;
    let context=VerifyContext{public_key,max_carrier_bytes:CARRIER_LEN,record_limits:limits,expected:Bindings{
        role:Role::OwnerCommit,purpose:Purpose::CommitResult,lane:Lane::Fixture,side:Side::Local,status:Status::Committed,
        producer:owner(1),key_id:[4;32],roster_id:[5;32],roster_revision:6,revocation_revision:7,audience:[8;32],
        admission_decision:[9;32],admission_revision:10,policy_id:[11;32],policy_revision:12,application:[13;32],
        workload:[14;32],scope_purpose:[15;32],journal:h.journal_ledger,genesis:h.journal_genesis,volume:[16;32],floor_namespace:[17;32],
        obligation:h.obligation_id,adoption:h.adoption_id,transaction:h.local_transaction,origin_journal:[0;32],origin_genesis:[0;32],
        origin_transaction:[0;16],origin_revision:0,create_key:[0;16],operation_key:[0;16],release_key:[0;16],transition_map:[0;32],
        request_schema:[26;32],request_length:27,request_digest:[28;32],effect:[29;32],profile:record.payload[..32].try_into().unwrap(),
        destination:[30;32],requesting_endpoint:[31;32],responding_endpoint:[32;32],correlation:[33;32],provenance:owner(34),custodian:owner(37),
        writer_epoch:h.journal_writer_epoch,authority_fence:40,controller_epoch:41,custody_revision:42,allocation_revision:0,
        input_revision:h.local_revision-1,output_revision:h.local_revision,input_floor_revision:43,output_floor_revision:44,
        input_record:RecordBinding{encoded_length:308,kind:32,digest:h.previous_record_digest},
        record:RecordBinding{encoded_length:record.bytes.len() as u32,kind:record.kind,digest:record.digest},record_schema:schema_id(record.kind).unwrap(),
        coverage_roster:[45;32],coverage_revision:46,coverage_count:1,checkpoint:[47;32],backend_admission:[48;32],failure_model:[49;32],
    }};
    let b=&context.expected;let mut out=[0u8;CARRIER_LEN];
    put(&mut out,0,b"NCF1");put(&mut out,4,&1u16.to_le_bytes());put(&mut out,6,&1u16.to_le_bytes());
    put(&mut out,8,&(CARRIER_LEN as u32).to_le_bytes());put(&mut out,12,&(BODY_LEN as u32).to_le_bytes());
    for (offset,value) in [(16,b.role as u16),(18,b.purpose as u16),(20,b.lane as u16),(22,b.side as u16),(24,b.status as u16),(26,1)] {put(&mut out,offset,&value.to_le_bytes())}
    put_owner(&mut out,32,&b.producer);put(&mut out,136,&b.key_id);put(&mut out,168,&b.roster_id);
    put(&mut out,200,&b.roster_revision.to_le_bytes());put(&mut out,208,&b.revocation_revision.to_le_bytes());
    put(&mut out,216,&b.audience);put(&mut out,248,&b.admission_decision);put(&mut out,280,&b.admission_revision.to_le_bytes());
    put(&mut out,288,&b.policy_id);put(&mut out,320,&b.policy_revision.to_le_bytes());put(&mut out,328,&b.application);
    put(&mut out,360,&b.workload);put(&mut out,392,&b.scope_purpose);put(&mut out,424,&b.journal);put(&mut out,456,&b.genesis);
    put(&mut out,488,&b.volume);put(&mut out,520,&b.floor_namespace);put(&mut out,552,&b.obligation);put(&mut out,568,&b.adoption);
    put(&mut out,584,&b.transaction);put(&mut out,600,&b.origin_journal);put(&mut out,632,&b.origin_genesis);put(&mut out,664,&b.origin_transaction);
    put(&mut out,680,&b.origin_revision.to_le_bytes());put(&mut out,688,&b.create_key);put(&mut out,704,&b.operation_key);put(&mut out,720,&b.release_key);
    put(&mut out,736,&b.transition_map);put(&mut out,768,&b.request_schema);put(&mut out,800,&b.request_length.to_le_bytes());
    put(&mut out,808,&b.request_digest);put(&mut out,840,&b.effect);put(&mut out,872,&b.profile);put(&mut out,904,&b.destination);
    put(&mut out,936,&b.requesting_endpoint);put(&mut out,968,&b.responding_endpoint);put(&mut out,1000,&b.correlation);
    put_owner(&mut out,1032,&b.provenance);put_owner(&mut out,1136,&b.custodian);
    for (offset,value) in [(1240,b.writer_epoch),(1248,b.authority_fence),(1256,b.controller_epoch),(1264,b.custody_revision),(1272,b.allocation_revision),
        (1280,b.input_revision),(1288,b.output_revision),(1296,b.input_floor_revision),(1304,b.output_floor_revision)] {put(&mut out,offset,&value.to_le_bytes())}
    put_ref(&mut out,1312,&b.input_record);put_ref(&mut out,1352,&b.record);put(&mut out,1392,&b.record_schema);
    put(&mut out,1424,&b.coverage_roster);put(&mut out,1456,&b.coverage_revision.to_le_bytes());put(&mut out,1464,&b.coverage_count.to_le_bytes());
    put(&mut out,1468,&b.checkpoint);put(&mut out,1500,&b.backend_admission);put(&mut out,1532,&b.failure_model);put(&mut out,1564,&context.public_key);
    let mut signed=[0;SIGNING_DOMAIN.len()+SIGNED_LEN];signed[..SIGNING_DOMAIN.len()].copy_from_slice(SIGNING_DOMAIN);
    signed[SIGNING_DOMAIN.len()..].copy_from_slice(&out[..SIGNED_LEN]);let signature=keys.sk.sign(signed,None);out[SIGNED_LEN..].copy_from_slice(&signature[..]);
    let verified=proof::verify(&out,record,&context).expect("host-only fixture Ed25519 verification");
    assert_eq!(verified.role(),Role::OwnerCommit);assert_eq!(verified.purpose(),Purpose::CommitResult);
    println!("NCF1 host-fixture Ed25519 verification completed; carrier_bytes={CARRIER_LEN}; fixture statement only, no native signer/pin/backend qualification");
}
