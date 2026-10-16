//! Host fixture only: actual encode/decode/retained-profile checks, not native durability.
use durable_charge_codec::{decode,encode,schema_id,Header,Limits};
use durable_charge_codec::record::PayloadView;
use durable_charge_codec::references::{validate_references,Dimension,Profile,GraphScratch};

fn main() {
    let limits=Limits{max_record_bytes:4096,max_entries:8,max_records:8,max_reference_depth:8,max_total_bytes:32768,max_work:100000};
    let header=Header{journal_ledger:[1;32],journal_genesis:[2;32],obligation_id:[0;16],adoption_id:[0;16],
        local_transaction:[3;16],local_revision:2,journal_writer_epoch:1,previous_record_digest:[4;32]};
    let mut payload=[0u8;60];
    payload[..32].fill(5);
    payload[32..36].copy_from_slice(&1u32.to_le_bytes());
    payload[36..38].copy_from_slice(&1u16.to_le_bytes());
    payload[38..40].copy_from_slice(&1u16.to_le_bytes());
    payload[44..52].copy_from_slice(&7u64.to_le_bytes());
    payload[52..60].copy_from_slice(&1u64.to_le_bytes());
    let mut bytes=[0u8;308];
    let length=encode(32,&header,&payload,&mut bytes,&limits).expect("synthetic vector encode");
    let record=decode(&bytes[..length],&limits).expect("synthetic vector decode");
    assert_eq!(record.header,header);
    assert_eq!(record.payload,&payload);
    assert_eq!(&record.bytes[16..48],&schema_id(32).expect("known schema"));
    match record.payload_view(&limits).expect("borrowed payload view") {
        PayloadView::VectorManifest{profile,entries}=>{assert_eq!(profile,&[5;32]);assert_eq!(entries.len(),24);}
        _=>panic!("wrong payload view"),
    }
    let dimensions=[Dimension{dimension:1,unit:1,revision:1}];
    let profiles=[Profile{schema:[5;32],dimensions:&dimensions}];
    let retained=[&bytes[..length]];
    let mut scratch=[GraphScratch::default();1];let mut stack=[0;8];
    let graph=validate_references(0,&retained,&limits,&mut scratch,&mut stack,&profiles).expect("synthetic vector dependency check");
    assert_eq!(graph.reachable_records,1);
    println!("DCO1 synthetic kind={} bytes={} profile_dimensions=1; encode/decode/reference checks completed",record.kind,length);
    println!("Integrity/profile consistency only; {:?} remains required, no native permission or durability asserted",graph.admission_requirement());
}
