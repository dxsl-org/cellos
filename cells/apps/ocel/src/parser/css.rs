// SPDX-License-Identifier: MIT
//! Alloc-only CSS subset. Unsupported selectors/values are rejected, not broadened.
//! This is not a CSS Syntax/Selectors conformance implementation.
use alloc::{collections::BTreeMap, string::String, vec::Vec};
use dom_arena::{DocumentArena, NodeId};
use taffy::prelude::*;
use crate::draw::Color;
/// Keep glyph rasterization bounded even for untrusted CSS sizes.
pub const MAX_FONT_SIZE: f32 = 128.0;

#[derive(Clone, Debug)]
pub struct ComputedStyle {
    pub layout: Style,
    pub inline: bool,
    pub color: Color,
    pub background: Option<Color>,
    pub border_color: Color,
    pub font_size: f32,
    pub line_height: f32,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    /// 0 = start, 1 = center, 2 = end.
    pub text_align: u8,
    pub pre: bool,
    pub wrap_pre: bool,
}
impl ComputedStyle {
    pub fn initial(parent: Option<&Self>, tag: &str) -> Self {
        let mut s = Self {
            layout: Style { display: Display::Block, box_sizing: BoxSizing::ContentBox, ..Style::default() },
            inline: matches!(tag, "a"|"span"|"b"|"strong"|"i"|"em"|"code"|"small"|"label"|"br"),
            color: Color::rgb(32,32,32), background: None, border_color: Color::rgb(32,32,32),
            font_size: 16.0, line_height: 20.0, bold: false, italic: false, mono: false, text_align: 0, pre: false, wrap_pre: false,
        };
        if let Some(p) = parent {
            s.color=p.color; s.font_size=p.font_size; s.line_height=p.line_height;
            s.bold=p.bold; s.italic=p.italic; s.pre=p.pre;
            s.wrap_pre=p.wrap_pre;
            s.mono=p.mono; s.text_align=p.text_align;
        }
        if matches!(tag,"head"|"style"|"script"|"link"|"meta"|"title"|"template") { s.layout.display=Display::None; }
        if tag=="body" { s.layout.margin=Rect { left:length(8.0),right:length(8.0),top:length(8.0),bottom:length(8.0) }; }
        if matches!(tag,"p"|"pre"|"h1"|"h2"|"h3"|"h4"|"h5"|"h6") { s.layout.margin.top=length(12.0); s.layout.margin.bottom=length(12.0); }
        if matches!(tag,"h1"|"h2") { s.font_size=32.0; s.line_height=38.0; s.bold=true; }
        if matches!(tag,"b"|"strong") { s.bold=true; }
        if matches!(tag,"i"|"em") { s.italic=true; }
        if tag=="pre" { s.pre=true; }
        if matches!(tag,"pre"|"code"|"kbd"|"samp") { s.mono=true; }
        if tag=="a" { s.color=Color::rgb(0,80,180); }
        if tag=="button" { s.background=Some(Color::rgb(230,230,230)); s.layout.padding=Rect {left:length(8.0),right:length(8.0),top:length(4.0),bottom:length(4.0)}; }
        s
    }
}

#[derive(Clone)]
struct Declaration { name:String, value:String, important:bool }
#[derive(Clone)]
struct Rule { selectors:Vec<String>, declarations:Vec<Declaration> }
#[derive(Default)]
pub struct Stylesheet { rules:Vec<Rule> }

