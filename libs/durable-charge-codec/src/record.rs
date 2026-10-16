use sha2::{Digest, Sha256};
use crate::{Error, Limits};
use crate::references::{Ref, AdmissionRequirement};

pub const HEADER_BYTES: usize = 216;
pub const OVERHEAD: usize = 248;
const DOMAIN: &[u8] = b"cellos/durable-charge/record/v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub journal_ledger: [u8;32],
    pub journal_genesis: [u8;32],
    pub obligation_id: [u8;16],
    pub adoption_id: [u8;16],
    pub local_transaction: [u8;16],
    pub local_revision: u64,
    pub journal_writer_epoch: u64,
    pub previous_record_digest: [u8;32],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record<'a> {
    pub kind: u16,
    pub header: Header,
    pub payload: &'a [u8],
    pub digest: [u8;32],
    pub bytes: &'a [u8],
}
impl<'a> Record<'a> {
    pub fn reference_bytes(&self, out: &mut [u8;40]) {
        out[..4].copy_from_slice(&(self.bytes.len() as u32).to_le_bytes());
        out[4..6].copy_from_slice(&self.kind.to_le_bytes());
        out[6..8].fill(0);
        out[8..].copy_from_slice(&self.digest);
    }
    pub fn admission_requirement(&self) -> AdmissionRequirement {
        AdmissionRequirement::AuthenticatedCurrentState
    }
}

