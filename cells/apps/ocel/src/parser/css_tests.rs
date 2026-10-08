// SPDX-License-Identifier: MIT
use super::*;
use dom_arena::NodeData;
fn element(arena:&mut DocumentArena,parent:NodeId,tag:&str,attrs:&[(&str,&str)])->NodeId {
    let id=arena.alloc_node(NodeData::Element{tag:String::from(tag),attributes:attrs.iter().map(|(k,v)|(String::from(*k),String::from(*v))).collect()});
    assert!(arena.append_child(parent,id));id
}
#[test]
fn compound_combinators_attributes_and_unsupported_selectors_are_exact() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let section=element(&mut arena,root,"section",&[("class","outer")]);
    let a=element(&mut arena,section,"p",&[("id","intro"),("class","card selected"),("data-kind","news")]);
    let b=element(&mut arena,section,"p",&[("class","card")]);
    assert_eq!(selector_specificity(&arena,a,"section.outer > p#intro.card[data-kind='news']:first-child"),Some((1,4,2)));
    assert!(selector_specificity(&arena,b,"#intro + .card:last-child").is_some());
    assert!(selector_specificity(&arena,b,"section .card").is_some());
    assert!(selector_specificity(&arena,a,".card:not(.missing)").is_some());
    assert!(selector_specificity(&arena,a,".card:not(:hover)").is_none());
    assert!(selector_specificity(&arena,a,".card:hover").is_none());
    assert!(selector_specificity(&arena,a,"div.card").is_none());
    assert!(selector_specificity(&arena,b,"section > #intro").is_none());
}
#[test]
fn important_specificity_inline_and_order_compete_per_property() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let id=element(&mut arena,root,"p",&[("id","lead"),("class","card"),("style","color: green; background-color: blue !important")]);
    let mut sheet=Stylesheet::default();sheet.append("p {color:black} .card {color:red !important;background-color:red !important} #lead {color:blue !important} #lead {color:white !important}",(800,600));
    let style=sheet.compute(&arena,id,None,(800,600));
    assert_eq!(style.color,Color::rgb(255,255,255));
    assert_eq!(style.background,Some(Color::rgb(0,0,255)));
}
#[test]
fn invalid_values_do_not_replace_valid_values_and_spacing_shorthands_expand() {
    let mut arena=DocumentArena::new();let root=arena.root;let id=element(&mut arena,root,"div",&[]);
    let mut sheet=Stylesheet::default();sheet.append("div {color:red;color:bogus;margin: 2px 4px; margin-left: 9px;gap: 4px 8px;column-gap:12px}",(800,600));
    let s=sheet.compute(&arena,id,None,(800,600));
    assert_eq!(s.color,Color::rgb(255,0,0));assert_eq!(s.layout.margin.left,length(9.0));assert_eq!(s.layout.margin.top,length(2.0));
    assert_eq!(s.layout.gap.width,length(12.0));assert_eq!(s.layout.gap.height,length(4.0));
}
#[test]
fn inheritance_responsive_media_and_viewport_units_are_recomputed() {
    let mut arena=DocumentArena::new();let root=arena.root;let parent=element(&mut arena,root,"main",&[]);let child=element(&mut arena,parent,"p",&[]);
    let source="main{color:#123456;font-size:32px} p{width:50vw} @media screen and (max-width:500px){p{color:red;width:100%}}";
    let mut sheet=Stylesheet::default();sheet.append(source,(800,600));let p=sheet.compute(&arena,parent,None,(800,600));let s=sheet.compute(&arena,child,Some(&p),(800,600));
    assert_eq!(s.color,Color::rgb(0x12,0x34,0x56));assert_eq!(s.font_size,32.0);assert_eq!(s.layout.size.width,length(400.0));
    let mut narrow=Stylesheet::default();narrow.append(source,(400,600));let s=narrow.compute(&arena,child,Some(&p),(400,600));assert_eq!(s.color,Color::rgb(255,0,0));assert_eq!(s.layout.size.width,percent(1.0));
}
#[test]
fn comments_unknown_at_rules_and_nested_media_do_not_leak_rules() {
    let mut arena=DocumentArena::new();let root=arena.root;let id=element(&mut arena,root,"p",&[]);
    let mut sheet=Stylesheet::default();sheet.append("/* { } */ @supports (display:grid){p{color:red}} p { color: blue } @media (min-width:900px){p{color:white}}",(400,600));
    assert_eq!(sheet.compute(&arena,id,None,(400,600)).color,Color::rgb(0,0,255));
}

#[test]
fn flex_shorthand_and_longhand_share_the_same_cascade_properties() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let id=element(&mut arena,root,"div",&[("class","item")]);
    let mut sheet=Stylesheet::default();sheet.append(".item{flex:1}div{flex-grow:5}.item{flex-shrink:0}",(800,600));
    let s=sheet.compute(&arena,id,None,(800,600));
    assert_eq!(s.layout.flex_grow,1.0);assert_eq!(s.layout.flex_shrink,0.0);assert_eq!(s.layout.flex_basis,percent(0.0));
}