/// Split only at top level, preserving quoted strings and functions.
fn split_top(input:&str, delimiter:char) -> Vec<&str> {
    let mut out=Vec::new(); let mut start=0; let mut depth=0usize; let mut quote=None; let mut escape=false;
    for (i,c) in input.char_indices() {
        if escape { escape=false; continue; }
        if c=='\\' { escape=true; continue; }
        if let Some(q)=quote { if c==q {quote=None;} continue; }
        if c=='\''||c=='"' {quote=Some(c);continue;}
        if c=='('||c=='[' {depth+=1;} else if c==')'||c==']' {depth=depth.saturating_sub(1);}
        else if c==delimiter && depth==0 {out.push(input[start..i].trim());start=i+c.len_utf8();}
    }
    out.push(input[start..].trim());out
}
fn declarations(input:&str)->Vec<Declaration> {
    let mut out=Vec::new();
    for part in split_top(input,';') {
        let Some((name,value))=part.split_once(':') else {continue;};
        let name=name.trim().to_ascii_lowercase(); let mut value=value.trim(); let mut important=false;
        if let Some(i)=value.rfind('!') { if value[i+1..].trim().eq_ignore_ascii_case("important") { important=true;value=value[..i].trim(); } }
        // Expand spacing shorthands before cascading, so shorthand/longhand compete correctly.
        if matches!(name.as_str(),"margin"|"padding"|"border-width") {
            let words:Vec<_>=value.split_whitespace().collect(); if words.is_empty()||words.len()>4 {continue;}
            let sides=[words[0],*words.get(1).unwrap_or(&words[0]),*words.get(2).unwrap_or(&words[0]),*words.get(3).unwrap_or(words.get(1).unwrap_or(&words[0]))];
            for (side,val) in ["top","right","bottom","left"].into_iter().zip(sides) {
                let key=if name=="border-width" {alloc::format!("border-{}-width",side)} else {alloc::format!("{}-{}",name,side)};
                out.push(Declaration{name:key,value:String::from(val),important});
            }
        } else if name=="background" {
            if color(value).is_some() {out.push(Declaration{name:String::from("background-color"),value:String::from(value),important});}
        } else if name=="border" {
            let words:Vec<_>=value.split_whitespace().collect();
            if words.len()==3 && words[1]=="solid" && color(words[2]).is_some() {
                for side in ["left","right","top","bottom"] {out.push(Declaration{name:alloc::format!("border-{}-width",side),value:String::from(words[0]),important});}
                out.push(Declaration{name:String::from("border-color"),value:String::from(words[2]),important});
            }
        } else if name=="gap" {
            let p:Vec<_>=value.split_whitespace().collect();
            if (1..=2).contains(&p.len()) {for (key,v) in [("row-gap",p[0]),("column-gap",*p.get(1).unwrap_or(&p[0]))] {out.push(Declaration{name:String::from(key),value:String::from(v),important});}}
        } else if name=="flex" {
            let p:Vec<_>=value.split_whitespace().collect();
            let expanded=match value {
                "none"=>Some(("0","0","auto")), "auto"=>Some(("1","1","auto")),
                _ if (1..=3).contains(&p.len()) && number(p[0]).is_some()=>Some((p[0],*p.get(1).unwrap_or(&"1"),*p.get(2).unwrap_or(&"0%"))),
                _=>None,
            };
            if let Some((grow,shrink,basis))=expanded {for (key,v) in [("flex-grow",grow),("flex-shrink",shrink),("flex-basis",basis)] {out.push(Declaration{name:String::from(key),value:String::from(v),important});}}
        } else {out.push(Declaration{name,value:String::from(value),important});}
    }
    out
}
fn strip_comments(input:&str)->String {
    let mut out=String::new(); let mut it=input.chars().peekable();let mut quote=None;let mut escaped=false;
    while let Some(c)=it.next() {
        if escaped {out.push(c);escaped=false;continue;}
        if c=='\\' {out.push(c);escaped=true;continue;}
        if let Some(q)=quote {if c==q {quote=None;}out.push(c);continue;}
        if c=='\''||c=='"' {quote=Some(c);out.push(c);continue;}
        if c=='/' && it.peek()==Some(&'*') {it.next();while let Some(c)=it.next(){if c=='*'&&it.peek()==Some(&'/'){it.next();break;}}out.push(' ');} else {out.push(c);}
    } out
}
impl Stylesheet {
    pub fn append(&mut self, css:&str, viewport:(u32,u32)) { self.parse_rules(&strip_comments(css),viewport); }
    fn parse_rules(&mut self,css:&str,viewport:(u32,u32)) {
        let mut start=0;let mut opening=None;let mut depth=0usize;let mut quote=None;let mut escaped=false;
        for (i,c) in css.char_indices() {
            if escaped {escaped=false;continue;} if c=='\\' {escaped=true;continue;}
            if let Some(q)=quote {if c==q {quote=None;}continue;} if c=='\''||c=='"' {quote=Some(c);continue;}
            if c=='{' {if depth==0 {opening=Some(i);}depth+=1;}
            else if c=='}' && depth>0 {depth-=1;if depth==0 {let open=opening.take().unwrap();let header=css[start..open].trim();let body=&css[open+1..i];
                if let Some(query)=header.strip_prefix("@media") {if media_matches(query,viewport) {self.parse_rules(body,viewport);}}
                else if !header.starts_with('@') {self.rules.push(Rule {selectors:split_top(header,',').into_iter().map(String::from).collect(),declarations:declarations(body)});}
                start=i+1;
            }} else if c==';'&&depth==0 {start=i+1;}
        }
    }
    pub fn compute(&self,arena:&DocumentArena,id:NodeId,parent:Option<&ComputedStyle>,viewport:(u32,u32))->ComputedStyle {
        let Some(node)=arena.get(id) else {return ComputedStyle::initial(parent,"");};
        let mut style=ComputedStyle::initial(parent,node.tag().unwrap_or(""));
        let mut winners:BTreeMap<&str,((bool,u32,u32,u32,u32,usize),&str)>=BTreeMap::new();
        let mut order=0;
        for rule in &self.rules {
            let specificity=rule.selectors.iter().filter_map(|s| selector_specificity(arena,id,s)).max();
            if let Some((a,b,c))=specificity {for d in &rule.declarations {order+=1;if !valid_value(&d.name,&d.value,style.font_size,viewport){continue;}let rank=(d.important,0,a,b,c,order);if winners.get(d.name.as_str()).map_or(true,|(old,_)|rank>=*old) {winners.insert(&d.name,(rank,&d.value));}}}
        }
        let inline=node.get_attribute("style").map(declarations).unwrap_or_default();
        for d in &inline {order+=1;if !valid_value(&d.name,&d.value,style.font_size,viewport){continue;}let rank=(d.important,1,0,0,0,order);if winners.get(d.name.as_str()).map_or(true,|(old,_)|rank>=*old) {winners.insert(&d.name,(rank,&d.value));}}
        // Font size first: em dimensions and unitless line-height use the computed size.
        if let Some((_,v))=winners.get("font-size") {if let Some(n)=resolve_length(v,parent.map_or(16.0,|p|p.font_size),viewport) {if n>0.0 {style.font_size=n.min(MAX_FONT_SIZE);style.line_height=style.font_size*1.25;}}}
        for (name,(_,value)) in winners {apply(&mut style,name,value,viewport);}
        if node.get_attribute("hidden").is_some() {style.layout.display=Display::None;}
        style
    }
}

