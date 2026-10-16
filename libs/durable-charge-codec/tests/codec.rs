use durable_charge_codec::{decode,encode,schema_id,Error,Header,Limits};
use durable_charge_codec::references::{validate_references,Dimension,Profile,GraphScratch,AdmissionRequirement,Ref};
use sha2::{Digest,Sha256};
const KINDS:[u16;27]=[1,2,3,4,5,6,7,8,9,10,11,32,33,34,35,36,48,49,50,51,52,53,54,55,56,57,58];
fn limits()->Limits {Limits{max_record_bytes:65536,max_entries:64,max_records:64,max_reference_depth:64,max_total_bytes:1048576,max_work:16777216}}
fn header(kind:u16)->Header {
    let obligation=matches!(kind,1|10|34|48|49|50|51|52|56|57|58);
    let adoption=matches!(kind,2..=9|11|33|35|53|54|55);
    Header{journal_ledger:[1;32],journal_genesis:[2;32],obligation_id:if obligation {[9;16]}else{[0;16]},
        adoption_id:if adoption {[8;16]}else{[0;16]},local_transaction:[3;16],local_revision:2,journal_writer_epoch:1,previous_record_digest:[4;32]}
}
fn put16(p:&mut[u8],o:usize,v:u16){p[o..o+2].copy_from_slice(&v.to_le_bytes())}
fn put32(p:&mut[u8],o:usize,v:u32){p[o..o+4].copy_from_slice(&v.to_le_bytes())}
fn put64(p:&mut[u8],o:usize,v:u64){p[o..o+8].copy_from_slice(&v.to_le_bytes())}
fn reference(p:&mut[u8],o:usize,k:u16){p[o..o+40].fill(7);put32(p,o,308);put16(p,o+4,k);put16(p,o+6,0)}
fn wm(p:&mut[u8],o:usize,h:&Header){p[o..o+32].copy_from_slice(&h.journal_ledger);p[o+32..o+64].copy_from_slice(&h.journal_genesis);
    put64(p,o+64,h.local_revision-1);p[o+72..o+104].copy_from_slice(&h.previous_record_digest);reference(p,o+104,600)}
fn vec_ref(p:&mut[u8],o:usize){p[o..o+32].fill(1);reference(p,o+32,32)}
fn map(p:&mut[u8],v2:bool){let keys=if v2 {[(0,1),(0,3),(0,4),(0,6),(0,7),(1,2),(1,5),(1,8)]}else{[(0,2),(0,4),(0,6),(0,7),(0,11),(1,3),(1,5),(1,8)]};
    put32(p,124,8);p[108..124].fill(8);
    for (i,(side,k)) in keys.iter().enumerate(){let o=128+20*i;p[o]=*side;p[o+1]=*k;p[o+2..o+4].fill(0);p[o+4..o+20].fill(10+i as u8);}}
