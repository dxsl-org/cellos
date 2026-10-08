// SPDX-License-Identifier: MIT
use super::*;
fn element(arena:&mut DocumentArena,parent:NodeId,tag:&str,attrs:&[(&str,&str)])->NodeId {
    let id=arena.alloc_node(NodeData::Element{tag:String::from(tag),attributes:attrs.iter().map(|(k,v)|(String::from(*k),String::from(*v))).collect()});assert!(arena.append_child(parent,id));id
}
fn text(arena:&mut DocumentArena,parent:NodeId,value:&str) {let id=arena.alloc_node(NodeData::Text(String::from(value)));assert!(arena.append_child(parent,id));}
fn style(arena:&mut DocumentArena,value:&str) {let root=arena.root;let id=element(arena,root,"style",&[]);text(arena,id,value);}
fn box_for(layout:&WebLayout,id:NodeId)->&WebBox {layout.boxes.iter().find(|b|b.node_id==Some(id)).unwrap()}
fn laid_out(arena:&DocumentArena,external:&[(NodeId,String)],width:u32,height:u32)->WebLayout {
    layout(&mut TextFonts::new(),arena,external,width,height).unwrap()
}
#[test]
fn taffy_flex_geometry_responds_to_viewport_and_media_queries() {
    let mut arena=DocumentArena::new();let root=arena.root;
    style(&mut arena,"main{display:flex;gap:10px} .item{flex:1;min-width:0;height:20px} @media(max-width:400px){main{flex-direction:column}}");
    let main=element(&mut arena,root,"main",&[]);let a=element(&mut arena,main,"div",&[("class","item")]);let b=element(&mut arena,main,"div",&[("class","item")]);text(&mut arena,a,"first");text(&mut arena,b,"second");
    let wide=laid_out(&arena,&[],600,400);let ab=box_for(&wide,a);let bb=box_for(&wide,b);
    assert_eq!(ab.width,295);assert_eq!(bb.x,ab.x+305);assert_eq!(ab.y,bb.y);
    let narrow=laid_out(&arena,&[],300,400);let ab=box_for(&narrow,a);let bb=box_for(&narrow,b);
    assert_eq!(ab.width,300);assert_eq!(ab.x,bb.x);assert_eq!(bb.y,ab.y+30);
}
#[test]
fn taffy_grid_places_tracks_and_wraps_to_next_row() {
    let mut arena=DocumentArena::new();let root=arena.root;
    style(&mut arena,"main{display:grid;grid-template-columns:repeat(2,1fr);gap:10px} .item{height:20px;min-width:0}");
    let main=element(&mut arena,root,"main",&[]);let a=element(&mut arena,main,"div",&[("class","item")]);let b=element(&mut arena,main,"div",&[("class","item")]);let c=element(&mut arena,main,"div",&[("class","item")]);
    let laid=laid_out(&arena,&[],300,200);let ab=box_for(&laid,a);let bb=box_for(&laid,b);let cb=box_for(&laid,c);
    assert_eq!(ab.width,145);assert_eq!(bb.x,155);assert_eq!(cb.x,ab.x);assert_eq!(cb.y,ab.y+30);
}
#[test]
fn relayout_observes_dom_text_style_and_class_changes_without_losing_css() {
    let mut arena=DocumentArena::new();let root=arena.root;
    style(&mut arena,".wide{width:200px;background-color:red}.narrow{width:40px;background-color:blue}");
    let target=element(&mut arena,root,"div",&[("class","wide")]);text(&mut arena,target,"initial");
    let mut doc=crate::doc::Document::new();doc.arena=Some(arena);assert!(doc.relayout_from_arena(&mut TextFonts::new(),400,300));
    assert_eq!(box_for(doc.web_layout.as_ref().unwrap(),target).width,200);
    let arena=doc.arena.as_mut().unwrap();if let NodeData::Element{attributes,..}=&mut arena.get_mut(target).unwrap().data{attributes[0].1=String::from("narrow");}
    let text_id=arena.get(target).unwrap().first_child.unwrap();arena.get_mut(text_id).unwrap().data=NodeData::Text(String::from("changed"));
    assert!(doc.relayout_from_arena(&mut TextFonts::new(),400,300));let target_box=box_for(doc.web_layout.as_ref().unwrap(),target);
    assert_eq!(target_box.width,40);assert_eq!(target_box.background,Some(Color::rgb(0,0,255)));assert!(!doc.search("initial").iter().any(|_|true));
    assert!(!doc.search("cha").is_empty());
}
#[test]
fn inline_links_keep_node_ids_and_unicode_uses_glyph_not_utf8_width() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let p=element(&mut arena,root,"p",&[("style","width:32px;margin:0")]);let a=element(&mut arena,p,"a",&[("href","/next")]);text(&mut arena,a,"ééééé");
    let laid=laid_out(&arena,&[],100,100);let text_box=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
    assert!(text_box.lines.len()>1);
    let painted:String=text_box.lines.iter().flat_map(|l|l.runs.iter()).map(|r|r.span.text.as_str()).collect();
    assert_eq!(painted,"ééééé");
    let mut fonts=TextFonts::new();
    for line in &text_box.lines {
        assert!(line.width<=32.0);
        for run in &line.runs {assert!((run.width-fonts.measure(run.face(),&run.span.text,run.px)).abs()<0.01);}
    }
    assert_eq!(laid.hit_link(text_box.x+1,text_box.y+1),Some(String::from("/next")));
    assert_eq!(laid.hit_node(text_box.x+1,text_box.y+1),Some(a));
}
#[test]
fn external_stylesheets_follow_dom_source_order_and_hidden_content_is_not_searched() {
    let mut arena=DocumentArena::new();let root=arena.root;
    style(&mut arena,"p{color:red}");let link=element(&mut arena,root,"link",&[("rel","stylesheet")]);style(&mut arena,"p{color:green}");
    let p=element(&mut arena,root,"p",&[]);text(&mut arena,p,"visible");let hidden=element(&mut arena,root,"p",&[("style","display:none")]);text(&mut arena,hidden,"secret");
    let laid=laid_out(&arena,&[(link,String::from("p{color:blue}"))],300,200);let text_box=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
    assert_eq!(text_box.lines[0].runs[0].span.color,Some(Color::rgb(0,128,0)));assert!(laid.search("secret").is_empty());assert!(!laid.search("visible").is_empty());
}
#[test]
fn explicit_line_breaks_are_not_collapsed_with_html_whitespace() {
    let mut arena=DocumentArena::new();let root=arena.root;let p=element(&mut arena,root,"p",&[]);text(&mut arena,p,"one");element(&mut arena,p,"br",&[]);text(&mut arena,p,"two");
    let laid=laid_out(&arena,&[],300,200);let text_box=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();assert_eq!(text_box.lines.len(),2);
}