pub fn kind_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        1=>"reserve",2=>"prepare_source",3=>"prepare_destination",4=>"assign_source",
        5=>"accept_destination",6=>"settle_source",7=>"abort_source",8=>"abort_destination",
        9=>"adopt_same_ledger",10=>"release_once",11=>"fence_started",
        32=>"vector_manifest",33=>"ownership_manifest",34=>"retained_facts",
        35=>"transition_map",36=>"capacity_manifest",48=>"allocation_snapshot_v2",
        49=>"resize_request_v2",50=>"allocation_adjustment_request_v2",
        51=>"allocation_evidence_set_v2",52=>"allocation_key_registry_v2",
        53=>"ownership_manifest_v2",54=>"adoption_transition_map_v2",
        55=>"adoption_transition_v2",56=>"allocation_lookup_result_v2",
        57=>"allocation_release_request_v2",58=>"allocation_release_result_v2",_=>return None,
    })
}
pub fn schema_id(kind: u16) -> Option<[u8;32]> {
    // SHA256("cellos/durable-charge/schema/v1/" || exact kind_name), no NUL.
    // Fixed candidate identities avoid hashing a known schema name on every parse.
    Some(match kind {
        1=>[28,79,173,203,69,139,74,10,109,251,152,77,89,143,219,116,181,128,143,89,213,181,143,84,237,48,229,191,255,64,85,61],
        2=>[221,7,146,119,196,89,78,76,134,8,111,94,132,199,7,218,69,207,134,201,200,219,104,161,219,172,202,186,110,148,91,152],
        3=>[175,214,129,157,6,230,88,177,208,69,42,5,241,68,90,206,115,178,11,117,103,139,105,165,190,59,126,178,189,36,149,243],
        4=>[157,32,4,138,154,56,95,161,118,210,55,3,53,227,106,211,29,222,191,32,25,144,128,76,55,161,157,178,155,33,116,204],
        5=>[16,153,129,29,231,62,29,105,21,233,237,128,181,151,146,165,227,162,11,249,116,100,111,40,168,235,128,151,107,45,94,221],
        6=>[19,125,34,143,37,92,128,186,254,52,122,177,87,146,126,243,54,97,81,69,72,223,76,154,210,82,171,70,223,32,90,16],
        7=>[236,202,14,29,42,0,175,124,110,172,163,183,149,218,47,148,174,4,80,132,104,50,194,164,18,136,176,242,145,240,3,95],
        8=>[226,118,99,53,13,121,244,85,37,44,184,21,63,180,98,245,236,22,153,129,237,17,85,49,33,212,125,149,57,127,51,239],
        9=>[74,192,222,56,254,158,73,24,159,134,192,251,65,25,136,26,174,124,254,67,233,63,1,160,131,209,8,60,31,102,174,49],
        10=>[26,93,255,173,152,131,148,110,25,43,160,194,50,173,231,44,54,252,97,58,85,73,131,215,82,247,97,36,242,75,103,122],
        11=>[232,251,15,162,81,49,92,216,98,48,247,103,124,37,59,138,32,158,219,26,171,202,117,0,54,192,195,186,172,90,0,124],
        32=>[108,191,174,96,17,88,152,162,74,149,41,17,74,43,144,174,117,168,84,151,69,3,215,16,29,138,226,93,138,186,41,90],
        33=>[54,45,99,27,195,97,198,194,214,163,87,79,237,236,157,47,83,231,93,48,49,78,255,86,204,226,169,215,184,32,202,250],
        34=>[97,218,126,75,255,46,201,147,51,114,119,179,172,141,102,181,246,233,6,238,110,43,116,199,194,224,17,142,111,5,212,180],
        35=>[12,177,235,215,121,40,119,252,80,155,126,230,115,139,84,110,146,222,219,239,184,6,71,113,239,105,59,235,39,176,114,28],
        36=>[137,11,18,224,70,25,202,185,9,169,42,81,250,204,200,19,195,236,10,128,161,222,61,174,179,102,85,99,119,68,111,55],
        48=>[164,92,69,154,144,19,23,143,58,201,227,14,199,28,14,222,47,79,129,224,84,193,152,202,244,65,21,248,236,78,86,174],
        49=>[233,144,146,165,111,221,254,9,90,39,40,16,148,231,54,30,79,41,111,87,249,58,0,20,72,237,15,237,52,132,217,214],
        50=>[253,14,184,114,45,83,176,102,20,127,176,158,147,151,98,37,149,121,22,245,248,3,128,211,203,5,101,151,100,195,172,237],
        51=>[215,116,154,243,160,248,87,203,70,174,175,203,83,179,158,86,72,63,129,69,80,111,37,174,93,120,255,91,84,120,95,42],
        52=>[146,38,121,174,140,207,89,69,244,103,176,176,193,40,98,20,247,126,92,74,162,63,253,2,60,252,58,214,27,107,124,56],
        53=>[100,57,39,235,129,239,127,1,182,247,227,204,237,169,170,29,180,21,128,173,120,181,153,66,226,195,206,228,61,213,84,90],
        54=>[221,126,126,232,44,111,77,74,171,96,50,137,26,190,178,116,244,149,163,59,35,117,248,144,80,8,14,215,145,137,173,203],
        55=>[53,153,201,241,47,83,198,96,53,65,222,243,55,176,217,209,127,189,65,170,146,132,51,44,35,91,124,55,125,42,137,46],
        56=>[140,87,82,225,253,241,43,18,144,61,91,53,141,157,245,175,125,14,185,179,36,105,153,215,37,205,168,142,21,60,204,173],
        57=>[42,85,169,91,131,51,73,36,37,129,84,32,119,228,83,115,198,58,180,135,206,220,72,166,50,20,226,159,186,152,100,44],
        58=>[31,210,158,64,246,136,19,226,124,169,140,91,9,191,218,73,246,222,11,90,180,99,203,113,36,45,95,69,7,190,150,250],
        _=>return None,
    })
}
pub(crate) fn zero(b:&[u8])->bool { b.iter().all(|v|*v==0) }
pub(crate) fn id(b:&[u8])->Result<(),Error> { if zero(b) {Err(Error::Identity)} else {Ok(())} }
pub(crate) fn u16at(b:&[u8], o:usize)->u16 { u16::from_le_bytes([b[o],b[o+1]]) }
pub(crate) fn u32at(b:&[u8], o:usize)->u32 { u32::from_le_bytes(b[o..o+4].try_into().unwrap()) }
pub(crate) fn u64at(b:&[u8], o:usize)->u64 { u64::from_le_bytes(b[o..o+8].try_into().unwrap()) }
fn positive(v:u64)->Result<(),Error> { if v==0 {Err(Error::Counter)} else {Ok(())} }
fn zeros(b:&[u8])->Result<(),Error> { if zero(b) {Ok(())} else {Err(Error::Reserved)} }
fn eq(a:&[u8],b:&[u8])->Result<(),Error> { if a==b {Ok(())} else {Err(Error::Binding)} }
fn exact(p:&[u8],n:usize)->Result<(),Error> { if p.len()==n {Ok(())} else {Err(Error::Length)} }
fn variable(p:&[u8],prefix:usize,stride:usize,count:usize,l:&Limits)->Result<(),Error> {
    if count>l.max_entries {return Err(Error::Limits)}
    let n=prefix.checked_add(stride.checked_mul(count).ok_or(Error::Overflow)?).ok_or(Error::Overflow)?;
    exact(p,n)
}
fn count(p:&[u8],o:usize)->Result<usize,Error> {
    let end=o.checked_add(4).ok_or(Error::Overflow)?;
    if p.len()<end {Err(Error::Length)} else {Ok(u32at(p,o) as usize)}
}
fn scope(p:&[u8],o:usize)->Result<(),Error> {
    for i in [0,32,64] {id(&p[o+i..o+i+32])?;}
    positive(u64at(p,o+100))
}
fn owner(p:&[u8],o:usize)->Result<(),Error> {
    for i in [0,32,64] {id(&p[o+i..o+i+32])?;}
    positive(u64at(p,o+96))
}
fn debit(p:&[u8],o:usize)->Result<(),Error> {
    id(&p[o..o+32])?;id(&p[o+32..o+64])?;id(&p[o+64..o+80])?;positive(u64at(p,o+80))
}
fn reference(p:&[u8],o:usize,kinds:&[u16],optional:bool)->Result<(),Error> {
    let b=&p[o..o+40];
    if optional && zero(b) {return Ok(())}
    let r=Ref::parse(b)?;
    if !kinds.is_empty()&&!kinds.contains(&r.kind()) {return Err(Error::Role)}
    Ok(())
}
fn vector(p:&[u8],o:usize)->Result<(),Error> {id(&p[o..o+32])?;reference(p,o+32,&[32],false)}
fn profiles(p:&[u8],offsets:&[usize])->Result<(),Error> {
    for o in offsets {vector(p,*o)?;eq(&p[*o..*o+32],&p[offsets[0]..offsets[0]+32]).map_err(|_|Error::Profile)?;} Ok(())
}
fn binding(p:&[u8],o:usize)->Result<(),Error> {
    id(&p[o..o+32])?;positive(u64at(p,o+32))?;id(&p[o+40..o+72])?;id(&p[o+72..o+104])?;
    reference(p,o+104,&[],false)
}
fn watermark(p:&[u8],o:usize,h:Option<&Header>)->Result<(),Error> {
    id(&p[o..o+32])?;id(&p[o+32..o+64])?;positive(u64at(p,o+64))?;
    id(&p[o+72..o+104])?;reference(p,o+104,&[],false)?;
    if let Some(h)=h {
        if p[o..o+32]!=h.journal_ledger || p[o+32..o+64]!=h.journal_genesis
            || p[o+72..o+104]!=h.previous_record_digest
            || u64at(p,o+64).checked_add(1)!=Some(h.local_revision) {return Err(Error::Watermark)}
    } Ok(())
}
fn unique_ids(p:&[u8],offsets:&[usize],h:&Header)->Result<(),Error> {
    for (i,o) in offsets.iter().enumerate() {
        id(&p[*o..*o+16])?;
        if p[*o..*o+16]==h.local_transaction {return Err(Error::Duplicate)}
        for a in &offsets[..i] {if p[*a..*a+16]==p[*o..*o+16] {return Err(Error::Duplicate)}}
    } Ok(())
}
fn header(kind:u16,h:&Header)->Result<(),Error> {
    id(&h.journal_ledger)?;id(&h.journal_genesis)?;id(&h.local_transaction)?;
    positive(h.local_revision)?;positive(h.journal_writer_epoch)?;
    // This schema has no implicit genesis record or revision-zero predecessor.
    id(&h.previous_record_digest)?;
    let (o,a)=match kind {1|10|34|48|49|50|52|56|57|58=>(true,false),
        2..=9|11|33|35|53|54|55=>(false,true),32|36=>(false,false),51=>{
            if !zero(&h.adoption_id) {return Err(Error::Identity)} return Ok(())
        },_=>return Err(Error::Kind)};
    if zero(&h.obligation_id)==o || zero(&h.adoption_id)==a {return Err(Error::Identity)} Ok(())
}