fn payload(kind:u16,h:&Header)->Vec<u8>{
    let len=match kind {1=>716,2=>580,3=>476,4=>556,5=>516,6|7|8=>372,9=>620,10=>644,11=>516,32=>60,33=>744,34=>20,35=>288,36=>448,
        48|58=>1060,49=>1212,50=>764,51=>216,52=>76,53=>904,54=>296,55=>644,56=>720,57=>516,_=>unreachable!()};
    let mut p=vec![1u8;len];
    match kind {
        1=>{p[228..244].fill(0);for o in [492,564]{vec_ref(&mut p,o)}for (o,k) in [(452,600),(636,36),(676,34)]{reference(&mut p,o,k)}}
        2=>{p[212..316].fill(2);for (o,k) in [(316,33),(356,35),(396,36)]{reference(&mut p,o,k)}wm(&mut p,436,h)}
        3=>{for (o,k) in [(108,2),(148,33),(292,36)]{reference(&mut p,o,k)}wm(&mut p,332,h)}
        4=>{for (o,k) in [(108,2),(148,3),(188,33),(332,600),(372,11)]{reference(&mut p,o,k)}wm(&mut p,412,h)}
        5=>{for (o,k) in [(108,4),(148,33),(292,600),(332,600)]{reference(&mut p,o,k)}wm(&mut p,372,h)}
        6|7|8=>{let ks=match kind {6=>[5,4,33],7=>[2,33,35],_=>[7,3,33]};for (i,k) in ks.iter().enumerate(){reference(&mut p,108+40*i,*k)}wm(&mut p,228,h)}
        9=>{p[212..316].fill(2);for (o,k) in [(316,33),(356,35),(396,600),(436,600)]{reference(&mut p,o,k)}wm(&mut p,476,h)}
        10=>{p[212..228].fill(0);p[228..244].fill(2);for (o,k) in [(244,1),(284,600)]{reference(&mut p,o,k)}vec_ref(&mut p,428);wm(&mut p,500,h)}
        11=>{for (o,k) in [(108,2),(148,3),(188,33),(228,35)]{reference(&mut p,o,k)}wm(&mut p,372,h)}
        32=>{put32(&mut p,32,1);put16(&mut p,36,1);put16(&mut p,38,1);p[40..44].fill(0);put64(&mut p,44,0);put64(&mut p,52,1)}
        33|53=>{p[212..316].fill(2);wm(&mut p,316,h);put32(&mut p,460,1);p[464..480].fill(9);reference(&mut p,568,1);
            if kind==33 {for (o,k) in [(608,32),(648,32),(688,34)]{reference(&mut p,o,k)}p[736..744].fill(0)}
            else{for (o,k) in [(608,48),(648,32),(688,32),(728,32),(768,52),(808,34),(848,36)]{reference(&mut p,o,k)}}}
        34=>{p[..16].copy_from_slice(&h.obligation_id);put32(&mut p,16,0)}
        35=>map(&mut p,false),
        36=>{reference(&mut p,212,1);put32(&mut p,252,8);for i in 0..8 {let o=256+24*i;put16(&mut p,o,i as u16+1);put16(&mut p,o+2,1);p[o+4..o+8].fill(0)}}
        48|58=>{for (o,k) in [(212,1),(356,51),(396,48),(756,1),(796,51),(836,52),(876,36)]{reference(&mut p,o,k)}
            put64(&mut p,436,if kind==48 {1}else{2});put64(&mut p,444,1);put16(&mut p,452,if kind==48 {1}else{10});put16(&mut p,454,if kind==48 {1}else{6});
            if kind==48 {p[396..436].fill(0)}else{reference(&mut p,756,57)}p[456..460].fill(0);for o in [460,532,604]{vec_ref(&mut p,o)}p[676..756].fill(0);wm(&mut p,916,h)}
        49=>{p[212..228].fill(2);for (o,k) in [(228,1),(268,48),(516,600),(1060,51),(1100,36)]{reference(&mut p,o,k)}
            for o in [556,628,700,772,844,916,988]{vec_ref(&mut p,o)}for (i,o) in [1148,1164,1180,1196].iter().enumerate(){p[*o..*o+16].fill(20+i as u8)}}
        50=>{p[212..228].fill(2);put16(&mut p,228,1);p[230..232].fill(0);for (o,k) in [(232,1),(272,49),(312,48),(640,51),(680,36)]{reference(&mut p,o,k)}
            for o in [352,424,496,568]{vec_ref(&mut p,o)}p[728..744].fill(20);p[744..760].fill(21);p[760..764].fill(0)}
        51=>{p[108..124].copy_from_slice(&h.obligation_id);reference(&mut p,124,1);put32(&mut p,164,1);put16(&mut p,168,1);p[170..172].fill(0);reference(&mut p,172,600);p[212..216].fill(0)}
        52=>{p[..16].copy_from_slice(&h.obligation_id);reference(&mut p,16,1);put32(&mut p,72,0)}
        54=>{map(&mut p,true);put32(&mut p,288,0);put32(&mut p,292,0)}
        55=>{p[108..124].copy_from_slice(&h.adoption_id);put16(&mut p,124,1);p[126..128].fill(0);p[232..336].fill(2);
            for (o,k) in [(336,53),(376,54),(416,36),(456,51)]{reference(&mut p,o,k)}wm(&mut p,496,h);put32(&mut p,640,0)}
        56=>{put16(&mut p,212,1);put16(&mut p,214,3);p[216..232].fill(2);for o in [232,272]{reference(&mut p,o,48)}p[312..392].fill(0);
            wm(&mut p,392,h);reference(&mut p,536,51);reference(&mut p,680,600)}
        57=>{p[212..228].fill(2);for (o,k) in [(228,1),(268,48),(380,51),(420,36)]{reference(&mut p,o,k)}vec_ref(&mut p,308);
            p[468..484].fill(20);p[484..500].fill(21);p[500..516].fill(0)}
        _=>unreachable!()
    }p
}
fn encoded(kind:u16)->Vec<u8>{let h=header(kind);let p=payload(kind,&h);let mut out=vec![0;p.len()+248];let n=encode(kind,&h,&p,&mut out,&limits()).unwrap();assert_eq!(n,out.len());out}
fn rehash(b:&mut[u8]){let n=b.len()-32;let mut h=Sha256::new();h.update(b"cellos/durable-charge/record/v1\0");h.update(&b[..n]);b[n..].copy_from_slice(&h.finalize())}
#[test] fn all_twenty_seven_kinds_roundtrip(){for kind in KINDS {let bytes=encoded(kind);let r=decode(&bytes,&limits()).unwrap();assert_eq!(r.kind,kind);assert_eq!(r.payload,payload(kind,&header(kind)));assert_eq!(&bytes[16..48],&schema_id(kind).unwrap());}}
#[test] fn every_payload_boundary_rejects_without_buffer_writes(){for k in KINDS {let h=header(k);let p=payload(k,&h);for n in [p.len()-1,p.len()+1] {let mut changed=p.clone();changed.resize(n,0);let mut out=vec![0x55;4096];assert!(encode(k,&h,&changed,&mut out,&limits()).is_err(),"kind {k}");assert!(out.iter().all(|v|*v==0x55));}}}
#[test] fn valid_checksum_does_not_hide_schema_flags_identity_or_watermark(){for (offset,value,error) in [(16,0,Error::Schema),(12,1,Error::Reserved),(112,0,Error::Identity)] {
    let mut b=encoded(1);if offset==112 {b[112..128].fill(0)}else{b[offset]=value}rehash(&mut b);assert_eq!(decode(&b,&limits()),Err(error));}
    let mut b=encoded(2);b[216+436]=0;rehash(&mut b);assert_eq!(decode(&b,&limits()),Err(Error::Watermark));}