#[test]
fn pre_wrap_preserves_spaces_while_wrapping_at_viewport_width() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let p=element(&mut arena,root,"div",&[("style","width:24px;white-space:pre-wrap")]);text(&mut arena,p,"a  bc");
    let laid=laid_out(&arena,&[],100,100);let text_box=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
    assert!(text_box.lines.len()>1);
    let painted:String=text_box.lines.iter().flat_map(|l|l.runs.iter()).map(|r|r.span.text.as_str()).collect();
    assert_eq!(painted,"a  bc");
    assert!(text_box.lines.iter().all(|l|l.width<=24.0));
}

#[test]
fn vietnamese_canonical_forms_share_wrapping_and_advances() {
    let mut fonts=TextFonts::new();
    let nfc=TextRun::new(StyledSpan::plain("Tiếng Việt ở Huế"),16.0,None,20.0);
    let nfd=TextRun::new(StyledSpan::plain("Tie\u{302}\u{301}ng Vie\u{323}\u{302}t o\u{31b}\u{309} Hue\u{302}\u{301}"),16.0,None,20.0);
    let a=wrap_runs(&mut fonts,&[nfc],65.0,20,false,true,0);
    let b=wrap_runs(&mut fonts,&[nfd],65.0,20,false,true,0);
    assert_eq!(a.len(),b.len());
    for (a,b) in a.iter().zip(&b) {
        assert_eq!(a.height,b.height);
        assert!((a.width-b.width).abs()<0.01);
        assert_eq!(a.runs[0].span.text,b.runs[0].span.text);
    }
}