/// Validates all local payload structure. External proof schemas are checked by the
/// graph validator, not by pretending an integrity-valid Ref has authority.
pub(crate) fn payload(kind:u16,h:&Header,p:&[u8],l:&Limits)->Result<(),Error> {
    match kind {
        1=>{ exact(p,716)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;
            zeros(&p[228..244])?;owner(p,244)?;binding(p,348)?;
            profiles(p,&[492,564])?;reference(p,636,&[36],false)?;reference(p,676,&[34],false)?; }
        2=>{exact(p,580)?;scope(p,0)?;owner(p,108)?;owner(p,212)?;
            reference(p,316,&[33],false)?;reference(p,356,&[35],false)?;reference(p,396,&[36],false)?;watermark(p,436,Some(h))?;}
        3=>{exact(p,476)?;scope(p,0)?;reference(p,108,&[2],false)?;reference(p,148,&[33],false)?;
            owner(p,188)?;reference(p,292,&[36],false)?;watermark(p,332,Some(h))?;}
        4=>{exact(p,556)?;scope(p,0)?;reference(p,108,&[2],false)?;reference(p,148,&[3],false)?;
            reference(p,188,&[33],false)?;owner(p,228)?;reference(p,332,&[],false)?;
            reference(p,372,&[11],false)?;watermark(p,412,Some(h))?;}
        5=>{exact(p,516)?;scope(p,0)?;reference(p,108,&[4],false)?;reference(p,148,&[33],false)?;
            owner(p,188)?;reference(p,292,&[],false)?;reference(p,332,&[],false)?;watermark(p,372,Some(h))?;}
        6|7|8=>{exact(p,372)?;scope(p,0)?;
            let kinds=match kind {6=>[5,4,33],7=>[2,33,35],_=>[7,3,33]};
            for (i,k) in kinds.iter().enumerate(){reference(p,108+40*i,&[*k],false)?;}
            watermark(p,228,Some(h))?;}
        9=>{exact(p,620)?;scope(p,0)?;owner(p,108)?;owner(p,212)?;
            reference(p,316,&[33],false)?;reference(p,356,&[35],false)?;
            reference(p,396,&[],false)?;reference(p,436,&[],false)?;watermark(p,476,Some(h))?;}
        10=>{exact(p,644)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;
            zeros(&p[212..228])?;id(&p[228..244])?;
            if p[196..212]==p[228..244] {return Err(Error::Duplicate)}
            reference(p,244,&[1],false)?;reference(p,284,&[],false)?;owner(p,324)?;
            vector(p,428)?;watermark(p,500,Some(h))?;}
        11=>{exact(p,516)?;scope(p,0)?;for (o,k) in [(108,2),(148,3),(188,33),(228,35)] {reference(p,o,&[k],false)?;}
            owner(p,268)?;watermark(p,372,Some(h))?;}
        32=>{let n=count(p,32)?;variable(p,36,24,n,l)?;id(&p[..32])?;
            if n==0 {return Err(Error::Profile)}
            for i in 0..n {let o=36+24*i;let d=u16at(p,o);if d==0||u16at(p,o+2)==0 {return Err(Error::Profile)}
                zeros(&p[o+4..o+8])?;positive(u64at(p,o+16))?;
                if i>0&&u16at(p,o-24)>=d {return Err(Error::NonCanonical)}}}
        33|53=>{let n=count(p,460)?;let stride=if kind==33 {280}else{440};variable(p,464,stride,n,l)?;
            if n==0 {return Err(Error::NonCanonical)}
            scope(p,0)?;owner(p,108)?;owner(p,212)?;watermark(p,316,None)?;
            if p[108..212]==p[212..316] {return Err(Error::Identity)}
            for i in 0..n {let o=464+stride*i;id(&p[o..o+16])?;debit(p,o+16)?;
                if i>0&&p[o-stride..o-stride+16]>=p[o..o+16] {return Err(Error::NonCanonical)}
                for j in 0..i {let a=464+stride*j;if p[a+16..a+104]==p[o+16..o+104] {return Err(Error::Duplicate)}}
                reference(p,o+104,&[1],false)?;
                if kind==33 {for (r,k) in [(144,32),(184,32),(224,34)] {reference(p,o+r,&[k],false)?;}
                    positive(u64at(p,o+264))?;zeros(&p[o+272..o+280])?;
                }else{reference(p,o+144,&[48,58],false)?;
                    for (r,k) in [(184,32),(224,32),(264,32),(304,52),(344,34),(384,36)] {reference(p,o+r,&[k],false)?;}
                    positive(u64at(p,o+424))?;positive(u64at(p,o+432))?;}
            }}
        34|51=>{let (co,base)=if kind==34 {(16,20)}else{(164,168)};let n=count(p,co)?;variable(p,base,48,n,l)?;
            if kind==34 {eq(&p[..16],&h.obligation_id)?;}else{scope(p,0)?;eq(&p[108..124],&h.obligation_id)?;
                reference(p,124,&[1,48,49,53,55,58],false)?;if n==0 {return Err(Error::NonCanonical)}}
            for i in 0..n {let o=base+48*i;let role=u16at(p,o);if role==0||role>if kind==34 {8}else{12} {return Err(Error::Role)}
                zeros(&p[o+2..o+4])?;reference(p,o+4,&[],false)?;zeros(&p[o+44..o+48])?;
                if i>0 {let a=o-48;let ar=u16at(p,a);if ar>role||(ar==role&&p[a+12..a+44]>=p[o+12..o+44]) {return Err(Error::NonCanonical)}}}}
        35=>{let n=count(p,124)?;variable(p,128,20,n,l)?;scope(p,0)?;eq(&p[108..124],&h.adoption_id)?;
            transition_list(p,128,n,false,h)?;}
        36=>{let n=count(p,252)?;variable(p,256,24,n,l)?;scope(p,0)?;owner(p,108)?;reference(p,212,&[1],false)?;
            if n!=8 {return Err(Error::Profile)}
            for i in 0..n {let o=256+24*i;if u16at(p,o)!=i as u16+1||u16at(p,o+2)==0 {return Err(Error::Profile)}
                zeros(&p[o+4..o+8])?;positive(u64at(p,o+8))?;positive(u64at(p,o+16))?;}}
        48|58=>{exact(p,1060)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;reference(p,212,&[1],false)?;
            owner(p,252)?;reference(p,356,&[51,55],false)?;positive(u64at(p,436))?;positive(u64at(p,444))?;
            let t=u16at(p,452);let s=u16at(p,454);zeros(&p[456..460])?;
            if (kind==58&&(t!=10||s!=6))||(kind==48&&(!(1..=9).contains(&t)||!(1..=5).contains(&s))) {return Err(Error::Discriminant)}
            reference(p,396,&[48],t==1)?;
            if (t==1)!=(zero(&p[396..436])&&u64at(p,436)==1) {return Err(Error::Counter)}
            profiles(p,&[460,532,604])?;reference(p,676,&[49],true)?;reference(p,716,&[50,57],true)?;
            reference(p,756,&[1,49,50,55,57],false)?;reference(p,796,&[51],false)?;
            reference(p,836,&[52],false)?;reference(p,876,&[36],false)?;watermark(p,916,Some(h))?;
            let rz=!zero(&p[676..716]);let ad=!zero(&p[716..756]);
            if ((s==1||s==6)&&(rz||ad))||((s==2||s==3||s==4)&&!rz)||(s==5&&(rz||!ad)) {return Err(Error::State)}
            if s==5&&u16at(p,720)!=57 {return Err(Error::Role)}
            if ad&&s!=5&&u16at(p,720)!=50 {return Err(Error::Role)}
            let expected=match t {1=>1,2..=5=>49,6|7=>50,8=>55,9|10=>57,_=>return Err(Error::Discriminant)};
            reference(p,756,&[expected],false)?;
        }
        49=>{exact(p,1212)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;id(&p[212..228])?;
            if p[196..212]==p[212..228] {return Err(Error::Duplicate)}
            reference(p,228,&[1],false)?;reference(p,268,&[48],false)?;owner(p,308)?;binding(p,412)?;
            profiles(p,&[556,628,700,772,844,916,988])?;reference(p,1060,&[51],false)?;
            reference(p,1100,&[36],false)?;positive(u64at(p,1140))?;unique_ids(p,&[1148,1164,1180,1196],h)?;}
        50=>{exact(p,764)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;id(&p[212..228])?;
            if p[196..212]==p[212..228] {return Err(Error::Duplicate)}
            if !(1..=2).contains(&u16at(p,228)) {return Err(Error::Discriminant)}zeros(&p[230..232])?;
            for (o,k) in [(232,1),(272,49),(312,48),(640,51),(680,36)] {reference(p,o,&[k],false)?;}
            profiles(p,&[352,424,496,568])?;positive(u64at(p,720))?;unique_ids(p,&[728,744],h)?;zeros(&p[760..764])?;}
        52=>{let n=count(p,72)?;variable(p,76,64,n,l)?;eq(&p[..16],&h.obligation_id)?;
            reference(p,16,&[1],false)?;id(&p[56..72])?;
            for i in 0..n {let o=76+64*i;let c=u16at(p,o);let k=match c {1=>49,2=>50,3=>57,_=>return Err(Error::Discriminant)};
                zeros(&p[o+2..o+4])?;id(&p[o+4..o+20])?;if p[o+4..o+20]==p[56..72] {return Err(Error::Duplicate)}
                if i>0&&p[o-60..o-44]>=p[o+4..o+20] {return Err(Error::NonCanonical)}
                reference(p,o+20,&[k],false)?;zeros(&p[o+60..o+64])?;}}
        54=>{let n=count(p,124)?;if n>l.max_entries {return Err(Error::Limits)}
            let a=128usize.checked_add(n.checked_mul(20).ok_or(Error::Overflow)?).ok_or(Error::Overflow)?;
            let m=count(p,a)?;if m>l.max_entries {return Err(Error::Limits)}
            let b=a.checked_add(4).and_then(|x|m.checked_mul(32).and_then(|y|x.checked_add(y))).ok_or(Error::Overflow)?;
            let r=count(p,b)?;variable(p,b+4,76,r,l)?;scope(p,0)?;eq(&p[108..124],&h.adoption_id)?;
            transition_list(p,128,n,true,h)?;
            for i in 0..m {let o=a+4+32*i;id(&p[o..o+16])?;id(&p[o+16..o+32])?;
                if i>0&&p[o-32..o-16]>=p[o..o+16] {return Err(Error::NonCanonical)}}
            for i in 0..r {let o=b+4+76*i;id(&p[o..o+16])?;reference(p,o+16,&[49,50,57],false)?;
                let k=u16at(p,o+20);let t=u16at(p,o+56);
                if !match k {49=>(3..=5).contains(&t),50=>t==7,57=>t==10,_=>false} {return Err(Error::Role)}
                zeros(&p[o+58..o+60])?;id(&p[o+60..o+76])?;
                if i>0 {let q=o-76;let left=(&p[q..q+16],&p[q+24..q+56],u16at(p,q+56));
                    let right=(&p[o..o+16],&p[o+24..o+56],t);if left>=right {return Err(Error::NonCanonical)}}}
            // All three transaction namespaces, including the helper, are disjoint.
            let total=n.checked_add(m).and_then(|x|x.checked_add(r)).ok_or(Error::Overflow)?;
            for i in 0..total {let o=map_tx_offset(i,n,m,a,b);let t=&p[o..o+16];
                if t==h.local_transaction {return Err(Error::Duplicate)}
                for j in 0..i {let q=map_tx_offset(j,n,m,a,b);if t==&p[q..q+16] {return Err(Error::Duplicate)}}}
        }
        55=>{let n=count(p,640)?;variable(p,644,40,n,l)?;scope(p,0)?;eq(&p[108..124],&h.adoption_id)?;
            let ph=u16at(p,124);let required=match ph {1|9=>0,2|5|6|7=>1,3|4|8=>2,_=>return Err(Error::Discriminant)};
            if n!=required {return Err(Error::Role)}zeros(&p[126..128])?;owner(p,128)?;owner(p,232)?;
            for (o,k) in [(336,53),(376,54),(416,36),(456,51)] {reference(p,o,&[k],false)?;}
            watermark(p,496,Some(h))?;for i in 0..n {reference(p,644+40*i,&[55],false)?;}}
        56=>{exact(p,720)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;
            let c=u16at(p,212);let s=u16at(p,214);if !(1..=3).contains(&c)||!(1..=3).contains(&s) {return Err(Error::Discriminant)}
            id(&p[216..232])?;if p[196..212]==p[216..232] {return Err(Error::Duplicate)}
            reference(p,232,&[48,58],false)?;reference(p,272,&[48,58],false)?;
            reference(p,312,&[match c {1=>49,2=>50,_=>57}],s==3)?;reference(p,352,&[48,58],s!=2)?;
            if (s==1&&!zero(&p[352..392]))||(s==2&&p[232..272]!=p[352..392])
                ||(s==3&&(!zero(&p[312..392])||p[232..272]!=p[272..312])) {return Err(Error::State)}
            watermark(p,392,Some(h))?;reference(p,536,&[51],false)?;binding(p,576)?;}
        57=>{exact(p,516)?;scope(p,0)?;debit(p,108)?;id(&p[196..212])?;id(&p[212..228])?;
            if p[196..212]==p[212..228] {return Err(Error::Duplicate)}
            for (o,k) in [(228,1),(268,48),(380,51),(420,36)] {reference(p,o,&[k],false)?;}
            vector(p,308)?;positive(u64at(p,460))?;unique_ids(p,&[468,484],h)?;zeros(&p[500..516])?;}
        _=>return Err(Error::Kind),
    } Ok(())
}
fn map_tx_offset(i:usize,n:usize,m:usize,a:usize,b:usize)->usize {
    if i<n {128+20*i+4}else if i<n+m {a+4+32*(i-n)+16}else{b+4+76*(i-n-m)+60}
}
fn transition_list(p:&[u8],start:usize,n:usize,v2:bool,h:&Header)->Result<(),Error> {
    let bootstrap=if v2 {[(0,1),(0,3),(0,4),(0,6),(0,7),(1,2),(1,5),(1,8)]}
        else {[(0,2),(0,4),(0,6),(0,7),(0,11),(1,3),(1,5),(1,8)]};
    if n!=1&&n!=8 {return Err(Error::Role)}
    for i in 0..n {let o=start+20*i;let key=(p[o],p[o+1]);
        if key!=if n==1 {(0,9)}else{bootstrap[i]} {return Err(Error::Role)}
        zeros(&p[o+2..o+4])?;id(&p[o+4..o+20])?;
        if p[o+4..o+20]==h.local_transaction {return Err(Error::Duplicate)}
        for j in 0..i {let a=start+20*j;if p[a+4..a+20]==p[o+4..o+20] {return Err(Error::Duplicate)}}
    } Ok(())
}

