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

#[test]
fn gradient_and_rgba_backgrounds_extract_colors_correctly() {
    let mut arena = DocumentArena::new();
    let root = arena.root;
    let header = element(&mut arena, root, "div", &[("class", "header__top")]);
    let card = element(&mut arena, root, "div", &[("class", "card")]);
    let mut sheet = Stylesheet::default();
    sheet.append(
        ".header__top { background: linear-gradient(90.13deg, #e30e16 18.76%, #c90009 96.9%); }\
         .card { background-color: rgba(231, 239, 250, 0.5); }",
        (1024, 768),
    );
    let header_style = sheet.compute(&arena, header, None, (1024, 768));
    assert_eq!(header_style.background, Some(Color::rgba(0xe3, 0x0e, 0x16, 255)));

    let card_style = sheet.compute(&arena, card, None, (1024, 768));
    assert_eq!(card_style.background, Some(Color::rgba(231, 239, 250, 127)));
}

#[test]
fn nth_child_selectors_match_element_indices() {
    let mut arena = DocumentArena::new();
    let root = arena.root;
    let list = element(&mut arena, root, "ul", &[]);
    let li1 = element(&mut arena, list, "li", &[("class", "item")]);
    let li2 = element(&mut arena, list, "li", &[("class", "item")]);
    let li3 = element(&mut arena, list, "li", &[("class", "item")]);
    let li4 = element(&mut arena, list, "li", &[("class", "item")]);

    assert!(selector_specificity(&arena, li1, "li:nth-child(1)").is_some());
    assert!(selector_specificity(&arena, li2, "li:nth-child(1)").is_none());
    assert!(selector_specificity(&arena, li2, "li:nth-child(2)").is_some());
    assert!(selector_specificity(&arena, li2, "li:nth-child(2n)").is_some());
    assert!(selector_specificity(&arena, li4, "li:nth-child(2n)").is_some());
    assert!(selector_specificity(&arena, li3, "li:nth-child(2n)").is_none());
    assert!(selector_specificity(&arena, li3, "li:nth-child(n+3)").is_some());
    assert!(selector_specificity(&arena, li4, "li:nth-child(n+3)").is_some());
    assert!(selector_specificity(&arena, li2, "li:nth-child(n+3)").is_none());
    assert!(selector_specificity(&arena, li1, "li:nth-child(odd)").is_some());
    assert!(selector_specificity(&arena, li2, "li:nth-child(even)").is_some());
}

#[test]
fn calc_and_var_fallback_compute_proper_dimensions() {
    let mut arena = DocumentArena::new();
    let root = arena.root;
    let el = element(&mut arena, root, "div", &[("class", "box")]);
    let mut sheet = Stylesheet::default();
    sheet.append(
        ".box { \
            width: calc(100% - 331px); \
            height: calc(100vh - 90px); \
            margin-right: var(--bullet-gap, 8px); \
            font-weight: 500; \
            overflow: hidden; \
            display: inline-block; \
            border-bottom: 2px solid #ed1c24; \
            border-top: 0; \
        }",
        (1280, 800),
    );
    let s = sheet.compute(&arena, el, None, (1280, 800));
    use taffy::util::ResolveOrZero;
    assert_eq!(s.layout.size.width.resolve_or_zero(Some(1280.0),|h,b|resolve_calc(&s.calcs,h,b)),949.0);
    assert_eq!(s.layout.size.width.resolve_or_zero(Some(600.0),|h,b|resolve_calc(&s.calcs,h,b)),269.0);
    assert_eq!(s.layout.size.height, length(710.0));
    assert_eq!(s.layout.margin.right, length(8.0));
    assert_eq!(s.bold, true);
    assert_eq!(s.layout.overflow.x, Overflow::Hidden);
    assert_eq!(s.layout.overflow.y, Overflow::Hidden);
    assert_eq!(s.inline, true);
    assert_eq!(s.layout.border.bottom, length(2.0));
    assert_eq!(s.layout.border.top, length(0.0));
    assert_eq!(s.border_color, Color::rgb(0xed, 0x1c, 0x24));
}