#[test] fn unknown_kind_version_and_trailing_data_refuse(){let mut b=encoded(32);b[6..8].copy_from_slice(&31u16.to_le_bytes());rehash(&mut b);assert_eq!(decode(&b,&limits()),Err(Error::Kind));
    let mut b=encoded(32);b[4]=2;rehash(&mut b);assert_eq!(decode(&b,&limits()),Err(Error::Version));let mut b=encoded(32);b.push(0);assert_eq!(decode(&b,&limits()),Err(Error::Length));}
#[test] fn canonical_vector_and_profile_equalities(){let h=header(32);let mut p=payload(32,&h);p.extend_from_within(36..60);put32(&mut p,32,2);let mut out=vec![0;1024];assert_eq!(encode(32,&h,&p,&mut out,&limits()),Err(Error::NonCanonical));
    let h=header(49);let mut p=payload(49,&h);p[772]=2;assert_eq!(encode(49,&h,&p,&mut out,&limits()),Err(Error::Profile));}
#[test] fn transaction_maps_and_operation_keys_are_unique(){for k in [35,54] {let h=header(k);let mut p=payload(k,&h);p[152..168].copy_from_slice(&[10;16]);assert_eq!(encode(k,&h,&p,&mut vec![0;4096],&limits()),Err(Error::Duplicate));}
    let h=header(49);let mut p=payload(49,&h);p[212..228].copy_from_slice(&[1;16]);assert_eq!(encode(49,&h,&p,&mut vec![0;4096],&limits()),Err(Error::Duplicate));}