pub fn decode<'a>(bytes:&'a[u8],limits:&Limits)->Result<Record<'a>,Error> {
    limits.check()?;
    if bytes.len()<OVERHEAD||bytes.len()>limits.max_record_bytes||bytes.len()>limits.max_work {return Err(Error::Limits)}
    if &bytes[..4]!=b"DCO1" {return Err(Error::Magic)}
    if u16at(bytes,4)!=1 {return Err(Error::Version)}
    let kind=u16at(bytes,6);let schema=schema_id(kind).ok_or(Error::Kind)?;
    let n=u32at(bytes,208) as usize;
    if n.checked_add(OVERHEAD)!=Some(bytes.len())||u32at(bytes,8) as usize!=bytes.len() {return Err(Error::Length)}
    zeros(&bytes[12..16])?;zeros(&bytes[212..216])?;
    if bytes[16..48]!=schema {return Err(Error::Schema)}
    let h=read_header(bytes);
    header(kind,&h)?;
    // Bound quadratic identity comparisons before inspecting variable entries.
    let work=bytes.len().checked_add(payload_work(kind,&bytes[216..216+n])?).ok_or(Error::Overflow)?;
    if work>limits.max_work {return Err(Error::WorkExhausted)}
    payload(kind,&h,&bytes[216..216+n],limits)?;
    let mut hash=Sha256::new();hash.update(DOMAIN);hash.update(&bytes[..216+n]);
    let digest:[u8;32]=hash.finalize().into();
    if bytes[216+n..]!=digest {return Err(Error::Digest)}
    Ok(Record {kind,header:h,payload:&bytes[216..216+n],digest,bytes})
}
fn read_header(bytes:&[u8])->Header {
    Header {journal_ledger:bytes[48..80].try_into().unwrap(),journal_genesis:bytes[80..112].try_into().unwrap(),
        obligation_id:bytes[112..128].try_into().unwrap(),adoption_id:bytes[128..144].try_into().unwrap(),
        local_transaction:bytes[144..160].try_into().unwrap(),local_revision:u64at(bytes,160),journal_writer_epoch:u64at(bytes,168),
        previous_record_digest:bytes[176..208].try_into().unwrap()}
}
/// Only the graph's immutable, already independently decoded retained list uses this.
/// Keeping this crate-private prevents bypassing the public structural decoder.
pub(crate) fn retained_view(bytes:&[u8])->Record<'_> {
    Record{kind:u16at(bytes,6),header:read_header(bytes),payload:&bytes[216..bytes.len()-32],
        digest:bytes[bytes.len()-32..].try_into().unwrap(),bytes}
}
pub(crate) fn payload_work(kind:u16,p:&[u8])->Result<usize,Error> {
    let n=match kind {33|53=>count(p,460)?,35|54=>count(p,124)?,_=>0};
    // Kind54 has three independently bounded transaction lists.
    let n=if kind==54 {let a=128usize.checked_add(n.checked_mul(20).ok_or(Error::Overflow)?).ok_or(Error::Overflow)?;
        let m=count(p,a)?;let b=a.checked_add(4).and_then(|v|m.checked_mul(32).and_then(|x|v.checked_add(x))).ok_or(Error::Overflow)?;
        n.checked_add(m).and_then(|v|count(p,b).ok().and_then(|r|v.checked_add(r))).ok_or(Error::Length)?}else{n};
    n.checked_mul(n).ok_or(Error::Overflow)
}