#[test]
fn css_sizes_faces_baselines_alignment_and_link_rectangles_are_measured() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let p=element(&mut arena,root,"p",&[("style","width:240px;margin:0;text-align:right")]);
    let a=element(&mut arena,p,"a",&[("href","/type"),("style","font-size:23px;font-weight:600;font-style:italic")]);
    text(&mut arena,a,"AV Tiếng Việt");
    let code=element(&mut arena,p,"code",&[]);text(&mut arena,code,"let");
    let laid=laid_out(&arena,&[],300,200);
    let b=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
    let line=&b.lines[0];let run=&line.runs[0];
    let mut fonts=TextFonts::new();
    assert_eq!(run.px,23.0);assert_eq!(run.face(),FontFace::UiSemiboldItalic);
    assert_eq!(line.runs[1].face(),FontFace::MonoRegular);
    assert!((run.width-fonts.measure(run.face(),&run.span.text,23.0)).abs()<0.01);
    assert!((run.x+line.width-b.width as f32).abs()<0.01);
    assert!(line.baseline>=fonts.ascender(run.face(),23.0));
    let left=b.x+run.x as i32+1;
    assert_eq!(laid.hit_link(left,b.y+1),Some(String::from("/type")));
    assert_eq!(laid.hit_node(left,b.y+1),Some(a));
    assert_eq!(laid.hit_link(b.x,b.y+1),None);
}

#[test]
fn markdown_and_plain_text_wrap_using_pixel_width_and_real_faces() {
    use crate::doc::{DocNode,Document};
    let mut fonts=TextFonts::new();let mut doc=Document::new();
    let mut styled=StyledSpan::plain("AV Tiếng Việt long words");styled.bold=true;styled.italic=true;
    doc.nodes=alloc::vec![
        DocNode::Paragraph{spans:alloc::vec![styled]},
        DocNode::RawLines{lines:alloc::vec![String::from("Tiếng Việt long words")]},
        DocNode::Table{headers:alloc::vec![String::from("Tiếng Việt")],
            rows:alloc::vec![alloc::vec![String::from("long words repeat repeat")]]},
    ];
    doc.compute_layout(&mut fonts,112);
    for b in &doc.layout_boxes[..2] {
        assert!(b.lines.len()>1);
        for line in &b.lines {
            assert!(line.width<=72.0);
            for run in &line.runs {assert!((run.width-fonts.measure(run.face(),&run.span.text,run.px)).abs()<0.01);}
        }
    }
    assert_eq!(doc.layout_boxes[0].lines[0].runs[0].face(),FontFace::UiSemiboldItalic);
    let rows=doc.layout_boxes[2].table_rows.as_ref().unwrap();
    assert!(rows[1].cells[0].lines.len()>1);
    assert_eq!(rows[1].height,rows[1].cells[0].lines.iter().map(|l|l.height).sum::<u32>()+12);
}

#[test]
fn oversized_css_font_sizes_are_bounded_before_layout_and_paint() {
    let mut arena=DocumentArena::new();let root=arena.root;
    let parent=element(&mut arena,root,"div",&[("style","font-size:1000000px;line-height:1.5;width:400px")]);
    let inherited=element(&mut arena,parent,"span",&[]);text(&mut arena,inherited,"AV");
    let large_em=element(&mut arena,parent,"span",&[("style","font-size:1000000em")]);
    text(&mut arena,large_em," Việt");
    let sheet=Stylesheet::default();
    let parent_style=sheet.compute(&arena,parent,None,(500,300));
    assert_eq!(parent_style.font_size,crate::parser::css::MAX_FONT_SIZE);
    assert_eq!(parent_style.line_height,192.0);
    assert_eq!(sheet.compute(&arena,inherited,Some(&parent_style),(500,300)).font_size,128.0);
    assert_eq!(sheet.compute(&arena,large_em,Some(&parent_style),(500,300)).font_size,128.0);
    let laid=laid_out(&arena,&[],500,300);let mut fonts=TextFonts::new();
    for b in &laid.boxes {for line in &b.lines {for run in &line.runs {
        assert_eq!(run.px,128.0);
        assert!(run.line_height<=192.0);
        assert!(line.height<=192);
        assert!((run.width-fonts.measure(run.face(),&run.span.text,run.px)).abs()<0.01);
    }}}
}