#[test] fn registry_rejects_duplicate_cross_class_keys(){let h=header(52);let mut p=payload(52,&h);p.resize(204,0);put32(&mut p,72,2);
    for i in 0..2 {let o=76+64*i;put16(&mut p,o,i as u16+1);p[o+4..o+20].fill(2);reference(&mut p,o+20,if i==0 {49}else{50})}
    assert_eq!(encode(52,&h,&p,&mut vec![0;4096],&limits()),Err(Error::NonCanonical));}
#[test] fn counters_lengths_and_work_are_finite(){let h=header(32);let mut p=payload(32,&h);put32(&mut p,32,u32::MAX);assert_eq!(encode(32,&h,&p,&mut vec![0;4096],&limits()),Err(Error::Limits));
    let mut l=limits();l.max_work=250;assert!(matches!(decode(&encoded(32),&l),Err(Error::Limits|Error::WorkExhausted)));
    let mut h=header(2);h.local_revision=u64::MAX;let mut p=payload(2,&h);put64(&mut p,500,u64::MAX);assert_eq!(encode(2,&h,&p,&mut vec![0;4096],&limits()),Err(Error::Watermark));}
#[test] fn exact_reference_length_kind_digest_and_reserved(){let b=encoded(32);let r=decode(&b,&limits()).unwrap();let mut bytes=[0;40];r.reference_bytes(&mut bytes);assert!(Ref::parse(&bytes).unwrap().matches(&r));bytes[0]^=1;assert!(!Ref::parse(&bytes).unwrap().matches(&r));bytes[6]=1;assert_eq!(Ref::parse(&bytes),Err(Error::Reserved));}
#[test] fn bounded_graph_accepts_only_exact_admitted_profile(){let b=encoded(32);let retained=[b.as_slice()];let dimensions=[Dimension{dimension:1,unit:1,revision:1}];let profiles=[Profile{schema:[1;32],dimensions:&dimensions}];
    let mut scratch=[GraphScratch::default();1];let mut stack=[0;64];let report=validate_references(0,&retained,&limits(),&mut scratch,&mut stack,&profiles).unwrap();assert_eq!(report.reachable_records,1);
    assert_eq!(report.admission_requirement(),AdmissionRequirement::AuthenticatedCurrentState);
    assert_eq!(validate_references(0,&retained,&limits(),&mut scratch,&mut stack,&[]),Err(Error::AdmissionRequired(AdmissionRequirement::AdmittedProfile)));
    let wrong=[Dimension{dimension:1,unit:2,revision:1}];assert_eq!(validate_references(0,&retained,&limits(),&mut scratch,&mut stack,&[Profile{schema:[1;32],dimensions:&wrong}]),Err(Error::Profile));}
#[test] fn missing_retained_dependencies_and_reused_identity_refuse(){let b=encoded(1);let mut scratch=[GraphScratch::default();2];let mut stack=[0;64];assert_eq!(validate_references(0,&[&b],&limits(),&mut scratch,&mut stack,&[]),Err(Error::MissingReference));
    let b=encoded(32);let mut other=b.clone();other[216+44]=1;rehash(&mut other);assert_eq!(validate_references(0,&[&b,&other],&limits(),&mut scratch,&mut stack,&[]),Err(Error::Duplicate));}
#[test] fn altered_cycle_edges_cannot_bypass_integrity(){let mut b=encoded(1);let digest=b[b.len()-32..].to_vec();b[216+644..216+676].copy_from_slice(&digest);assert_eq!(decode(&b,&limits()),Err(Error::Digest));}
#[test] fn external_proof_schemas_never_become_generic_digest_permission(){let h=header(51);let mut p=payload(51,&h);let subject=encoded(32);let r=decode(&subject,&limits()).unwrap();let mut reference_bytes=[0;40];r.reference_bytes(&mut reference_bytes);
    // A vector cannot substitute for an evidence subject even with its genuine checksum.
    p[124..164].copy_from_slice(&reference_bytes);assert_eq!(encode(51,&h,&p,&mut vec![0;4096],&limits()),Err(Error::Role));}