fn media_matches(query:&str,viewport:(u32,u32))->bool {
    split_top(query,',').into_iter().any(|alternative| {
        let q=alternative.trim().to_ascii_lowercase();
        let parts:Vec<_>=q.split("and").map(str::trim).collect();
        parts.into_iter().all(|part| {
            if matches!(part,"all"|"screen"|"only screen"|"") {return true;}
            let Some(inner)=part.strip_prefix('(').and_then(|p|p.strip_suffix(')')) else {return false;};
            let Some((feature,value))=inner.split_once(':') else {return false;};
            if feature.trim()=="orientation" {return match value.trim(){"landscape"=>viewport.0>=viewport.1,"portrait"=>viewport.1>viewport.0,_=>false};}
            let Some(n)=resolve_length(value.trim(),16.0,viewport) else {return false;};
            match feature.trim(){"min-width"=>viewport.0 as f32>=n,"max-width"=>viewport.0 as f32<=n,"width"=>viewport.0 as f32==n,"min-height"=>viewport.1 as f32>=n,"max-height"=>viewport.1 as f32<=n,"height"=>viewport.1 as f32==n,_=>false}
        })
    })
}

#[derive(Clone,Copy)]
enum Relation { Descendant,Child,Adjacent,Sibling }
fn ident_end(s:&str)->usize {s.char_indices().find(|(_,c)|!c.is_ascii_alphanumeric()&&*c!='-'&&*c!='_').map_or(s.len(),|(i,_)|i)}
fn selector_parts(selector:&str)->Option<(Vec<&str>,Vec<Relation>)> {
    let mut parts=Vec::new();let mut relations=Vec::new();let mut start=0;let mut depth=0usize;let mut quote=None;let mut pending=None;
    for (i,c) in selector.char_indices() {
        if let Some(q)=quote {if c==q {quote=None;}continue;}
        if c=='\''||c=='"' {quote=Some(c);continue;}
        if c=='['||c=='(' {depth+=1;continue;} if c==']'||c==')' {depth=depth.checked_sub(1)?;continue;}
        if depth==0&&(c.is_ascii_whitespace()||matches!(c,'>'|'+'|'~')) {
            if start<i {if !parts.is_empty(){relations.push(pending.take().unwrap_or(Relation::Descendant));}parts.push(&selector[start..i]);}
            if !c.is_ascii_whitespace() {if pending.is_some_and(|r|!matches!(r,Relation::Descendant)){return None;}pending=Some(match c {'>'=>Relation::Child,'+'=>Relation::Adjacent,_=>Relation::Sibling});} else if pending.is_none(){pending=Some(Relation::Descendant);}
            start=i+c.len_utf8();
        }
    }
    if depth!=0||quote.is_some(){return None;} if start<selector.len(){if !parts.is_empty(){relations.push(pending.take().unwrap_or(Relation::Descendant));}parts.push(&selector[start..]);}
    else if pending.is_some_and(|r|!matches!(r,Relation::Descendant)){return None;}
    if parts.is_empty(){None}else{Some((parts,relations))}
}
fn previous_element(arena:&DocumentArena,id:NodeId)->Option<NodeId> {let mut p=arena.get(id)?.prev_sibling;while let Some(id)=p{let n=arena.get(id)?;if n.is_element(){return Some(id);}p=n.prev_sibling;}None}
fn compound(arena:&DocumentArena,id:NodeId,selector:&str)->Option<(u32,u32,u32)> {
    let node=arena.get(id)?;let tag=node.tag()?;let mut s=selector;let mut spec=(0,0,0);
    if let Some(rest)=s.strip_prefix('*'){s=rest;}else{let n=ident_end(s);if n>0{if !tag.eq_ignore_ascii_case(&s[..n]){return None;}spec.2+=1;s=&s[n..];}}
    while !s.is_empty() {
        let prefix=s.chars().next()?;s=&s[prefix.len_utf8()..];
        match prefix {
            '#'|'.'=>{let n=ident_end(s);if n==0{return None;}let value=&s[..n];if prefix=='#'{if node.get_attribute("id")!=Some(value){return None;}spec.0+=1;}else{if !node.get_attribute("class").unwrap_or("").split_whitespace().any(|c|c==value){return None;}spec.1+=1;}s=&s[n..];},
            '['=>{let n=s.find(']')?;let inner=s[..n].trim();let mut matched=false;
                if let Some((key,value))=inner.split_once('='){let key=key.trim();let value=value.trim().trim_matches(['\'','"']);if let Some(key)=key.strip_suffix('~'){matched=node.get_attribute(key.trim()).is_some_and(|v|v.split_whitespace().any(|v|v==value));}else{matched=node.get_attribute(key)==Some(value);}}
                else if ident_end(inner)==inner.len(){matched=node.get_attribute(inner).is_some();}
                if !matched{return None;}spec.1+=1;s=&s[n+1..];},
            ':'=>{let n=ident_end(s);let pseudo=&s[..n];s=&s[n..];
                let matched=match pseudo {
                    "root"=>node.parent.is_some_and(|p|p==arena.root),
                    "first-child"=>previous_element(arena,id).is_none(),
                    "last-child"=>{let mut next=node.next_sibling;let mut last=true;while let Some(i)=next{let n=arena.get(i)?;if n.is_element(){last=false;break;}next=n.next_sibling;}last},
                    "not"|"is"|"where"=>{let rest=s.strip_prefix('(')?;let close=rest.find(')')?;let inner=&rest[..close];s=&rest[close+1..];let result=compound(arena,id,inner);if pseudo=="not"{if result.is_some(){return None;} // Validate unsupported syntax even when it does not match.
                            if !simple_selector_valid(inner){return None;}
                        }else if result.is_none(){return None;}
                        if pseudo!="where"{let specificity=syntactic_specificity(inner)?;spec.0+=specificity.0;spec.1+=specificity.1;spec.2+=specificity.2;}continue;},
                    _=>return None,
                };if !matched{return None;}spec.1+=1;
            },_=>return None,
        }
    } Some(spec)
}
fn simple_selector_valid(s:&str)->bool {syntactic_specificity(s).is_some()}
fn syntactic_specificity(mut s:&str)->Option<(u32,u32,u32)> {
    let mut spec=(0,0,0);if let Some(rest)=s.strip_prefix('*'){s=rest;}else{let n=ident_end(s);if n>0{spec.2+=1;s=&s[n..];}}
    while !s.is_empty(){let c=s.chars().next()?;s=&s[c.len_utf8()..];match c {'#'|'.'=>{let n=ident_end(s);if n==0{return None;}if c=='#'{spec.0+=1;}else{spec.1+=1;}s=&s[n..];},'['=>{let n=s.find(']')?;spec.1+=1;s=&s[n+1..];},':'=>{let n=ident_end(s);if !matches!(&s[..n],"root"|"first-child"|"last-child"){return None;}spec.1+=1;s=&s[n..];},_=>return None}}Some(spec)
}
pub fn selector_specificity(arena:&DocumentArena,id:NodeId,selector:&str)->Option<(u32,u32,u32)> {
    let (parts,relations)=selector_parts(selector.trim())?;
    fn matches(arena:&DocumentArena,id:NodeId,parts:&[&str],relations:&[Relation])->Option<(u32,u32,u32)> {
        let own=compound(arena,id,parts.last()?)?;if parts.len()==1{return Some(own);}
        let relation=relations[relations.len()-1];let node=arena.get(id)?;
        let mut candidate=match relation{Relation::Child|Relation::Descendant=>node.parent,Relation::Adjacent|Relation::Sibling=>previous_element(arena,id)};
        while let Some(i)=candidate {if let Some(p)=matches(arena,i,&parts[..parts.len()-1],&relations[..relations.len()-1]){return Some((own.0+p.0,own.1+p.1,own.2+p.2));}candidate=match relation {Relation::Descendant=>arena.get(i)?.parent,Relation::Sibling=>previous_element(arena,i),_=>None};}None
    }
    matches(arena,id,&parts,&relations)
}