#[test]
fn collapsed_spaces_keep_their_source_link_node_style_and_hit_rectangle() {
    let mut fonts=TextFonts::new();
    for inside_anchor in [false,true] {
        let mut arena=DocumentArena::new();let root=arena.root;
        let p=element(&mut arena,root,"p",&[("style","font-size:11px;margin:0;width:400px")]);
        if !inside_anchor {text(&mut arena,p,"foo ");}
        let a=element(&mut arena,p,"a",&[("href","/next"),("style","font-size:23px")]);
        text(&mut arena,a,if inside_anchor {"foo "}else{"bar"});
        if inside_anchor {text(&mut arena,p,"bar");}
        let laid=laid_out(&arena,&[],500,300);
        let b=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
        let first=&b.lines[0].runs[0];
        assert_eq!(first.span.text,"foo ");
        assert_eq!(first.node_id,Some(if inside_anchor {a}else{p}));
        assert_eq!(first.px,if inside_anchor {23.0}else{11.0});
        assert!((first.width-fonts.measure(first.face(),"foo ",first.px)).abs()<0.01);
        let gap=b.x+(fonts.measure(first.face(),"foo",first.px)
            +fonts.advance(first.face(),' ',first.px)*0.5) as i32;
        assert_eq!(laid.hit_node(gap,b.y+1),Some(if inside_anchor {a}else{p}));
        assert_eq!(laid.hit_link(gap,b.y+1),
            if inside_anchor {Some(String::from("/next"))}else{None});
    }
}

#[test]
fn parsed_html_preserves_whitespace_between_styled_inline_elements() {
    let parsed=crate::parser::html::parse_html("<p><b>đậm</b> <i>nghiêng</i></p>");
    let laid=laid_out(&parsed.arena,&[],400,200);
    let b=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
    let runs=&b.lines[0].runs;
    let painted:String=runs.iter().map(|r|r.span.text.as_str()).collect();
    assert_eq!(painted,"đậm nghiêng");
    assert_eq!(runs[0].face(),FontFace::UiSemibold);
    assert_eq!(runs[1].span.text," ");
    assert_eq!(runs[2].face(),FontFace::UiItalic);
}

#[test]
fn parsed_separate_whitespace_nodes_keep_anchor_hit_ownership() {
    for inside_anchor in [false,true] {
        let html=if inside_anchor {
            "<p><a href='/next'><b>foo</b> </a>bar</p>"
        } else {
            "<p><b>foo</b> <a href='/next'>bar</a></p>"
        };
        let parsed=crate::parser::html::parse_html(html);
        let p=parsed.arena.nodes.iter().find(|n|n.tag()==Some("p")).unwrap().id;
        let a=parsed.arena.nodes.iter().find(|n|n.tag()==Some("a")).unwrap().id;
        let laid=laid_out(&parsed.arena,&[],400,200);
        let b=laid.boxes.iter().find(|b|!b.lines.is_empty()).unwrap();
        let space=b.lines[0].runs.iter().find(|r|r.span.text==" ").unwrap();
        assert_eq!(space.node_id,Some(if inside_anchor {a}else{p}));
        let x=b.x+(space.x+space.width*0.5) as i32;
        assert_eq!(laid.hit_node(x,b.y+1),space.node_id);
        assert_eq!(laid.hit_link(x,b.y+1),
            if inside_anchor {Some(String::from("/next"))}else{None});
    }
}