fn with_header(kind:u16,h:&Header,p:&[u8])->Vec<u8> {
    let mut bytes=vec![0;p.len()+248];encode(kind,h,p,&mut bytes,&limits()).unwrap();bytes
}
fn bind(p:&mut[u8],offset:usize,bytes:&[u8]) {
    let record=decode(bytes,&limits()).unwrap();let mut reference=[0;40];record.reference_bytes(&mut reference);
    p[offset..offset+40].copy_from_slice(&reference);
}
#[test] fn genuine_referent_with_wrong_outer_profile_refuses() {
    let mut vh=header(32);vh.local_revision=3;vh.local_transaction=[6;16];
    let mut vp=payload(32,&vh);vp[..32].fill(2);let vector=with_header(32,&vh,&vp);
    let h=header(1);let mut p=payload(1,&h);bind(&mut p,524,&vector);bind(&mut p,596,&vector);
    let reserve=with_header(1,&h,&p);let mut scratch=[GraphScratch::default();2];let mut stack=[0;64];
    assert_eq!(validate_references(0,&[&reserve,&vector],&limits(),&mut scratch,&mut stack,&[]),Err(Error::Profile));
}
#[test] fn ownership_rejects_cross_obligation_facts_even_with_genuine_checksum() {
    let reserve=encoded(1);
    let mut fh=header(34);fh.obligation_id=[10;16];fh.local_revision=3;fh.local_transaction=[6;16];
    let facts=with_header(34,&fh,&payload(34,&fh));
    let mut mh=header(33);mh.local_revision=4;mh.local_transaction=[7;16];
    let mut p=payload(33,&mh);bind(&mut p,568,&reserve);bind(&mut p,688,&facts);
    let manifest=with_header(33,&mh,&p);let mut scratch=[GraphScratch::default();3];let mut stack=[0;64];
    assert_eq!(validate_references(0,&[&manifest,&reserve,&facts],&limits(),&mut scratch,&mut stack,&[]),Err(Error::Binding));
}
#[test] fn unavailable_external_fact_schema_is_typed_admission_refusal() {
    let h=header(34);let mut p=payload(34,&h);p.resize(68,0);put32(&mut p,16,1);put16(&mut p,20,1);
    reference(&mut p,24,600);let bytes=with_header(34,&h,&p);
    let mut scratch=[GraphScratch::default();1];let mut stack=[0;64];
    assert_eq!(validate_references(0,&[&bytes],&limits(),&mut scratch,&mut stack,&[]),
        Err(Error::AdmissionRequired(AdmissionRequirement::ExternalProofSchema{kind:600})));
}
#[test] fn every_variable_count_is_bounded_before_entry_walk() {
    for (kind,offset) in [(32,32),(33,460),(34,16),(35,124),(36,252),(51,164),(52,72),(53,460),(54,124),(55,640)] {
        let h=header(kind);let mut p=payload(kind,&h);put32(&mut p,offset,65);
        assert!(encode(kind,&h,&p,&mut vec![0;4096],&limits()).is_err(),"kind {kind}");
    }
    for offset in [288,292] {let h=header(54);let mut p=payload(54,&h);put32(&mut p,offset,65);
        assert!(encode(54,&h,&p,&mut vec![0;4096],&limits()).is_err());}
}
#[test] fn role_reserved_and_payload_identity_corruption_cannot_hide_behind_checksum() {
    for (kind,offset,error) in [(51,168,Error::Role),(34,0,Error::Binding),(48,456,Error::Reserved),(55,126,Error::Reserved)] {
        let mut bytes=encoded(kind);bytes[216+offset]=if kind==51 {13}else{2};rehash(&mut bytes);
        assert_eq!(decode(&bytes,&limits()),Err(error));
    }
}
#[test] fn schema_constants_match_the_exact_candidate_names() {
    for kind in KINDS {let mut hash=Sha256::new();hash.update(b"cellos/durable-charge/schema/v1/");
        hash.update(durable_charge_codec::record::kind_name(kind).unwrap().as_bytes());
        let expected:[u8;32]=hash.finalize().into();assert_eq!(schema_id(kind),Some(expected));
    }
}
