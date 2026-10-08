// SPDX-License-Identifier: MIT
//! Document representation, styling, and layout engine for Ocel.

extern crate alloc;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::draw::{self, theme, Color};
use crate::parser::web::{paint_lines, wrap_runs, TextLine, TextRun};
use ostd::display::ViSurface;
use ostd::typography::{FontFace, TextFonts};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
    pub color: Option<Color>,
}

impl StyledSpan {
    pub fn plain(text: &str) -> Self {
        Self {
            text: String::from(text),
            bold: false,
            italic: false,
            code: false,
            link: None,
            color: None,
        }
    }

    pub fn code(text: &str) -> Self {
        Self {
            text: String::from(text),
            bold: false,
            italic: false,
            code: true,
            link: None,
            color: Some(theme::ACCENT_CYAN),
        }
    }

    pub fn face(&self) -> FontFace {
        if self.code {
            if self.bold { FontFace::MonoBold } else { FontFace::MonoRegular }
        } else {
            match (self.bold, self.italic) {
                (false, false) => FontFace::UiRegular,
                (true, false) => FontFace::UiSemibold,
                (false, true) => FontFace::UiItalic,
                (true, true) => FontFace::UiSemiboldItalic,
            }
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum DocNode {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        spans: Vec<StyledSpan>,
    },
    CodeBlock {
        lang: String,
        lines: Vec<String>,
    },
    ListItem {
        bullet: char,
        indent: u8,
        spans: Vec<StyledSpan>,
    },
    Blockquote {
        lines: Vec<String>,
    },
    Rule,
    RawLines {
        lines: Vec<String>,
    },
    Image {
        width: u32,
        height: u32,
        pixels: Rc<Vec<u8>>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Button {
        text: String,
        node_id: Option<dom_arena::NodeId>,
    },
}

#[derive(Clone, Debug)]
pub struct TableCellLayout {
    pub lines: Vec<TextLine>,
    pub x: i32,
    pub width: u32,
}
#[derive(Clone, Debug)]
pub struct TableRowLayout {
    pub y_rel: i32,
    pub height: u32,
    pub cells: Vec<TableCellLayout>,
    pub is_header: bool,
}
#[derive(Clone, Debug)]
pub struct LayoutBox {
    pub y_offset: i32,
    pub height: i32,
    pub lines: Vec<TextLine>,
    pub bg_color: Option<Color>,
    pub border_color: Option<Color>,
    pub is_rule: bool,
    pub left_margin: i32,
    pub image: Option<(u32, u32, Rc<Vec<u8>>)>,
    pub table_rows: Option<Vec<TableRowLayout>>,
    pub table_width: u32,
    pub node_id: Option<dom_arena::NodeId>,
}
impl LayoutBox {
    fn new(y: i32) -> Self {
        Self { y_offset:y, height:0, lines:Vec::new(), bg_color:None,
            border_color:None, is_rule:false, left_margin:0, image:None,
            table_rows:None, table_width:0, node_id:None }
    }
}
pub struct Document {
    pub nodes: Vec<DocNode>,
    pub layout_boxes: Vec<LayoutBox>,
    pub total_height: i32,
    pub arena: Option<dom_arena::DocumentArena>,
    /// Loaded CSS keyed by its DOM link node, preserving style/link source order.
    pub external_stylesheets: Vec<(dom_arena::NodeId, String)>,
    pub web_layout: Option<crate::parser::web::WebLayout>,
}

fn lines_for(fonts:&mut TextFonts,spans:&[StyledSpan],px:f32,width:f32,pre:bool)->Vec<TextLine> {
    let mut runs=Vec::new();
    for span in spans {crate::parser::web::append_run(&mut runs,TextRun::new(span.clone(),px,None,0.0));}
    let height=crate::parser::web::ceil_pixel(fonts.line_height(
        spans.first().map_or(FontFace::UiRegular,StyledSpan::face),px));
    wrap_runs(fonts,&runs,width,height,pre,true,0)
}
fn lines_height(lines:&[TextLine])->i32 { lines.iter().map(|l|l.height as i32).sum() }

impl Document {
    pub fn new() -> Self {
        Self { nodes:Vec::new(), layout_boxes:Vec::new(), total_height:0,
            arena:None, external_stylesheets:Vec::new(), web_layout:None }
    }
    pub fn clear(&mut self) {
        self.nodes.clear();self.layout_boxes.clear();self.total_height=0;
        self.arena=None;self.external_stylesheets.clear();self.web_layout=None;
    }
    pub fn search(&self,query:&str)->Vec<i32> {
        if let Some(web)=&self.web_layout {return web.search(query);}
        let query:String=ostd::typography::normalized_chars(query.trim()).map(|c|c.to_ascii_lowercase()).collect();
        if query.is_empty(){return Vec::new();}
        let mut matches=Vec::new();let mut text=String::new();
        for b in &self.layout_boxes {
            let mut found=false;
            for line in &b.lines {
                text.clear();for run in &line.runs {text.push_str(&run.span.text);}
                text.make_ascii_lowercase();found|=text.contains(&query);
            }
            if let Some(rows)=&b.table_rows {for row in rows {for cell in &row.cells {for line in &cell.lines {
                text.clear();for run in &line.runs {text.push_str(&run.span.text);}
                text.make_ascii_lowercase();found|=text.contains(&query);
            }}}}
            if found {matches.push(b.y_offset);}
        }
        matches
    }
    pub fn relayout_from_arena(&mut self,fonts:&mut TextFonts,width:u32,height:u32)->bool {
        let Some(arena)=&self.arena else{return false;};
        match crate::parser::web::layout(fonts,arena,&self.external_stylesheets,width,height) {
            Ok(layout)=>{
                self.nodes=crate::parser::html::arena_to_doc_nodes(arena);
                self.layout_boxes.clear();self.total_height=layout.height;
                self.web_layout=Some(layout);true
            }
            Err(_)=>{self.layout_boxes.clear();self.web_layout=None;self.total_height=0;false}
        }
    }
    pub fn compute_layout(&mut self,fonts:&mut TextFonts,content_width:u32) {
        if self.arena.is_some(){self.relayout_from_arena(fonts,content_width,600);return;}
        self.web_layout=None;self.layout_boxes.clear();
        let mut y=16;
        let width=content_width.saturating_sub(40).max(1) as f32;
        for node in &self.nodes {
            let mut b=LayoutBox::new(y);
            let mut gap=0;
            match node {
                DocNode::Heading{level,text}=>{
                    let px=match level {1=>32.0,2=>26.0,3=>22.0,_=>18.0};
                    let mut span=StyledSpan::plain(text);span.bold=true;
                    if *level==1 {span.color=Some(theme::ACCENT_BLUE);}
                    b.lines=lines_for(fonts,&[span],px,width,false);gap=8;
                }
                DocNode::Paragraph{spans}=>{
                    b.lines=lines_for(fonts,spans,16.0,width,false);gap=6;
                }
                DocNode::CodeBlock{lang,lines}=>{
                    let mut highlighter=crate::parser::syntax::Highlighter::new(lang);
                    for text in lines {
                        let mut spans=highlighter.line(text);
                        for span in &mut spans {span.code=true;}
                        b.lines.extend(lines_for(fonts,&spans,14.0,(width-8.0).max(1.0),true));
                    }
                    b.bg_color=Some(theme::BG_INPUT);b.border_color=Some(theme::BORDER);
                    b.left_margin=8;gap=8;
                }
                DocNode::ListItem{bullet,indent,spans}=>{
                    b.left_margin=*indent as i32*20+12;
                    let mut list=alloc::vec![StyledSpan::plain(&format!("{} ",bullet))];
                    list[0].bold=true;list[0].color=Some(theme::ACCENT_CYAN);
                    list.extend_from_slice(spans);
                    b.lines=lines_for(fonts,&list,16.0,(width-b.left_margin as f32).max(1.0),false);
                }
                DocNode::Blockquote{lines}=>{
                    b.left_margin=16;b.border_color=Some(theme::ACCENT_BLUE);
                    for text in lines {
                        let mut span=StyledSpan::plain(text);span.italic=true;span.color=Some(theme::TEXT_MUTED);
                        b.lines.extend(lines_for(fonts,&[span],16.0,(width-16.0).max(1.0),false));
                    }
                    gap=4;
                }
                DocNode::RawLines{lines}=>{
                    b.left_margin=4;
                    for text in lines {
                        b.lines.extend(lines_for(fonts,&[StyledSpan::plain(text)],16.0,(width-4.0).max(1.0),true));
                    }
                }
                DocNode::Rule=>{b.is_rule=true;b.height=16;}
                DocNode::Image{width:iw,height:ih,pixels}=>{
                    let display_w=(*iw).min(content_width.saturating_sub(32).max(1));
                    let display_h=((*ih as u64*display_w as u64)/(*iw).max(1) as u64).max(1) as u32;
                    b.height=display_h as i32+16;b.left_margin=16;
                    b.image=Some((*iw,*ih,pixels.clone()));
                }
                DocNode::Table{headers,rows}=>{
                    let count=headers.len().max(rows.iter().map(|r|r.len()).max().unwrap_or(0));
                    if count==0{continue;}
                    let mut desired=alloc::vec![12.0f32;count];
                    for (i,text) in headers.iter().enumerate() {
                        desired[i]=desired[i].max(fonts.measure(FontFace::UiSemibold,text,16.0)+12.0);
                    }
                    for row in rows {for (i,text) in row.iter().enumerate() {
                        desired[i]=desired[i].max(fonts.measure(FontFace::UiRegular,text,16.0)+12.0);
                    }}
                    let sum: f32=desired.iter().sum();
                    let available=content_width.saturating_sub(40).max(1);
                    let mut widths=Vec::new();let mut used=0;
                    for (i,w) in desired.iter().enumerate() {
                        let w=if i+1==count {available.saturating_sub(used)}
                            else {(*w/sum*available as f32) as u32};
                        widths.push(w);used+=w;
                    }
                    let mut table_rows=Vec::new();let mut row_y=0;
                    for (header,row) in core::iter::once((true,headers)).filter(|(_,r)|!r.is_empty())
                        .chain(rows.iter().map(|r|(false,r))) {
                        let mut cells=Vec::new();let mut x=0;let mut row_h=0;
                        for (i,&w) in widths.iter().enumerate() {
                            let mut span=StyledSpan::plain(row.get(i).map_or("",String::as_str));
                            span.bold=header;span.color=Some(if header{theme::ACCENT_CYAN}else{theme::TEXT_PRIMARY});
                            let lines=lines_for(fonts,&[span],16.0,w.saturating_sub(12).max(1) as f32,false);
                            row_h=row_h.max(lines_height(&lines) as u32+12);
                            cells.push(TableCellLayout{lines,x,width:w});x+=w as i32;
                        }
                        table_rows.push(TableRowLayout{y_rel:row_y,height:row_h,cells,is_header:header});
                        row_y+=row_h as i32;
                    }
                    b.table_rows=Some(table_rows);b.table_width=used;
                    b.left_margin=16;b.height=row_y+12;gap=8;
                }
                DocNode::Button{text,node_id}=>{
                    let mut span=StyledSpan::plain(text.trim());span.bold=true;
                    b.lines=lines_for(fonts,&[span],16.0,width,false);
                    b.table_width=b.lines.iter().map(|l|l.width as u32+16).max().unwrap_or(16);
                    b.left_margin=8;b.bg_color=Some(theme::ACCENT_BLUE);
                    b.border_color=Some(theme::BORDER);b.node_id=*node_id;gap=8;
                }
            }
            if b.height==0 {b.height=lines_height(&b.lines)+8;}
            y+=b.height+gap;self.layout_boxes.push(b);
        }
        self.total_height=y+32;
    }

    pub fn render_viewport(&self,fonts:&mut TextFonts,surf:&mut ViSurface,
        view_x:i32,view_y:i32,view_w:u32,view_h:u32,scroll_y:i32) {
        if let Some(web)=&self.web_layout {web.render(fonts,surf,(view_x,view_y,view_w,view_h),scroll_y);return;}
        let clip=(view_x,view_y,view_x+view_w as i32,view_y+view_h as i32);
        for b in &self.layout_boxes {
            let y=view_y+b.y_offset-scroll_y;
            if y+b.height<=clip.1 || y>=clip.3 {continue;}
            let text_x=view_x+16+b.left_margin;
            if let Some(bg)=b.bg_color {
                let w=if b.node_id.is_some(){b.table_width}else{view_w.saturating_sub(b.left_margin as u32+20)};
                draw::clipped_rect(surf,text_x-8,y,w,b.height as u32,bg,clip);
                if let Some(border)=b.border_color {
                    draw::clipped_rect(surf,text_x-8,y,w,1,border,clip);
                    draw::clipped_rect(surf,text_x-8,y+b.height-1,w,1,border,clip);
                }
            } else if let Some(border)=b.border_color {
                draw::clipped_rect(surf,text_x-8,y,2,b.height as u32,border,clip);
            }
            if b.is_rule {
                draw::clipped_rect(surf,view_x+10,y+4,view_w.saturating_sub(30),1,theme::BORDER,clip);continue;
            }
            if let Some((iw,ih,pixels))=&b.image {
                draw::draw_image_clipped(surf,view_x+b.left_margin,y+8,
                    (*iw).min(view_w.saturating_sub(32).max(1)),(b.height-16) as u32,
                    pixels,(*iw*4) as usize,*ih,clip);continue;
            }
            if let Some(rows)=&b.table_rows {
                let x=view_x+b.left_margin;
                for row in rows {
                    let ry=y+row.y_rel;
                    if row.is_header {draw::clipped_rect(surf,x,ry,b.table_width,row.height,theme::BG_INPUT,clip);}
                    draw::clipped_rect(surf,x,ry+row.height as i32-1,b.table_width,1,theme::BORDER,clip);
                    for cell in &row.cells {
                        draw::clipped_rect(surf,x+cell.x+cell.width as i32-1,ry,1,row.height,theme::BORDER,clip);
                        let cell_clip=((x+cell.x).max(clip.0),ry.max(clip.1),
                            (x+cell.x+cell.width as i32).min(clip.2),(ry+row.height as i32).min(clip.3));
                        paint_lines(fonts,surf,&cell.lines,(x+cell.x+6) as f32,(ry+6) as f32,theme::TEXT_PRIMARY,cell_clip);
                    }
                }
                continue;
            }
            if b.bg_color.is_none() {
                let mut line_y=y+4;
                for line in &b.lines {
                    for run in &line.runs {
                        if run.span.code {
                            draw::clipped_rect(surf,text_x+run.x as i32-2,line_y,
                                crate::parser::web::ceil_pixel(run.width)+4,line.height,
                                theme::BG_INPUT,clip);
                        }
                    }
                    line_y+=line.height as i32;
                }
            }
            paint_lines(fonts,surf,&b.lines,text_x as f32,(y+4) as f32,theme::TEXT_PRIMARY,clip);
        }
    }

    pub fn hit_test_link(&self,click_x:i32,click_y:i32,view_x:i32,view_y:i32,scroll_y:i32)->Option<String> {
        if let Some(web)=&self.web_layout {return web.hit_link(click_x-view_x,click_y-view_y+scroll_y);}
        for b in &self.layout_boxes {
            let mut y=view_y+b.y_offset-scroll_y+4;
            for line in &b.lines {
                if click_y>=y && click_y<y+line.height as i32 {
                    for run in &line.runs {
                        let x=(view_x+16+b.left_margin) as f32+run.x;
                        if (click_x as f32)>=x && (click_x as f32)<x+run.width {
                            if let Some(link)=&run.span.link{return Some(link.clone());}
                        }
                    }
                }
                y+=line.height as i32;
            }
        }
        None
    }
    pub fn hit_test_node(&self,click_x:i32,click_y:i32,view_x:i32,view_y:i32,scroll_y:i32)->Option<dom_arena::NodeId> {
        if let Some(web)=&self.web_layout {return web.hit_node(click_x-view_x,click_y-view_y+scroll_y);}
        for b in &self.layout_boxes {
            let y=view_y+b.y_offset-scroll_y;
            let x=view_x+8+b.left_margin;
            if click_y>=y && click_y<y+b.height && click_x>=x && click_x<x+b.table_width as i32 {
                if let Some(id)=b.node_id{return Some(id);}
            }
        }
        None
    }
}