/// Validates everything before touching `out`; errors leave the caller buffer unchanged.
pub fn encode(kind:u16,h:&Header,p:&[u8],out:&mut[u8],l:&Limits)->Result<usize,Error> {
    l.check()?;let n=p.len().checked_add(OVERHEAD).ok_or(Error::Overflow)?;
    if n>l.max_record_bytes||n>u32::MAX as usize {return Err(Error::Limits)}
    if n.checked_add(payload_work(kind,p)?).ok_or(Error::Overflow)?>l.max_work {return Err(Error::WorkExhausted)}
    let schema=schema_id(kind).ok_or(Error::Kind)?;header(kind,h)?;payload(kind,h,p,l)?;
    if out.len()<n {return Err(Error::BufferTooSmall)}
    let b=&mut out[..n];b[..4].copy_from_slice(b"DCO1");b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&kind.to_le_bytes());b[8..12].copy_from_slice(&(n as u32).to_le_bytes());
    b[12..16].fill(0);b[16..48].copy_from_slice(&schema);b[48..80].copy_from_slice(&h.journal_ledger);
    b[80..112].copy_from_slice(&h.journal_genesis);b[112..128].copy_from_slice(&h.obligation_id);
    b[128..144].copy_from_slice(&h.adoption_id);b[144..160].copy_from_slice(&h.local_transaction);
    b[160..168].copy_from_slice(&h.local_revision.to_le_bytes());b[168..176].copy_from_slice(&h.journal_writer_epoch.to_le_bytes());
    b[176..208].copy_from_slice(&h.previous_record_digest);b[208..212].copy_from_slice(&(p.len() as u32).to_le_bytes());
    b[212..216].fill(0);b[216..216+p.len()].copy_from_slice(p);
    let mut hash=Sha256::new();hash.update(DOMAIN);hash.update(&b[..n-32]);b[n-32..].copy_from_slice(&hash.finalize());Ok(n)
}

