//! Bounded retained-byte dependency validation. No resolver manufactures a missing fact.
use crate::{Error, Limits};
use crate::record::{self, Record, u16at, u32at, u64at, zero};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionRequirement {
    ExternalProofSchema { kind: u16 },
    AuthenticatedCurrentState,
    AdmittedProfile,
    CompleteOwnerSet,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref<'a> { bytes: &'a [u8] }
impl<'a> Ref<'a> {
    pub fn parse(bytes:&'a[u8])->Result<Self,Error> {
        if bytes.len()!=40 {return Err(Error::Length)}
        if u32at(bytes,0)<248||u16at(bytes,4)==0 {return Err(Error::Length)}
        if !zero(&bytes[6..8]) {return Err(Error::Reserved)}
        if zero(&bytes[8..40]) {return Err(Error::Identity)}
        Ok(Self{bytes})
    }
    pub fn encoded_length(self)->u32 {u32at(self.bytes,0)}
    pub fn kind(self)->u16 {u16at(self.bytes,4)}
    pub fn digest(self)->&'a[u8] {&self.bytes[8..40]}
    pub fn as_bytes(self)->&'a[u8] {self.bytes}
    pub fn matches(self,record:&Record<'_>)->bool {
        self.encoded_length() as usize==record.bytes.len()&&self.kind()==record.kind&&self.digest()==record.digest
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimension {pub dimension:u16,pub unit:u16,pub revision:u64}
#[derive(Clone, Copy, Debug)]
pub struct Profile<'a> {pub schema:[u8;32],pub dimensions:&'a[Dimension]}
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphScratch { mark:u8, next:usize }
/// Only byte consistency. This type intentionally cannot be converted into live permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructuralGraph {pub reachable_records:usize,pub charged_work:usize}
impl StructuralGraph {
    pub fn admission_requirement(self)->AdmissionRequirement {AdmissionRequirement::AuthenticatedCurrentState}
}
struct Graph<'a,'b> {retained:&'b[&'a[u8]],limits:&'b Limits,work:usize,profiles:&'b[Profile<'b>]}
impl<'a,'b> Graph<'a,'b> {
    fn charge(&mut self,n:usize)->Result<(),Error> {
        self.work=self.work.checked_add(n).ok_or(Error::Overflow)?;
        if self.work>self.limits.max_work {Err(Error::WorkExhausted)}else{Ok(())}
    }
    fn at(&mut self,i:usize)->Result<Record<'a>,Error> {
        let bytes=*self.retained.get(i).ok_or(Error::MissingReference)?;
        self.charge(1)?;
        Ok(record::retained_view(bytes))
    }
    fn find(&mut self,r:Ref<'_>)->Result<usize,Error> {
        if record::kind_name(r.kind()).is_none() {
            return Err(Error::AdmissionRequired(AdmissionRequirement::ExternalProofSchema{kind:r.kind()}))
        }
        let mut found=None;
        for i in 0..self.retained.len() {
            self.charge(1)?;let b=self.retained[i];
            if b.len()>=32&&&b[b.len()-32..]==r.digest() {
                if b.len()!=r.encoded_length() as usize||u16at(b,6)!=r.kind() {return Err(Error::Binding)}
                if found.is_some() {return Err(Error::Duplicate)}found=Some(i);
            }
        } found.ok_or(Error::MissingReference)
    }
    fn get(&mut self,p:&[u8],o:usize)->Result<Record<'a>,Error> {let i=self.find(Ref::parse(&p[o..o+40])?)?;self.at(i)}
    fn profile(&mut self,r:&Record<'_>)->Result<(),Error> {
        let p=r.payload;
        let profile=self.profiles.iter().find(|x|x.schema==p[..32]).ok_or(Error::AdmissionRequired(AdmissionRequirement::AdmittedProfile))?;
        let n=u32at(p,32) as usize;
        if profile.dimensions.len()!=n {return Err(Error::Profile)}
        for (i,d) in profile.dimensions.iter().enumerate() {
            self.charge(1)?;let o=36+24*i;
            if d.dimension!=u16at(p,o)||d.unit!=u16at(p,o+2)||d.revision!=u64at(p,o+16) {return Err(Error::Profile)}
        } Ok(())
    }
    fn vector(&mut self,p:&[u8],o:usize)->Result<Record<'a>,Error> {
        let r=self.get(p,o+32)?;same(&p[o..o+32],&r.payload[..32]).map_err(|_|Error::Profile)?;Ok(r)
    }
    fn same_obligation(&self,a:&Record<'_>,b:&Record<'_>)->Result<(),Error> {
        same(&a.header.obligation_id,&b.header.obligation_id)?;
        same(&a.header.journal_ledger,&b.header.journal_ledger)?;
        same(&a.header.journal_genesis,&b.header.journal_genesis)
    }
    fn reserve_binding(&mut self,r:&Record<'_>,o:usize)->Result<Record<'a>,Error> {
        let reserve=self.get(r.payload,o)?;self.same_obligation(r,&reserve)?;
        same(&r.payload[..212],&reserve.payload[..212])?;Ok(reserve)
    }
    fn vectors_equal(&mut self,a:&[u8],ao:usize,b:&[u8],bo:usize)->Result<(),Error> {
        let x=self.vector(a,ao)?;let y=self.vector(b,bo)?;same(x.payload,y.payload)
    }
    fn evidence(&mut self,p:&[u8],o:usize,subject:&Record<'_>,roles:&[u16])->Result<(),Error> {
        let e=self.get(p,o)?;
        if e.kind!=51 {return Err(Error::Role)}
        if !Ref::parse(&e.payload[124..164])?.matches(subject) {return Err(Error::Binding)}
        same(&e.payload[..108],&subject.payload[..108])?;
        same(&e.header.obligation_id,&subject.header.obligation_id)?;
        for role in roles {let mut found=false;for i in 0..u32at(e.payload,164) as usize {
            self.charge(1)?;if u16at(e.payload,168+48*i)==*role {found=true;}}
            if !found {return Err(Error::Role)}
        } Ok(())
    }
    fn bindings(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;
        match r.kind {
            32=>self.profile(r)?,
            1=>{self.vector(p,492)?;self.vector(p,564)?;
                let facts=self.get(p,676)?;self.same_obligation(r,&facts)?;
                let capacity=self.get(p,636)?;same(&p[..108],&capacity.payload[..108])?;
                // Resize-bearing original r1 reservations have no selected r1 decomposition.
                if !zero(&p[212..228]) {return Err(Error::AdmissionRequired(AdmissionRequirement::AuthenticatedCurrentState))}}
            2..=9|11=>self.r1_transition(r)?,
            10=>{let original=self.get(p,244)?;self.same_obligation(r,&original)?;
                same(&p[..212],&original.payload[..212])?;self.vectors_equal(p,428,original.payload,492)?;
                if !zero(&original.payload[212..244]) {return Err(Error::Binding)}}
            33|53=>self.manifest(r)?,
            34=>{same(&p[..16],&r.header.obligation_id)?;}
            35=>{let manifest=self.manifest_for(r,33)?;same(&p[..108],&manifest.payload[..108])?;}
            36=>{let control=self.get(p,212)?;
                if control.header.journal_ledger==r.header.journal_ledger&&control.header.journal_genesis==r.header.journal_genesis
                    &&control.header.local_revision>=r.header.local_revision {return Err(Error::Binding)}
                same(&p[..108],&control.payload[..108])?;}
            48|58=>self.snapshot(r)?,
            49=>self.resize(r)?,
            50=>self.adjustment(r)?,
            51=>{let subject=self.get(p,124)?;same(&p[..108],&subject.payload[..108])?;
                same(&r.header.obligation_id,&subject.header.obligation_id)?;}
            52=>self.registry(r)?,
            54=>{let manifest=self.manifest_for(r,53)?;same(&p[..108],&manifest.payload[..108])?;
                self.adoption_map(r,&manifest)?;}
            55=>self.adoption(r)?,
            56=>self.lookup(r)?,
            57=>{self.reserve_binding(r,228)?;let input=self.get(p,268)?;self.same_obligation(r,&input)?;
                same(&p[..212],&input.payload[..212])?;
                if u16at(input.payload,454)!=1||!zero(&input.payload[676..756])||u64at(p,460)!=u64at(input.payload,436) {return Err(Error::State)}
                self.vectors_equal(p,308,input.payload,460)?;self.evidence(p,380,&input,&[8])?;
                self.fresh_key(r,&input)?;}
            _=>return Err(Error::Kind),
        } Ok(())
    }
    fn manifest(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;let v2=r.kind==53;let stride=if v2 {440}else{280};
        for i in 0..u32at(p,460) as usize {self.charge(1)?;let o=464+stride*i;
            let reserve=self.get(p,o+104)?;
            same(&p[o..o+16],&reserve.header.obligation_id)?;same(&p[o+16..o+104],&reserve.payload[108..196])?;
            same(&p[..108],&reserve.payload[..108])?;
            let facts=self.get(p,o+if v2 {344}else{224})?;
            same(&facts.header.obligation_id,&reserve.header.obligation_id)?;same(&facts.payload[..16],&reserve.header.obligation_id)?;
            same(&facts.header.journal_ledger,&reserve.header.journal_ledger)?;
            same(&facts.header.journal_genesis,&reserve.header.journal_genesis)?;
            if v2 {let current=self.get(p,o+144)?;self.same_obligation(&reserve,&current)?;
                same(&current.payload[..212],&reserve.payload[..212])?;
                for (e,s) in [(184,492),(224,564),(264,636),(304,836),(384,876)] {same(&p[o+e..o+e+40],&current.payload[s..s+40])?;}
                if u64at(p,o+424)!=u64at(current.payload,436)||u64at(p,o+432)!=u64at(current.payload,444) {return Err(Error::Counter)}
                same(&p[108..212],&current.payload[252..356])?;
                if current.kind==58&&u32at(facts.payload,16)==0 {return Err(Error::Binding)}
            }else{same(&p[o+144..o+184],&reserve.payload[524..564])?;
                same(&p[o+184..o+224],&reserve.payload[596..636])?;
                if !zero(&reserve.payload[212..244]) {return Err(Error::Binding)}}
        } Ok(())
    }
    fn r1_transition(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;let mo=match r.kind {2|9=>316,3|5|7=>148,4|8|11=>188,6=>188,_=>return Err(Error::Kind)};
        let m=self.get(p,mo)?;same(&r.header.adoption_id,&m.header.adoption_id)?;same(&p[..108],&m.payload[..108])?;
        let source=if r.kind==9 {*r}else{self.r1_source(r)?};
        let map=self.get(source.payload,356)?;
        same(&r.header.adoption_id,&map.header.adoption_id)?;same(&p[..108],&map.payload[..108])?;
        let side=if matches!(r.kind,3|5|8) {1}else{0};self.map_transaction(r,&map,side,r.kind as u8)?;
        if side==0 {
            same(&r.header.journal_ledger,&source.header.journal_ledger)?;
            same(&r.header.journal_genesis,&source.header.journal_genesis)?;
        }else if r.kind!=3 {
            let destination=self.r1_destination(r)?;
            same(&r.header.journal_ledger,&destination.header.journal_ledger)?;
            same(&r.header.journal_genesis,&destination.header.journal_genesis)?;
        }
        same(&m.payload[316..348],&source.header.journal_ledger)?;
        same(&m.payload[348..380],&source.header.journal_genesis)?;
        if u64at(m.payload,380)>=source.header.local_revision {return Err(Error::Watermark)}
        match r.kind {
            2|9=>same(&p[108..316],&m.payload[108..316])?,
            3=>{let source=self.get(p,108)?;self.r1_common(r,&source,&m)?;
                same(&source.payload[316..356],&p[148..188])?;}
            4=>{same(&p[228..332],&m.payload[212..316])?;let started=self.get(p,372)?;
                same(&p[108..228],&started.payload[108..228])?;same(&p[228..332],&started.payload[268..372])?;}
            5=>same(&p[188..292],&m.payload[212..316])?,
            11=>same(&p[268..372],&m.payload[212..316])?,
            _=>{}
        }
        // Every r1 predecessor uses immutable A/scope/manifest and the proper journal side.
        let deps:&[usize]=match r.kind {2|9=>&[],3=>&[108],4=>&[108,148,372],5=>&[108],6=>&[108,148],7=>&[108],8=>&[108,148],11=>&[108,148],_=>&[]};
        for o in deps {let d=self.get(p,*o)?;self.r1_common(r,&d,&m)?;}
        Ok(())
    }
    fn manifest_for(&mut self,r:&Record<'_>,kind:u16)->Result<Record<'a>,Error> {
        let mut found=None;
        for i in 0..self.retained.len() {let candidate=self.at(i)?;
            if candidate.kind==kind&&candidate.header.adoption_id==r.header.adoption_id {
                if found.is_some() {return Err(Error::Duplicate)}found=Some(candidate);
            }
        } found.ok_or(Error::MissingReference)
    }
    fn r1_source(&mut self,r:&Record<'a>)->Result<Record<'a>,Error> {
        let mut at=*r;
        for _ in 0..self.limits.max_reference_depth {
            self.charge(1)?;
            if at.kind==2 {return Ok(at)}
            // All bootstrap branches have their source ancestor first.
            at=self.get(at.payload,108)?;
        } Err(Error::Limits)
    }
    fn r1_destination(&mut self,r:&Record<'a>)->Result<Record<'a>,Error> {
        let assignment=if r.kind==5 {self.get(r.payload,108)?}else{*r};
        if r.kind==8 {return self.get(r.payload,148)}
        self.get(assignment.payload,148)
    }
    fn r1_common(&self,r:&Record<'_>,d:&Record<'_>,m:&Record<'_>)->Result<(),Error> {
        same(&r.header.adoption_id,&d.header.adoption_id)?;same(&r.payload[..108],&d.payload[..108])?;
        let mo=match d.kind {2|9=>316,3|5|7=>148,_=>188};
        if !Ref::parse(&d.payload[mo..mo+40])?.matches(m) {return Err(Error::Binding)} Ok(())
    }
    fn map_transaction(&mut self,r:&Record<'_>,map:&Record<'_>,side:u8,phase:u8)->Result<(),Error> {
        let n=u32at(map.payload,124) as usize;
        for i in 0..n {self.charge(1)?;let o=128+20*i;if map.payload[o]==side&&map.payload[o+1]==phase {
            return same(&map.payload[o+4..o+20],&r.header.local_transaction)
        }}Err(Error::Role)
    }
    fn registry(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;let reserve=self.get(p,16)?;self.same_obligation(r,&reserve)?;same(&p[56..72],&reserve.payload[196..212])?;
        let n=u32at(p,72) as usize;
        for i in 0..n {self.charge(1)?;let o=76+64*i;let q=self.get(p,o+20)?;self.same_obligation(r,&q)?;
            same(&q.payload[..212],&reserve.payload[..212])?;same(&p[o+4..o+20],&q.payload[212..228])?;
            let ro=if q.kind==50 {232}else{228};if !Ref::parse(&q.payload[ro..ro+40])?.matches(&reserve) {return Err(Error::Binding)}
            if q.kind==50 {let resize=self.get(q.payload,272)?;let mut found=false;
                for j in 0..n {self.charge(1)?;let a=76+64*j;if u16at(p,a)==1&&Ref::parse(&p[a+20..a+60])?.matches(&resize) {found=true}}
                if !found {return Err(Error::Binding)}}
            // Operation keys and every mutation/helper transaction occupy disjoint namespaces.
            for j in 0..n {self.charge(1)?;let a=76+64*j;let other=self.get(p,a+20)?;
                if key_aliases_transaction(&p[o+4..o+20],&other) {return Err(Error::Duplicate)}}
        } Ok(())
    }
    fn fresh_key(&mut self,r:&Record<'_>,input:&Record<'_>)->Result<(),Error> {
        let registry=self.get(input.payload,836)?;
        for i in 0..u32at(registry.payload,72) as usize {self.charge(1)?;let o=76+64*i;
            if registry.payload[o+4..o+20]==r.payload[212..228] {return Err(Error::Duplicate)}}Ok(())
    }
    fn resize(&mut self,r:&Record<'a>)->Result<(),Error> {
        self.reserve_binding(r,228)?;let p=r.payload;let input=self.get(p,268)?;self.same_obligation(r,&input)?;
        same(&p[..212],&input.payload[..212])?;same(&p[308..412],&input.payload[252..356])?;
        if u16at(input.payload,454)!=1||!zero(&input.payload[676..756])||u64at(p,1140)!=u64at(input.payload,436) {return Err(Error::State)}
        for (a,b) in [(556,460),(628,532),(700,604)] {self.vectors_equal(p,a,input.payload,b)?;}
        let b=self.vector(p,556)?;let pp=self.vector(p,628)?;let e=self.vector(p,700)?;
        let q=self.vector(p,772)?;let g=self.vector(p,844)?;let l=self.vector(p,916)?;let m=self.vector(p,988)?;
        aligned(&[b,pp,e,q,g,l,m])?;
        for i in 0..u32at(b.payload,32) as usize {self.charge(1)?;let o=36+24*i+8;
            let bv=u64at(b.payload,o);let ev=u64at(e.payload,o);let qv=u64at(q.payload,o);
            if u64at(pp.payload,o)!=0||bv!=ev||qv>u64at(m.payload,o)||ev>u64at(m.payload,o)
                ||u64at(g.payload,o)!=if qv>bv {qv.checked_sub(bv).ok_or(Error::Arithmetic)?}else{0}
                ||u64at(l.payload,o)!=if qv>ev {qv.checked_sub(ev).ok_or(Error::Arithmetic)?}else{0} {return Err(Error::Arithmetic)}
            bv.checked_add(u64at(g.payload,o)).ok_or(Error::Overflow)?;
        }
        self.evidence(p,1060,&input,&[2,3])?;self.fresh_key(r,&input)
    }
    fn adjustment(&mut self,r:&Record<'a>)->Result<(),Error> {
        self.reserve_binding(r,232)?;let p=r.payload;let request=self.get(p,272)?;let input=self.get(p,312)?;
        self.same_obligation(r,&request)?;self.same_obligation(r,&input)?;same(&p[..212],&input.payload[..212])?;
        if !Ref::parse(&input.payload[676..716])?.matches(&request)||!zero(&input.payload[716..756])
            ||u64at(p,720)!=u64at(input.payload,436) {return Err(Error::State)}
        let mode=u16at(p,228);let state=u16at(input.payload,454);
        if (mode==1&&state!=4)||(mode==2&&state!=2&&state!=3) {return Err(Error::State)}
        let b=self.vector(input.payload,460)?;let pending=self.vector(input.payload,532)?;let e=self.vector(input.payload,604)?;
        let q=self.vector(request.payload,772)?;let s=self.vector(p,352)?;
        let rb=self.vector(p,424)?;let rp=self.vector(p,496)?;let re=self.vector(p,568)?;
        aligned(&[b,pending,e,q,s,rb,rp,re])?;let mut nonzero=false;
        for i in 0..u32at(b.payload,32) as usize {self.charge(1)?;let o=44+24*i;
            let bv=u64at(b.payload,o);let pv=u64at(pending.payload,o);let ev=u64at(e.payload,o);
            let qv=u64at(q.payload,o);let sv=u64at(s.payload,o);nonzero|=sv!=0;
            if u64at(rp.payload,o)!=0||u64at(re.payload,o)!=ev {return Err(Error::Arithmetic)}
            if mode==1 {if pv!=0||ev!=qv||sv>bv.checked_sub(qv).ok_or(Error::Arithmetic)?||u64at(rb.payload,o)!=bv.checked_sub(sv).ok_or(Error::Arithmetic)? {return Err(Error::Arithmetic)}}
            else if sv!=pv||u64at(rb.payload,o)!=bv {return Err(Error::Arithmetic)}
        }
        if mode==1&&!nonzero {return Err(Error::Arithmetic)}
        self.evidence(p,640,&request,&[if mode==1 {6}else{7}])?;self.fresh_key(r,&input)
    }
    fn snapshot(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;let original=self.reserve_binding(r,212)?;let t=u16at(p,452);let state=u16at(p,454);
        let b=self.vector(p,460)?;let pp=self.vector(p,532)?;let e=self.vector(p,604)?;aligned(&[b,pp,e])?;
        let registry=self.get(p,836)?;self.same_obligation(r,&registry)?;
        if !Ref::parse(&registry.payload[16..56])?.matches(&original)||registry.payload[56..72]!=p[196..212] {return Err(Error::Binding)}
        let op=self.get(p,756)?;
        if t!=8 {self.same_obligation(r,&op)?;same(&p[..212],&op.payload[..212])?;}
        for i in 0..u32at(b.payload,32) as usize {self.charge(1)?;let o=44+24*i;
            let bv=u64at(b.payload,o);let pv=u64at(pp.payload,o);let ev=u64at(e.payload,o);
            bv.checked_add(pv).ok_or(Error::Overflow)?;
            if bv<ev||((state==1||state==4||state==5||state==6)&&pv!=0)||(state==1&&bv!=ev)
                ||(state==6&&(bv!=0||pv!=0||ev!=0)) {return Err(Error::State)}
        }
        if t==1 {
            if !zero(&original.payload[212..244])||u32at(registry.payload,72)!=0||state!=1
                ||!Ref::parse(&p[756..796])?.matches(&original)||u16at(p,360)!=51 {return Err(Error::State)}
            self.vectors_equal(p,460,original.payload,492)?;self.vectors_equal(p,604,original.payload,492)?;
            self.vectors_equal(p,532,original.payload,564)?;
            if !vector_zero(&pp) {return Err(Error::State)}
            self.evidence(p,356,&original,&[1])?;self.evidence(p,796,&original,&[1,2,5,12])?;return Ok(())
        }
        let prev=self.get(p,396)?;self.same_obligation(r,&prev)?;same(&p[..252],&prev.payload[..252])?;
        if u64at(prev.payload,436).checked_add(1)!=Some(u64at(p,436)) {return Err(Error::Counter)}
        if t==8 {
            if !matches!(u16at(op.payload,124),5|9)||!Ref::parse(&p[356..396])?.matches(&op)
                ||u64at(prev.payload,444).checked_add(1)!=Some(u64at(p,444)) {return Err(Error::Binding)}
            same(&p[252..356],&op.payload[232..336])?;same(&p[454..756],&prev.payload[454..756])?;
            same(&p[836..876],&prev.payload[836..876])?;self.materialization(r,&op,&prev)?;
            self.evidence(p,796,&op,&[9,10,11])?;return Ok(())
        }
        same(&p[252..396],&prev.payload[252..396])?;same(&p[444..452],&prev.payload[444..452])?;
        let previous_state=u16at(prev.payload,454);let append=matches!(t,2|6|9);
        let old_registry=self.get(prev.payload,836)?;
        self.registry_transition(&registry,&old_registry,&op,append)?;
        let old_b=self.vector(prev.payload,460)?;let old_p=self.vector(prev.payload,532)?;let old_e=self.vector(prev.payload,604)?;
        aligned(&[b,pp,e,old_b,old_p,old_e])?;
        let expected_tx=match t {2=>1148,3=>1196,4=>1164,5=>1180,6=>728,7=>744,9=>468,10=>484,_=>return Err(Error::State)};
        if p[756..796]!=prev.payload[676..716]&&matches!(t,3|4|5) {return Err(Error::Binding)}
        if matches!(t,7|10)&&p[756..796]!=prev.payload[716..756] {return Err(Error::Binding)}
        self.operation_transaction(r,&op,expected_tx,&prev)?;
        match t {
            2=>{if previous_state!=1||state!=2||!zero(&prev.payload[676..756])||!zero(&p[716..756])
                    ||!Ref::parse(&p[676..716])?.matches(&op)||!Ref::parse(&op.payload[268..308])?.matches(&prev) {return Err(Error::State)}
                self.vectors_equal(p,532,op.payload,844)?;self.evidence(p,796,&prev,&[2,3])?;}
            3|4=>{if (t==3&&previous_state!=2&&previous_state!=3)||(t==4&&(previous_state!=2||state!=3))
                    ||(t==3&&state!=previous_state)||!zero(&prev.payload[716..756]) {return Err(Error::State)}
                same(&p[676..756],&prev.payload[676..756])?;
                self.evidence(p,796,&op,if t==3 {&[3,4]}else{&[3]})?;
                if t==4&&!self.has_ready(&prev,&op)? {return Err(Error::State)};}
            5=>{if !matches!(previous_state,2|3)||!zero(&prev.payload[716..756])||!zero(&p[716..756])||!self.has_ready(&prev,&op)? {return Err(Error::State)}
                let q=self.vector(op.payload,772)?;aligned(&[b,q])?;let mut settled=true;
                for i in 0..u32at(b.payload,32) as usize {self.charge(1)?;let o=44+24*i;
                    if u64at(b.payload,o)!=u64at(old_b.payload,o).checked_add(u64at(old_p.payload,o)).ok_or(Error::Overflow)?
                        ||u64at(pp.payload,o)!=0||u64at(e.payload,o)!=u64at(q.payload,o) {return Err(Error::Arithmetic)}
                    settled&=u64at(b.payload,o)==u64at(q.payload,o);}
                if state!=if settled {1}else{4}||zero(&p[676..716])!=settled {return Err(Error::State)}
                if !settled {same(&p[676..716],&prev.payload[676..716])?;}
                self.evidence(p,796,&op,&[5])?;}
            6=>{let mode=u16at(op.payload,228);if state!=previous_state||(mode==1&&previous_state!=4)
                    ||(mode==2&&!matches!(previous_state,2|3))||!zero(&prev.payload[716..756])
                    ||!Ref::parse(&p[716..756])?.matches(&op)||!Ref::parse(&op.payload[312..352])?.matches(&prev) {return Err(Error::State)}
                same(&p[676..716],&prev.payload[676..716])?;same(&p[796..836],&op.payload[640..680])?;
                let resize=self.get(op.payload,272)?;
                self.evidence(p,796,&resize,&[if mode==1 {6}else{7}])?;}
            7=>{if !zero(&p[716..756]) {return Err(Error::State)}
                for (a,o) in [(460,424),(532,496),(604,568)] {self.vectors_equal(p,a,op.payload,o)?;}
                let rz=self.get(prev.payload,676)?;let q=self.vector(rz.payload,772)?;aligned(&[b,q])?;
                let settled=u16at(op.payload,228)==2||vector_amount_equal(&b,&q);
                if state!=if settled {1}else{4}||zero(&p[676..716])!=settled {return Err(Error::State)}
                if !settled {same(&p[676..716],&prev.payload[676..716])?;}same(&p[796..836],&op.payload[640..680])?;}
            9=>{if previous_state!=1||state!=5||!zero(&p[676..716])||!Ref::parse(&p[716..756])?.matches(&op)
                    ||!Ref::parse(&op.payload[268..308])?.matches(&prev) {return Err(Error::State)}
                same(&p[796..836],&op.payload[380..420])?;}
            10=>{if previous_state!=5||state!=6 {return Err(Error::State)}same(&p[796..836],&op.payload[380..420])?;}
            _=>return Err(Error::State),
        }
        if matches!(t,2|3|4|6|9) {same(b.payload,old_b.payload)?;same(e.payload,old_e.payload)?;}
        if matches!(t,3|4|6|9) {same(pp.payload,old_p.payload)?;}
        Ok(())
    }
    fn registry_transition(&mut self,new:&Record<'_>,old:&Record<'_>,op:&Record<'_>,append:bool)->Result<(),Error> {
        if !append {return same(new.bytes,old.bytes)}
        let n=u32at(new.payload,72) as usize;let m=u32at(old.payload,72) as usize;
        if m.checked_add(1)!=Some(n) {return Err(Error::Binding)}
        let mut old_i=0;let mut added=false;
        for i in 0..n {self.charge(1)?;let o=76+64*i;
            if Ref::parse(&new.payload[o+20..o+60])?.matches(op) {if added {return Err(Error::Duplicate)}added=true;}
            else{if old_i>=m||new.payload[o..o+64]!=old.payload[76+64*old_i..140+64*old_i] {return Err(Error::Binding)}old_i+=1;}
        }if !added||old_i!=m {return Err(Error::Binding)}Ok(())
    }
    fn has_ready(&mut self,input:&Record<'a>,request:&Record<'_>)->Result<bool,Error> {
        let mut current=*input;
        for _ in 0..self.limits.max_reference_depth {
            self.charge(1)?;
            if !Ref::parse(&current.payload[676..716])?.matches(request) {return Ok(false)}
            if u16at(current.payload,452)==3 {return Ok(true)}
            if zero(&current.payload[396..436]) {return Ok(false)}current=self.get(current.payload,396)?;
        }Err(Error::Limits)
    }
    fn operation_transaction(&mut self,r:&Record<'_>,op:&Record<'_>,offset:usize,prev:&Record<'_>)->Result<(),Error> {
        let input_offset=if op.kind==50 {312}else{268};
        let operation_input=self.get(op.payload,input_offset)?;
        let same_custody=operation_input.payload[252..396]==prev.payload[252..396]
            &&operation_input.payload[444..452]==prev.payload[444..452];
        if same_custody&&r.header.local_transaction==op.payload[offset..offset+16] {
            if u16at(r.payload,452)==3&&u16at(prev.payload,454)==3 {return Err(Error::State)}
            return Ok(())
        }
        let decision=self.get(prev.payload,356)?;if decision.kind!=55 {return Err(Error::Binding)}
        let map=self.get(decision.payload,376)?;let p=map.payload;let n=u32at(p,124) as usize;
        let a=128+20*n;let m=u32at(p,a) as usize;let b=a+4+32*m;let count=u32at(p,b) as usize;
        for i in 0..count {self.charge(1)?;let o=b+4+76*i;
            if p[o..o+16]==r.header.obligation_id&&Ref::parse(&p[o+16..o+56])?.matches(op)
                &&u16at(p,o+56)==u16at(r.payload,452) {return same(&p[o+60..o+76],&r.header.local_transaction)}
        }Err(Error::Binding)
    }
    fn materialization(&mut self,r:&Record<'_>,decision:&Record<'_>,prev:&Record<'_>)->Result<(),Error> {
        let map=self.get(decision.payload,376)?;let manifest=self.get(decision.payload,336)?;
        let n=u32at(map.payload,124) as usize;let a=128+20*n;let m=u32at(map.payload,a) as usize;
        let mut found=false;
        for i in 0..m {self.charge(1)?;let o=a+4+32*i;if map.payload[o..o+16]==r.header.obligation_id {
            same(&map.payload[o+16..o+32],&r.header.local_transaction)?;found=true;}}
        if !found {return Err(Error::Binding)}
        for i in 0..u32at(manifest.payload,460) as usize {self.charge(1)?;let o=464+440*i;
            if manifest.payload[o..o+16]==r.header.obligation_id {
                if !Ref::parse(&manifest.payload[o+144..o+184])?.matches(prev) {return Err(Error::Binding)}return Ok(())}}
        Err(Error::Binding)
    }
    fn adoption(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;let phase=u16at(p,124);let manifest=self.get(p,336)?;let map=self.get(p,376)?;
        same(&p[..108],&manifest.payload[..108])?;same(&p[..124],&map.payload[..124])?;
        same(&p[128..336],&manifest.payload[108..316])?;
        same(&r.header.adoption_id,&manifest.header.adoption_id)?;same(&r.header.adoption_id,&map.header.adoption_id)?;
        let side=if matches!(phase,2|5|8) {1}else{0};self.map_transaction(r,&map,side,phase as u8)?;
        let expected:&[u16]=match phase {1|9=>&[],2=>&[1],3=>&[1,2],4=>&[3,2],5=>&[4],6=>&[5],7=>&[1],8=>&[7,2],_=>return Err(Error::Role)};
        for (i,ph) in expected.iter().enumerate() {let dep=self.get(p,644+40*i)?;
            if u16at(dep.payload,124)!=*ph {return Err(Error::Role)}
            same(&p[..124],&dep.payload[..124])?;same(&p[128..416],&dep.payload[128..416])?;
            let ds=if matches!(*ph,2|5|8) {1}else{0};
            if side==ds {same(&r.header.journal_ledger,&dep.header.journal_ledger)?;same(&r.header.journal_genesis,&dep.header.journal_genesis)?;}
        }
        // Canonical map materializations and continuations must be exactly the frozen set.
        self.adoption_map(&map,&manifest)?;
        let ev=self.get(p,456)?;let subject=self.get(ev.payload,124)?;
        if !Ref::parse(&p[336..376])?.matches(&subject)&&!expected.iter().enumerate().any(|(i,_)|Ref::parse(&p[644+40*i..684+40*i]).map(|x|x.matches(&subject)).unwrap_or(false)) {return Err(Error::Binding)}
        let roles:&[u16]=match phase {1|2=>&[9,10],4=>&[11],5|9=>&[9,10,11],_=>&[]};
        self.evidence(p,456,&subject,roles)?;Ok(())
    }
    fn adoption_map(&mut self,map:&Record<'_>,manifest:&Record<'_>)->Result<(),Error> {
        let p=map.payload;let n=u32at(p,124) as usize;let a=128+20*n;let m=u32at(p,a) as usize;
        let b=a+4+32*m;let r=u32at(p,b) as usize;let mut material=0;let mut expected_cont=0;
        for i in 0..u32at(manifest.payload,460) as usize {self.charge(1)?;let o=464+440*i;let snapshot=self.get(manifest.payload,o+144)?;
            let registry=self.get(snapshot.payload,836)?;
            for issued in 0..u32at(registry.payload,72) as usize {
                let request=self.get(registry.payload,96+64*issued)?;
                for tx in 0..n+m+r {
                    self.charge(1)?;
                    let offset=if tx<n {132+20*tx}else if tx<n+m {a+20+32*(tx-n)}else{b+64+76*(tx-n-m)};
                    if key_aliases_transaction(&p[offset..offset+16],&request) {return Err(Error::Duplicate)}
                }
            }
            if snapshot.kind==58 {continue}
            if material>=m||p[a+4+32*material..a+20+32*material]!=snapshot.header.obligation_id {return Err(Error::Binding)}material+=1;
            let s=u16at(snapshot.payload,454);
            let active=if !zero(&snapshot.payload[716..756]) {Some((self.get(snapshot.payload,716)?,if s==5 {10}else{7}))}
                else if matches!(s,2|3) {Some((self.get(snapshot.payload,676)?,3))}else{None};
            if let Some((request,tag))=active {
                let tags:&[u16]=if tag==3 {if s==2 {&[3,4,5]}else{&[3,5]}}else if tag==7 {&[7]}else{&[10]};
                for t in tags {let mut seen=false;
                    for j in 0..r {self.charge(1)?;let q=b+4+76*j;
                        if p[q..q+16]==snapshot.header.obligation_id&&Ref::parse(&p[q+16..q+56])?.matches(&request)&&u16at(p,q+56)==*t {seen=true;}}
                    if !seen {return Err(Error::Binding)}expected_cont+=1;
                }
                for j in 0..r {let q=b+4+76*j;if key_aliases_transaction(&p[q+60..q+76],&request) {return Err(Error::Duplicate)}}
            }
        }
        if material!=m||expected_cont!=r {return Err(Error::Binding)}Ok(())
    }
    fn lookup(&mut self,r:&Record<'a>)->Result<(),Error> {
        let p=r.payload;let current=self.get(p,272)?;let selected=self.get(p,232)?;
        self.same_obligation(r,&current)?;self.same_obligation(r,&selected)?;same(&p[..212],&current.payload[..212])?;
        same(&p[..212],&selected.payload[..212])?;self.evidence(p,536,&current,&[12])?;
        let registry=self.get(current.payload,836)?;let key=&p[216..232];let status=u16at(p,214);let class=u16at(p,212);
        let mut issued=None;
        for i in 0..u32at(registry.payload,72) as usize {self.charge(1)?;let o=76+64*i;
            let issued_request=self.get(registry.payload,o+20)?;
            if key_aliases_transaction(key,&issued_request) {return Err(Error::Duplicate)}
            if &registry.payload[o+4..o+20]==key {
                if u16at(registry.payload,o)!=class {return Err(Error::Binding)}issued=Some(issued_request);
            }
        }
        if status==3 {if issued.is_some() {return Err(Error::State)}return Ok(())}
        let request=issued.ok_or(Error::Binding)?;
        if !Ref::parse(&p[312..352])?.matches(&request) {return Err(Error::Binding)}
        if key_aliases_transaction(&r.header.local_transaction,&request) {return Err(Error::Duplicate)}
        if status==1 {
            if !snapshot_carries(&current,&request)||!snapshot_carries(&selected,&request) {return Err(Error::State)}
            let tag=u16at(selected.payload,452);if !matches!(tag,2|6|9) {return Err(Error::State)}
        }else{
            let tag=u16at(selected.payload,452);let valid=match class {
                1=>matches!(tag,5|7)&&u16at(selected.payload,454)==1&&zero(&selected.payload[676..756]),
                2=>tag==7&&Ref::parse(&selected.payload[756..796])?.matches(&request),
                3=>selected.kind==58&&Ref::parse(&selected.payload[756..796])?.matches(&request),_=>false};
            if !valid {return Err(Error::State)}
            if class==1 {let op=self.get(selected.payload,756)?;if op.kind==49 {if op.digest!=request.digest {return Err(Error::Binding)}}
                else if op.kind==50 {if !Ref::parse(&op.payload[272..312])?.matches(&request) {return Err(Error::Binding)}}else{return Err(Error::Role)}}
        }
        // Selected result must be on the retained current chain, never a caller-chosen fork.
        let mut at=current;
        for _ in 0..self.limits.max_reference_depth {self.charge(1)?;
            if at.digest==selected.digest {return Ok(())}
            if zero(&at.payload[396..436]) {return Err(Error::Binding)}at=self.get(at.payload,396)?;
        }Err(Error::Limits)
    }
}
fn same(a:&[u8],b:&[u8])->Result<(),Error> {if a==b {Ok(())}else{Err(Error::Binding)}}
fn aligned(records:&[Record<'_>])->Result<(),Error> {
    let first=records[0].payload;
    for r in &records[1..] {let p=r.payload;if first.len()!=p.len()||first[..36]!=p[..36] {return Err(Error::Profile)}
        for i in 0..u32at(first,32) as usize {let o=36+24*i;
            if first[o..o+8]!=p[o..o+8]||first[o+16..o+24]!=p[o+16..o+24] {return Err(Error::Profile)}}}
    Ok(())
}
fn vector_zero(r:&Record<'_>)->bool {(0..u32at(r.payload,32) as usize).all(|i|u64at(r.payload,44+24*i)==0)}
fn vector_amount_equal(a:&Record<'_>,b:&Record<'_>)->bool {a.payload==b.payload}
fn snapshot_carries(s:&Record<'_>,request:&Record<'_>)->bool {
    [676,716].iter().any(|o|!zero(&s.payload[*o..*o+40])&&Ref::parse(&s.payload[*o..*o+40]).map(|r|r.matches(request)).unwrap_or(false))
}
fn key_aliases_transaction(key:&[u8],r:&Record<'_>)->bool {
    if key==r.header.local_transaction {return true}
    let offsets:&[usize]=match r.kind {49=>&[1148,1164,1180,1196],50=>&[728,744],57=>&[468,484],_=>&[]};
    offsets.iter().any(|o|key==&r.payload[*o..*o+16])
}

/// Enumerates exact Ref slots; bool identifies an external proof role, not an
/// arbitrary permission-granting record. Every declared kind has a complete layout.
fn refs<'a>(r:&Record<'a>,mut f:impl FnMut(Ref<'a>,bool)->Result<(),Error>)->Result<(),Error> {
    let p=r.payload;
    let mut at=|o:usize,external:bool| {if zero(&p[o..o+40]) {Ok(())}else{f(Ref::parse(&p[o..o+40])?,external)}};
    match r.kind {
        1=>{for o in [524,596,636,676] {at(o,false)?;}at(452,true)?;}
        2=>{for o in [316,356,396] {at(o,false)?;}at(540,true)?;}
        3=>{for o in [108,148,292] {at(o,false)?;}at(436,true)?;}
        4=>{for o in [108,148,188,372] {at(o,false)?;}for o in [332,516] {at(o,true)?;}}
        5=>{for o in [108,148] {at(o,false)?;}for o in [292,332,476] {at(o,true)?;}}
        6|7|8=>{for o in [108,148,188] {at(o,false)?;}at(332,true)?;}
        9=>{for o in [316,356] {at(o,false)?;}for o in [396,436,580] {at(o,true)?;}}
        10=>{at(244,false)?;at(284,true)?;at(460,false)?;at(604,true)?;}
        11=>{for o in [108,148,188,228] {at(o,false)?;}at(476,true)?;}
        32=>{},
        33|53=>{at(420,true)?;let stride=if r.kind==33 {280}else{440};
            for i in 0..u32at(p,460) as usize {let o=464+stride*i;
                let offsets:&[usize]=if r.kind==33 {&[104,144,184,224]}else{&[104,144,184,224,264,304,344,384]};
                for a in offsets {at(o+a,false)?;}}}
        34|51=>{let (n,base)=if r.kind==34 {(u32at(p,16),20)}else{at(124,false)?;(u32at(p,164),168)};
            for i in 0..n as usize {at(base+48*i+4,r.kind==51)?;}}
        35=>{},36=>at(212,false)?,
        48|58=>{for o in [212,356,396,492,564,636,676,716,756,796,836,876] {at(o,false)?;}at(1020,true)?;}
        49=>{for o in [228,268,588,660,732,804,876,948,1020,1060,1100] {at(o,false)?;}at(516,true)?;}
        50=>{for o in [232,272,312,384,456,528,600,640,680] {at(o,false)?;}}
        52=>{at(16,false)?;for i in 0..u32at(p,72) as usize {at(96+64*i,false)?;}}
        54=>{let n=u32at(p,124) as usize;let a=128+20*n;let m=u32at(p,a) as usize;let b=a+4+32*m;
            for i in 0..u32at(p,b) as usize {at(b+4+76*i+16,false)?;}}
        55=>{for o in [336,376,416,456] {at(o,false)?;}at(600,true)?;for i in 0..u32at(p,640) as usize {at(644+40*i,false)?;}}
        56=>{for o in [232,272,312,352,536] {at(o,false)?;}at(496,true)?;at(680,true)?;}
        57=>{for o in [228,268,340,380,420] {at(o,false)?;}}
        _=>return Err(Error::Kind),
    }Ok(())
}

/// Parses each retained byte slice independently, rejects duplicate identities,
/// follows all dependency edges with caller-owned scratch, and checks structural
/// crossbindings. An unknown or not-yet-assigned proof schema refuses explicitly.
/// The list is exact retained bytes, not a digest oracle. It need not be topological.
pub fn validate_references<'a>(root:usize,retained:&[&'a[u8]],limits:&Limits,
    scratch:&mut[GraphScratch],stack:&mut[usize],profiles:&[Profile<'_>])->Result<StructuralGraph,Error> {
    limits.check()?;
    if retained.is_empty()||root>=retained.len()||retained.len()>limits.max_records {return Err(Error::Limits)}
    if scratch.len()<retained.len()||stack.len()<limits.max_reference_depth {return Err(Error::ScratchTooSmall)}
    let mut total=0usize;
    for b in retained {total=total.checked_add(b.len()).ok_or(Error::Overflow)?;}
    if total>limits.max_total_bytes {return Err(Error::Limits)}
    let mut g=Graph{retained,limits,work:0,profiles};
    let mut external_requirement=None;
    // Hash and validate each immutable retained slice once, before constructing
    // cheap borrowed views during traversal. Precharge quadratic payload work.
    for bytes in retained {
        let extra=if bytes.len()>=248 {record::payload_work(u16at(bytes,6),&bytes[216..bytes.len()-32])?}else{0};
        g.charge(bytes.len().checked_add(extra).ok_or(Error::Overflow)?)?;
        record::decode(bytes,limits)?;
    }
    for i in 0..retained.len() {let r=g.at(i)?;scratch[i]=GraphScratch::default();
        for j in 0..i {g.charge(1)?;let s=g.at(j)?;
            if r.digest==s.digest {return Err(Error::Duplicate)}
            if r.header.adoption_id==s.header.adoption_id&&!zero(&r.header.adoption_id) {
                if (matches!((r.kind,s.kind),(7,11)|(11,7)))
                    ||(r.kind==55&&s.kind==55&&matches!((u16at(r.payload,124),u16at(s.payload,124)),(3,7)|(7,3))) {
                    return Err(Error::State)
                }
            }
            if r.header.journal_ledger==s.header.journal_ledger&&r.header.journal_genesis==s.header.journal_genesis {
                if r.header.local_transaction==s.header.local_transaction||r.header.local_revision==s.header.local_revision {return Err(Error::Duplicate)}
                if r.kind==1&&s.kind==1&&r.header.obligation_id==s.header.obligation_id {return Err(Error::Duplicate)}
            }
        }
    }
    stack[0]=root;scratch[root].mark=1;let mut depth=1;let mut reachable=0;
    while depth!=0 {let i=stack[depth-1];let r=g.at(i)?;let wanted=scratch[i].next;let mut next=None;let mut count=0;
        if wanted==0 {g.bindings(&r)?;}
        refs(&r,|reference,external| {g.charge(1)?;if count==wanted {next=Some((reference,external));}count+=1;Ok(())})?;
        if let Some((reference,external))=next {
            scratch[i].next=scratch[i].next.checked_add(1).ok_or(Error::Overflow)?;
            if reference.digest()==r.digest {return Err(Error::Cycle)}
            if external {
                external_requirement=Some(AdmissionRequirement::ExternalProofSchema{kind:reference.kind()});
                continue
            }
            let j=g.find(reference)?;
            if dependency_needs_visit(scratch[j].mark)? {
                if depth>=limits.max_reference_depth {return Err(Error::Limits)}
                stack[depth]=j;depth+=1;scratch[j].mark=1;
            }
        }else{scratch[i].mark=2;reachable+=1;depth-=1;}
    }
    if let Some(requirement)=external_requirement {return Err(Error::AdmissionRequired(requirement))}
    Ok(StructuralGraph{reachable_records:reachable,charged_work:g.work})
}
fn dependency_needs_visit(mark:u8)->Result<bool,Error> {
    match mark {0=>Ok(true),1=>Err(Error::Cycle),_=>Ok(false)}
}

#[cfg(test)]
mod cycle_gate_regressions {
    use super::*;
    #[test]
    fn active_dependency_is_cycle_but_completed_shared_dependency_is_not() {
        // Exercise the actual graph color gate with synthetic dependency state;
        // no impossible SHA256 fixed-point bytes are claimed as a valid fixture.
        assert_eq!(dependency_needs_visit(1),Err(Error::Cycle));
        assert_eq!(dependency_needs_visit(2),Ok(false));
        assert_eq!(dependency_needs_visit(0),Ok(true));
    }
}