fn number(s:&str)->Option<f32>{let n=s.trim().parse::<f32>().ok()?;n.is_finite().then_some(n)}
pub fn resolve_length(s:&str,font:f32,viewport:(u32,u32))->Option<f32>{
    let s=s.trim();for (unit,factor) in [("rem",16.0),("px",1.0),("em",font),("vw",viewport.0 as f32/100.0),("vh",viewport.1 as f32/100.0)] {if let Some(v)=s.strip_suffix(unit){return number(v).map(|n|n*factor);}}(s=="0").then_some(0.0)
}
fn dimension<T:FromLength+FromPercent+TaffyAuto>(v:&str,font:f32,viewport:(u32,u32))->Option<T>{if v=="auto" {Some(auto())}else if let Some(n)=v.strip_suffix('%'){number(n).map(|n|percent(n/100.0))}else{resolve_length(v,font,viewport).map(length)}}
fn spacing(v:&str,font:f32,viewport:(u32,u32))->Option<LengthPercentage>{if let Some(n)=v.strip_suffix('%'){number(n).map(|n|percent(n/100.0))}else{resolve_length(v,font,viewport).filter(|n|*n>=0.0).map(length)}}
pub fn color(v:&str)->Option<Color>{
    let v=v.trim().to_ascii_lowercase();let c=match v.as_str(){"black"=>Color::rgb(0,0,0),"white"=>Color::rgb(255,255,255),"red"=>Color::rgb(255,0,0),"green"=>Color::rgb(0,128,0),"blue"=>Color::rgb(0,0,255),"gray"|"grey"=>Color::rgb(128,128,128),"yellow"=>Color::rgb(255,255,0),"transparent"=>Color::rgba(0,0,0,0),_=>{
        if let Some(hex)=v.strip_prefix('#'){let digit=|s:&str|u8::from_str_radix(s,16).ok();if !hex.is_ascii(){return None;}match hex.len(){3|4=>{let r=digit(&hex[0..1])?*17;let g=digit(&hex[1..2])?*17;let b=digit(&hex[2..3])?*17;Color::rgba(r,g,b,if hex.len()==4{digit(&hex[3..4])?*17}else{255})},6|8=>Color::rgba(digit(&hex[0..2])?,digit(&hex[2..4])?,digit(&hex[4..6])?,if hex.len()==8{digit(&hex[6..8])?}else{255}),_=>return None}}
        else if let Some(inner)=v.strip_prefix("rgb(").and_then(|s|s.strip_suffix(')')){let p:Vec<_>=inner.split(',').collect();if p.len()!=3{return None;}Color::rgb(number(p[0])?.clamp(0.0,255.0) as u8,number(p[1])?.clamp(0.0,255.0) as u8,number(p[2])?.clamp(0.0,255.0) as u8)}else{return None;}
    }};Some(c)
}
fn valid_value(name:&str,value:&str,font:f32,viewport:(u32,u32))->bool {
    let lower=value.to_ascii_lowercase();let v=lower.as_str();
    match name {
        "color"|"background-color"|"border-color"=>color(v).is_some(),
        "display"=>matches!(v,"block"|"inline"|"flex"|"grid"|"none"),
        "font-size"=>resolve_length(v,font,viewport).is_some_and(|n|n>0.0),
        "line-height"=>number(v).or_else(||resolve_length(v,font,viewport)).is_some_and(|n|n>0.0),
        "font-weight"=>matches!(v,"bold"|"normal"|"400"|"600"|"700"|"800"|"900"),
        "font-style"=>matches!(v,"normal"|"italic"|"oblique"),
        "font-family"=>matches!(v,"monospace"|"jetbrains mono"|"\"jetbrains mono\""|"sans-serif"|"inter"|"\"inter\""),
        "text-align"=>matches!(v,"left"|"start"|"center"|"right"|"end"),
        "white-space"=>matches!(v,"normal"|"pre"|"pre-wrap"),
        "width"|"height"|"min-width"|"max-width"|"min-height"|"max-height"|"flex-basis"=>v=="auto"||spacing(v,font,viewport).is_some(),
        "box-sizing"=>matches!(v,"border-box"|"content-box"),
        "flex-direction"=>matches!(v,"row"|"row-reverse"|"column"|"column-reverse"),
        "flex-wrap"=>matches!(v,"wrap"|"nowrap"|"wrap-reverse"),
        "flex-grow"|"flex-shrink"=>number(v).is_some_and(|n|n>=0.0),
        "align-items"|"align-self"=>matches!(v,"start"|"end"|"flex-start"|"flex-end"|"center"|"stretch"),
        "justify-content"=>matches!(v,"start"|"end"|"flex-start"|"flex-end"|"center"|"space-between"|"space-around"|"space-evenly"),
        "row-gap"|"column-gap"=>spacing(v,font,viewport).is_some(),
        "grid-template-columns"|"grid-template-rows"=>tracks(v,font,viewport).is_some(),
        _ if name.starts_with("margin-")=>dimension::<LengthPercentageAuto>(v,font,viewport).is_some(),
        _ if name.starts_with("padding-")||name.starts_with("border-")=>spacing(v,font,viewport).is_some(),
        _=>false,
    }
}