/// Borrowed shape-specific payload views. They describe bytes, not admitted facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadView<'a> {
    Reserve {scope:&'a[u8],debit_origin:&'a[u8],keys:&'a[u8],owner:&'a[u8],binding:&'a[u8],base:&'a[u8],pending:&'a[u8]},
    R1Transition {kind:u16,scope:&'a[u8],body:&'a[u8]},
    R1Release {scope:&'a[u8],debit_origin:&'a[u8],keys:&'a[u8],body:&'a[u8]},
    VectorManifest {profile:&'a[u8],entries:&'a[u8]},
    OwnershipManifest {successor:bool,scope:&'a[u8],source:&'a[u8],target:&'a[u8],source_watermark:&'a[u8],entries:&'a[u8]},
    RetainedFacts {obligation:&'a[u8],entries:&'a[u8]},
    TransitionMap {scope:&'a[u8],adoption:&'a[u8],entries:&'a[u8]},
    CapacityManifest {scope:&'a[u8],custodian:&'a[u8],control_reservation:&'a[u8],entries:&'a[u8]},
    AllocationSnapshot {closed:bool,scope:&'a[u8],debit_origin:&'a[u8],create_key:&'a[u8],custodian:&'a[u8],allocation_revision:u64,custody_revision:u64,transition:u16,state:u16,base:&'a[u8],pending:&'a[u8],effective:&'a[u8]},
    ResizeRequest {scope:&'a[u8],debit_origin:&'a[u8],create_key:&'a[u8],resize_key:&'a[u8],vectors:&'a[u8],transactions:&'a[u8]},
    AdjustmentRequest {scope:&'a[u8],debit_origin:&'a[u8],create_key:&'a[u8],adjustment_key:&'a[u8],mode:u16,vectors:&'a[u8]},
    EvidenceSet {scope:&'a[u8],subject_obligation:&'a[u8],subject:&'a[u8],entries:&'a[u8]},
    KeyRegistry {obligation:&'a[u8],original_reserve:&'a[u8],create_key:&'a[u8],entries:&'a[u8]},
    AdoptionMap {scope:&'a[u8],adoption:&'a[u8],transitions:&'a[u8],materializations:&'a[u8],continuations:&'a[u8]},
    AdoptionTransition {scope:&'a[u8],adoption:&'a[u8],phase:u16,source:&'a[u8],target:&'a[u8],dependencies:&'a[u8]},
    LookupResult {scope:&'a[u8],debit_origin:&'a[u8],create_key:&'a[u8],operation_class:u16,status:u16,operation_key:&'a[u8],selected:&'a[u8],current:&'a[u8],request:&'a[u8],result:&'a[u8]},
    ReleaseRequest {scope:&'a[u8],debit_origin:&'a[u8],create_key:&'a[u8],release_key:&'a[u8],credit:&'a[u8]},
}
impl<'a> Record<'a> {
    /// Rechecks local shape under explicit limits before exposing offset fields.
    /// This supports publicly inspectable Record fields without trusting a caller
    /// constructed view; integrity/authentication still require decode/proof checks.
    pub fn payload_view(&self,limits:&Limits)->Result<PayloadView<'a>,Error> {
        limits.check()?;header(self.kind,&self.header)?;
        if self.payload.len().checked_add(OVERHEAD).ok_or(Error::Overflow)?>limits.max_record_bytes {return Err(Error::Limits)}
        let work=self.payload.len().checked_add(payload_work(self.kind,self.payload)?).ok_or(Error::Overflow)?;
        if work>limits.max_work {return Err(Error::WorkExhausted)}
        payload(self.kind,&self.header,self.payload,limits)?;
        let p=self.payload;
        Ok(match self.kind {
            1=>PayloadView::Reserve{scope:&p[..108],debit_origin:&p[108..196],keys:&p[196..244],owner:&p[244..348],binding:&p[348..492],base:&p[492..564],pending:&p[564..636]},
            2..=9|11=>PayloadView::R1Transition{kind:self.kind,scope:&p[..108],body:&p[108..]},
            10=>PayloadView::R1Release{scope:&p[..108],debit_origin:&p[108..196],keys:&p[196..244],body:&p[244..]},
            32=>PayloadView::VectorManifest{profile:&p[..32],entries:&p[36..]},
            33|53=>PayloadView::OwnershipManifest{successor:self.kind==53,scope:&p[..108],source:&p[108..212],target:&p[212..316],source_watermark:&p[316..460],entries:&p[464..]},
            34=>PayloadView::RetainedFacts{obligation:&p[..16],entries:&p[20..]},
            35=>PayloadView::TransitionMap{scope:&p[..108],adoption:&p[108..124],entries:&p[128..]},
            36=>PayloadView::CapacityManifest{scope:&p[..108],custodian:&p[108..212],control_reservation:&p[212..252],entries:&p[256..]},
            48|58=>PayloadView::AllocationSnapshot{closed:self.kind==58,scope:&p[..108],debit_origin:&p[108..196],create_key:&p[196..212],custodian:&p[252..356],allocation_revision:u64at(p,436),custody_revision:u64at(p,444),transition:u16at(p,452),state:u16at(p,454),base:&p[460..532],pending:&p[532..604],effective:&p[604..676]},
            49=>PayloadView::ResizeRequest{scope:&p[..108],debit_origin:&p[108..196],create_key:&p[196..212],resize_key:&p[212..228],vectors:&p[556..1060],transactions:&p[1148..1212]},
            50=>PayloadView::AdjustmentRequest{scope:&p[..108],debit_origin:&p[108..196],create_key:&p[196..212],adjustment_key:&p[212..228],mode:u16at(p,228),vectors:&p[352..640]},
            51=>PayloadView::EvidenceSet{scope:&p[..108],subject_obligation:&p[108..124],subject:&p[124..164],entries:&p[168..]},
            52=>PayloadView::KeyRegistry{obligation:&p[..16],original_reserve:&p[16..56],create_key:&p[56..72],entries:&p[76..]},
            54=>{let a=128+20*u32at(p,124) as usize;let b=a+4+32*u32at(p,a) as usize;
                PayloadView::AdoptionMap{scope:&p[..108],adoption:&p[108..124],transitions:&p[128..a],materializations:&p[a+4..b],continuations:&p[b+4..]}}
            55=>PayloadView::AdoptionTransition{scope:&p[..108],adoption:&p[108..124],phase:u16at(p,124),source:&p[128..232],target:&p[232..336],dependencies:&p[644..]},
            56=>PayloadView::LookupResult{scope:&p[..108],debit_origin:&p[108..196],create_key:&p[196..212],operation_class:u16at(p,212),status:u16at(p,214),operation_key:&p[216..232],selected:&p[232..272],current:&p[272..312],request:&p[312..352],result:&p[352..392]},
            57=>PayloadView::ReleaseRequest{scope:&p[..108],debit_origin:&p[108..196],create_key:&p[196..212],release_key:&p[212..228],credit:&p[308..380]},
            _=>return Err(Error::Kind),
        })
    }
}