fn tracks(v:&str,font:f32,viewport:(u32,u32))->Option<Vec<GridTemplateComponent<String>>> {
    if v=="none"{return Some(Vec::new());}
    if let Some(inner)=v.strip_prefix("repeat(").and_then(|v|v.strip_suffix(')')){let (n,body)=inner.split_once(',')?;let count=n.trim().parse::<usize>().ok()?;if !(1..=128).contains(&count){return None;}let part=tracks(body.trim(),font,viewport)?;let mut out=Vec::new();for _ in 0..count{out.extend(part.iter().cloned());}return Some(out);}
    let mut out=Vec::new();for word in v.split_whitespace(){let track:TrackSizingFunction=if let Some(n)=word.strip_suffix("fr"){let n=number(n)?;if n<0.0{return None;}fr(n)}else if word=="auto"{auto()}else if let Some(n)=word.strip_suffix('%'){percent(number(n)?/100.0)}else{length(resolve_length(word,font,viewport)?)};out.push(GridTemplateComponent::Single(track));}(!out.is_empty()).then_some(out)
}
fn apply(s:&mut ComputedStyle,name:&str,value:&str,viewport:(u32,u32)) {
    let v=value.trim().to_ascii_lowercase();let v=v.as_str();let font=s.font_size;
    match name {
        "display"=>match v {"none"=>s.layout.display=Display::None,"block"=>{s.layout.display=Display::Block;s.inline=false;},"inline"=>{s.layout.display=Display::Block;s.inline=true;},"flex"=>{s.layout.display=Display::Flex;s.inline=false;},"grid"=>{s.layout.display=Display::Grid;s.inline=false;},_=>{}},
        "color"=>{if let Some(c)=color(v){s.color=c;}},"background-color"=>{if let Some(c)=color(v){s.background=Some(c);}},"border-color"=>{if let Some(c)=color(v){s.border_color=c;}},
        "font-size"=>{},"font-weight"=>match v{"bold"|"600"|"700"|"800"|"900"=>s.bold=true,"normal"|"400"=>s.bold=false,_=>{}},"font-style"=>match v{"italic"|"oblique"=>s.italic=true,"normal"=>s.italic=false,_=>{}},
        "font-family"=>s.mono=matches!(v,"monospace"|"jetbrains mono"|"\"jetbrains mono\""),
        "text-align"=>s.text_align=match v {"center"=>1,"right"|"end"=>2,_=>0},
        "line-height"=>{if let Some(n)=number(v).map(|n|n*font).or_else(||resolve_length(v,font,viewport)){if n>0.0{s.line_height=n;}}},
        "white-space"=>match v{"pre"=>{s.pre=true;s.wrap_pre=false;},"pre-wrap"=>{s.pre=true;s.wrap_pre=true;},"normal"=>{s.pre=false;s.wrap_pre=false;},_=>{}},
        "width"=>{if let Some(n)=dimension(v,font,viewport){s.layout.size.width=n;}},"height"=>{if let Some(n)=dimension(v,font,viewport){s.layout.size.height=n;}},
        "min-width"=>{if let Some(n)=dimension(v,font,viewport){s.layout.min_size.width=n;}},"max-width"=>{if let Some(n)=dimension(v,font,viewport){s.layout.max_size.width=n;}},
        "min-height"=>{if let Some(n)=dimension(v,font,viewport){s.layout.min_size.height=n;}},"max-height"=>{if let Some(n)=dimension(v,font,viewport){s.layout.max_size.height=n;}},
        "box-sizing"=>match v{"border-box"=>s.layout.box_sizing=BoxSizing::BorderBox,"content-box"=>s.layout.box_sizing=BoxSizing::ContentBox,_=>{}},
        "flex-direction"=>match v{"row"=>s.layout.flex_direction=FlexDirection::Row,"column"=>s.layout.flex_direction=FlexDirection::Column,"row-reverse"=>s.layout.flex_direction=FlexDirection::RowReverse,"column-reverse"=>s.layout.flex_direction=FlexDirection::ColumnReverse,_=>{}},
        "flex-wrap"=>match v{"wrap"=>s.layout.flex_wrap=FlexWrap::Wrap,"nowrap"=>s.layout.flex_wrap=FlexWrap::NoWrap,"wrap-reverse"=>s.layout.flex_wrap=FlexWrap::WrapReverse,_=>{}},
        "flex-grow"=>{if let Some(n)=number(v).filter(|n|*n>=0.0){s.layout.flex_grow=n;}},"flex-shrink"=>{if let Some(n)=number(v).filter(|n|*n>=0.0){s.layout.flex_shrink=n;}},"flex-basis"=>{if let Some(n)=dimension(v,font,viewport){s.layout.flex_basis=n;}},
        "align-items"|"align-self"=>{let alignment=match v{"start"=>Some(AlignItems::START),"end"=>Some(AlignItems::END),"flex-start"=>Some(AlignItems::FLEX_START),"flex-end"=>Some(AlignItems::FLEX_END),"center"=>Some(AlignItems::CENTER),"stretch"=>Some(AlignItems::STRETCH),_=>None};if let Some(a)=alignment{if name=="align-items"{s.layout.align_items=Some(a);}else{s.layout.align_self=Some(a);}}},
        "justify-content"=>{let a=match v{"start"=>Some(JustifyContent::START),"end"=>Some(JustifyContent::END),"flex-start"=>Some(JustifyContent::FLEX_START),"flex-end"=>Some(JustifyContent::FLEX_END),"center"=>Some(JustifyContent::CENTER),"space-between"=>Some(JustifyContent::SPACE_BETWEEN),"space-around"=>Some(JustifyContent::SPACE_AROUND),"space-evenly"=>Some(JustifyContent::SPACE_EVENLY),_=>None};if a.is_some(){s.layout.justify_content=a;}},
        "row-gap"=>{if let Some(n)=spacing(v,font,viewport){s.layout.gap.height=n;}},"column-gap"=>{if let Some(n)=spacing(v,font,viewport){s.layout.gap.width=n;}},
        "grid-template-columns"=>{if let Some(t)=tracks(v,font,viewport){s.layout.grid_template_columns=t;}},"grid-template-rows"=>{if let Some(t)=tracks(v,font,viewport){s.layout.grid_template_rows=t;}},
        _=>{
            let (side, kind) = if let Some(side)=name.strip_prefix("margin-") {(side,0)}
                else if let Some(side)=name.strip_prefix("padding-") {(side,1)}
                else if let Some(side)=name.strip_prefix("border-").and_then(|s|s.strip_suffix("-width")) {(side,2)}
                else {return;};
            let index=match side {"left"=>0,"right"=>1,"top"=>2,"bottom"=>3,_=>return};
            if kind==0 {if let Some(n)=dimension(v,font,viewport){match index{0=>s.layout.margin.left=n,1=>s.layout.margin.right=n,2=>s.layout.margin.top=n,_=>s.layout.margin.bottom=n}}}
            else if let Some(n)=spacing(v,font,viewport){let r=if kind==1{&mut s.layout.padding}else{&mut s.layout.border};match index{0=>r.left=n,1=>r.right=n,2=>r.top=n,_=>r.bottom=n}}
        }
    }
}

#[cfg(test)]
#[path="css_tests.rs"]
mod tests;
