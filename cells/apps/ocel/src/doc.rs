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
    DomImage {
        node_id: dom_arena::NodeId,
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
    Input {
        input_type: String,
        name: String,
        value: String,
        placeholder: String,
        checked: bool,
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
pub struct FormInputLayout {
    pub input_type: String,
    pub name: String,
    pub value: String,
    pub placeholder: String,
    pub checked: bool,
    pub is_focused: bool,
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
    pub input_data: Option<FormInputLayout>,
    search_highlights: Vec<(i32,u32,bool)>,
}
impl LayoutBox {
    fn new(y: i32) -> Self {
        Self { y_offset:y, height:0, lines:Vec::new(), bg_color:None,
            border_color:None, is_rule:false, left_margin:0, image:None,
            table_rows:None, table_width:0, node_id:None, input_data: None, search_highlights:Vec::new() }
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
    reader_nodes: Option<Vec<DocNode>>,
    zoom: u16,
    reader_column: u32,
    search_query: String,
    active_match: Option<usize>,
    focused_input: Option<dom_arena::NodeId>,
    pub last_error: Option<String>,
    memory_budget: usize,
}

#[derive(Clone, Debug)]
pub struct TocEntry { pub title:String, pub level:u8, pub y:i32, pub anchor:String }

pub fn fold_text(text:&str)->String {
    ostd::typography::normalized_chars(text).flat_map(char::to_lowercase).collect()
}

pub(crate) fn scale_lines(lines:&mut [TextLine],scale:f32) {
    for line in lines {line.height=(line.height as f32*scale).max(1.0) as u32;line.width*=scale;line.baseline*=scale;
        for run in &mut line.runs {run.px*=scale;run.x*=scale;run.width*=scale;run.line_height*=scale;}}
}

fn scale_box(b:&mut LayoutBox,scale:f32) {
    b.y_offset=(b.y_offset as f32*scale) as i32;b.height=(b.height as f32*scale) as i32;
    b.left_margin=(b.left_margin as f32*scale) as i32;b.table_width=(b.table_width as f32*scale) as u32;
    scale_lines(&mut b.lines,scale);
    if let Some(rows)=&mut b.table_rows {for row in rows {
        row.y_rel=(row.y_rel as f32*scale) as i32;row.height=(row.height as f32*scale) as u32;
        for cell in &mut row.cells {cell.x=(cell.x as f32*scale) as i32;cell.width=(cell.width as f32*scale) as u32;scale_lines(&mut cell.lines,scale);}
    }}
}

fn validate_arena(arena:&dom_arena::DocumentArena,external:&[(dom_arena::NodeId,String)])->Result<(),String> {
    use dom_arena::NodeData;
    if arena.nodes.len()>crate::budget::MAX_DOM_NODES {return Err(String::from("DOM exceeds node budget"));}
    let mut bytes=0usize;
    for node in &arena.nodes {
        let mut current=Some(node.id);let mut depth=0usize;
        while let Some(id)=current {
            depth+=1;if depth>crate::budget::MAX_DOM_DEPTH+1 {return Err(String::from("DOM exceeds depth budget or contains a cycle"));}
            current=arena.get(id).and_then(|node|node.parent);
        }
        match &node.data {
            NodeData::Text(text)|NodeData::Comment(text)=>bytes=bytes.saturating_add(text.len()),
            NodeData::Element{tag,attributes}=>{bytes=bytes.saturating_add(tag.len());for (name,value) in attributes {bytes=bytes.saturating_add(name.len()+value.len());}},
            _=>{}
        }
        if bytes>crate::budget::MAX_TEXT_BYTES {return Err(String::from("DOM exceeds text byte budget"));}
    }
    let mut css=String::new();
    for node in &arena.nodes {
        if let Some(style)=node.get_attribute("style") {crate::budget::validate_css(&format!("x{{{}}}",style))?;}
        if node.tag()==Some("style") {
            let text=arena.get_text_content(node.id);
            if css.len().saturating_add(text.len())>crate::budget::MAX_CSS_BYTES{return Err(String::from("CSS exceeds byte budget"));}
            css.push_str(&text);
        }
    }
    for (_,text) in external {
        if css.len().saturating_add(text.len())>crate::budget::MAX_CSS_BYTES{return Err(String::from("CSS exceeds byte budget"));}
        css.push_str(text);
    }
    crate::budget::validate_css(&css)
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
            arena:None, external_stylesheets:Vec::new(), web_layout:None,
            reader_nodes:None, zoom:100, reader_column:720, search_query:String::new(),
            active_match:None, focused_input:None, last_error:None, memory_budget:crate::budget::MAX_RESIDENT_BYTES }
    }
    pub fn clear(&mut self) {
        let memory_budget=self.memory_budget;
        *self = Self::new();
        self.memory_budget=memory_budget;
    }
    pub fn search(&self,query:&str)->Vec<i32> {
        if let Some(web)=&self.web_layout {return web.search(query);}
        let query=fold_text(query.trim());
        if query.is_empty(){return Vec::new();}
        let mut matches=Vec::new();let mut text=String::new();
        for b in &self.layout_boxes {
            let mut found=false;
            for line in &b.lines {
                text.clear();for run in &line.runs {text.push_str(&run.span.text);}
                found|=fold_text(&text).contains(&query);
            }
            if let Some(rows)=&b.table_rows {for row in rows {for cell in &row.cells {for line in &cell.lines {
                text.clear();for run in &line.runs {text.push_str(&run.span.text);}
                found|=fold_text(&text).contains(&query);
            }}}}
            if found {matches.push(b.y_offset);}
        }
        matches
    }
    pub fn relayout_from_arena(&mut self,fonts:&mut TextFonts,width:u32,height:u32)->bool {
        self.last_error=None;
        if self.is_reader_mode() { self.compute_layout(fonts,width); return self.last_error.is_none(); }
        if let Err(error)=self.reserve_layout(width) {self.last_error=Some(error);return false;}
        let Some(arena)=&self.arena else{return false;};
        let scale=self.zoom as f32/100.0;
        if let Err(error)=validate_arena(arena,&self.external_stylesheets) { self.last_error=Some(error); return false; }
        match crate::parser::web::layout_with_nodes_zoom(fonts,arena,&self.external_stylesheets,
            (width as f32/scale).max(1.0) as u32,(height as f32/scale).max(1.0) as u32,&self.nodes,scale) {
            Ok(mut layout)=>{
                layout.scale(scale);
                self.layout_boxes.clear();self.total_height=layout.height;
                self.web_layout=Some(layout); self.focus_input(self.focused_input); self.refresh_highlights(); true
            }
            Err(_)=>{self.last_error=Some(String::from("HTML layout failed"));false}
        }
    }
    pub fn compute_layout(&mut self,fonts:&mut TextFonts,content_width:u32) {
        if self.arena.is_some() && !self.is_reader_mode(){self.relayout_from_arena(fonts,content_width,600);return;}
        self.last_error=None;
        if let Err(error)=self.reserve_layout(content_width){self.last_error=Some(error);return;}
        self.web_layout=None;self.layout_boxes.clear();
        let scale=self.zoom as f32/100.0;
        let content_width=if self.is_reader_mode(){content_width.min(self.reader_column)}else{content_width};
        let logical_width=(content_width as f32/scale).max(1.0) as u32;
        let mut y=16;
        let width=logical_width.saturating_sub(40).max(1) as f32;
        let nodes=self.reader_nodes.as_ref().unwrap_or(&self.nodes);
        for node in nodes {
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
                    let mut list=alloc::vec![StyledSpan::plain(&if *bullet=='\0'{String::new()}else{format!("{} ",bullet)})];
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
                DocNode::Image{width:iw,height:ih,pixels} | DocNode::DomImage{width:iw,height:ih,pixels,..}=>{
                    let display_w=(*iw).min(logical_width.saturating_sub(32).max(1));
                    let display_h=((*ih as u64*display_w as u64)/(*iw).max(1) as u64).max(1) as u32;
                    b.height=display_h as i32+16;b.left_margin=16;
                    b.table_width=display_w;
                    b.image=Some((*iw,*ih,pixels.clone()));
                }
                DocNode::Table{headers,rows}=>{
                    let count=headers.len().max(rows.iter().map(|r|r.len()).max().unwrap_or(0));
                    if count==0{b.height=8;y+=8;self.layout_boxes.push(b);continue;}
                    let mut desired=alloc::vec![12.0f32;count];
                    for (i,text) in headers.iter().enumerate() {
                        desired[i]=desired[i].max(fonts.measure(FontFace::UiSemibold,text,16.0)+12.0);
                    }
                    for row in rows {for (i,text) in row.iter().enumerate() {
                        desired[i]=desired[i].max(fonts.measure(FontFace::UiRegular,text,16.0)+12.0);
                    }}
                    let sum: f32=desired.iter().sum();
                    let available=logical_width.saturating_sub(40).max(1);
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
                DocNode::Input { input_type, name, value, placeholder, checked, node_id } => {
                    let inp_w = match input_type.as_str() {
                        "checkbox" => 20,
                        "submit" | "button" => {
                            let label = if value.is_empty() { "Submit" } else { value.as_str() };
                            (fonts.measure(FontFace::UiSemibold, label, 14.0) as u32 + 24).max(64)
                        }
                        _ => (width - 16.0).clamp(120.0, 360.0) as u32,
                    };
                    let inp_h = if input_type == "checkbox" { 20 } else { 28 };
                    b.height = inp_h as i32 + 8;
                    b.table_width = inp_w;
                    b.left_margin = 8;
                    b.node_id = *node_id;
                    b.input_data = Some(FormInputLayout {
                        input_type: input_type.clone(),
                        name: name.clone(),
                        value: value.clone(),
                        placeholder: placeholder.clone(),
                        checked: *checked,
                        is_focused: false,
                    });
                    gap = 6;
                }
            }
            if b.height==0 {b.height=lines_height(&b.lines)+8;}
            y+=b.height+gap;self.layout_boxes.push(b);
        }
        self.total_height=((y+32) as f32*scale) as i32;
        for b in &mut self.layout_boxes { scale_box(b,scale); }
        self.refresh_highlights();
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
                    b.table_width,((*ih as u64*b.table_width as u64)/(*iw).max(1) as u64).max(1) as u32,
                    pixels,(*iw*4) as usize,*ih,clip);continue;
            }
            if let Some(input) = &b.input_data {
                let inp_x = text_x;
                let inp_y = y + 2;
                if input.input_type == "checkbox" {
                    draw::clipped_rect(surf, inp_x, inp_y, 18, 18, theme::BG_INPUT, clip);
                    draw::clipped_rect(surf, inp_x, inp_y, 18, 1, theme::BORDER, clip);
                    draw::clipped_rect(surf, inp_x, inp_y + 17, 18, 1, theme::BORDER, clip);
                    draw::clipped_rect(surf, inp_x, inp_y, 1, 18, theme::BORDER, clip);
                    draw::clipped_rect(surf, inp_x + 17, inp_y, 1, 18, theme::BORDER, clip);
                    if input.checked {
                        draw::clipped_rect(surf, inp_x + 3, inp_y + 3, 12, 12, theme::ACCENT_CYAN, clip);
                    }
                } else if input.input_type == "submit" || input.input_type == "button" {
                    let label = if input.value.is_empty() { "Submit" } else { input.value.as_str() };
                    draw::clipped_rect(surf, inp_x, inp_y, b.table_width, 28, theme::ACCENT_BLUE, clip);
                    let mut span = StyledSpan::plain(label);
                    span.bold = true;
                    let line = lines_for(fonts, &[span], 14.0, b.table_width as f32, false);
                    paint_lines(fonts, surf, &line, (inp_x + 8) as f32, (inp_y + 5) as f32, Color::rgb(17, 17, 27), clip);
                } else {
                    let inp_w = b.table_width.max(120);
                    let border_col = if input.is_focused { theme::ACCENT_CYAN } else { theme::BORDER };
                    draw::clipped_rect(surf, inp_x, inp_y, inp_w, 28, theme::BG_INPUT, clip);
                    draw::clipped_rect(surf, inp_x, inp_y, inp_w, 1, border_col, clip);
                    draw::clipped_rect(surf, inp_x, inp_y + 27, inp_w, 1, border_col, clip);
                    draw::clipped_rect(surf, inp_x, inp_y, 1, 28, border_col, clip);
                    draw::clipped_rect(surf, inp_x + inp_w as i32 - 1, inp_y, 1, 28, border_col, clip);
                    let display_text = if !input.value.is_empty() { &input.value } else { &input.placeholder };
                    let col = if !input.value.is_empty() { theme::TEXT_PRIMARY } else { theme::TEXT_MUTED };
                    let span = StyledSpan::plain(display_text);
                    let line = lines_for(fonts, &[span], 14.0, (inp_w.saturating_sub(12)) as f32, false);
                    paint_lines(fonts, surf, &line, (inp_x + 6) as f32, (inp_y + 5) as f32, col, clip);
                }
                continue;
            }
            if let Some(rows)=&b.table_rows {
                let x=view_x+b.left_margin;
                for row in rows {
                    let ry=y+row.y_rel;
                    if row.is_header {draw::clipped_rect(surf,x,ry,b.table_width,row.height,theme::BG_INPUT,clip);}
                    for &(offset,height,active) in &b.search_highlights {
                        if offset>=row.y_rel&&offset<row.y_rel+row.height as i32 {draw::clipped_rect(surf,x,y+offset,b.table_width,height,if active{Color::rgb(245,158,11)}else{Color::rgb(100,90,30)},clip);}
                    }
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
            for &(offset,height,active) in &b.search_highlights {
                let width=b.lines.iter().map(|line|line.width as u32).max().unwrap_or(1);
                draw::clipped_rect(surf,text_x,y+offset,width,height,if active{Color::rgb(245,158,11)}else{Color::rgb(100,90,30)},clip);
            }
            paint_lines(fonts,surf,&b.lines,text_x as f32,(y+4) as f32,theme::TEXT_PRIMARY,clip);
        }
    }

    pub fn hit_test_link(&self,click_x:i32,click_y:i32,view_x:i32,view_y:i32,scroll_y:i32)->Option<String> {
        if let Some(web)=&self.web_layout {
            if let Some(link) = web.hit_link(click_x-view_x,click_y-view_y+scroll_y) {
                return Some(link);
            }
            if let Some(arena) = &self.arena {
                if let Some(node_id) = web.hit_node(click_x-view_x, click_y-view_y+scroll_y) {
                    let mut curr = Some(node_id);
                    while let Some(id) = curr {
                        if let Some(node) = arena.get(id) {
                            if node.tag() == Some("a") {
                                if let Some(href) = node.get_attribute("href") {
                                    if !href.is_empty() {
                                        return Some(String::from(href));
                                    }
                                }
                            }
                            curr = node.parent;
                        } else {
                            break;
                        }
                    }
                }
            }
            return None;
        }
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
    pub fn hit_test_input(&self, click_x: i32, click_y: i32, view_x: i32, view_y: i32, scroll_y: i32) -> Option<usize> {
        for (idx, b) in self.layout_boxes.iter().enumerate() {
            if b.input_data.is_some() {
                let y = view_y + b.y_offset - scroll_y;
                let x = view_x + 16 + b.left_margin;
                if click_y >= y && click_y < y + b.height && click_x >= x && click_x < x + b.table_width as i32 {
                    return Some(idx);
                }
            }
        }
        None
    }
}

impl Document {
    pub fn is_reader_mode(&self)->bool {self.reader_nodes.is_some()}
    pub fn toggle_reader(&mut self,fonts:&mut TextFonts,width:u32,height:u32,base:&str,title:&str)->Result<bool,String> {
        self.reserve_layout(width)?;
        let previous=self.reader_nodes.take();
        if previous.is_none() {
            self.reserve_layout(width)?;
            let arena=self.arena.as_ref().ok_or_else(||String::from("Reader mode requires an HTML document"))?;
            validate_arena(arena,&self.external_stylesheets)?;
            self.reader_nodes=Some(crate::parser::readability::extract_article_cached(arena,base,title,&self.nodes).nodes);
        }
        self.focus_input(None);
        if self.is_reader_mode(){self.compute_layout(fonts,width);}else{self.relayout_from_arena(fonts,width,height);}
        if let Some(error)=self.last_error.clone(){self.reader_nodes=previous;return Err(error);}
        Ok(self.is_reader_mode())
    }
    pub fn zoom_percent(&self)->u16 {self.zoom}
    pub fn set_zoom(&mut self,percent:u16,fonts:&mut TextFonts,width:u32,height:u32) {
        let previous=self.zoom;
        self.zoom=percent.clamp(50,200);
        if self.arena.is_some(){self.relayout_from_arena(fonts,width,height);}else{self.compute_layout(fonts,width);}
        if self.last_error.is_some(){self.zoom=previous;}
    }
    pub fn reader_width(&self)->u32 {self.reader_column}
    pub fn set_reader_width(&mut self,column:u32,fonts:&mut TextFonts,width:u32,height:u32) {
        let previous=self.reader_column;
        self.reader_column=column.clamp(320,1200);
        if self.is_reader_mode(){self.relayout_from_arena(fonts,width,height);}
        if self.last_error.is_some(){self.reader_column=previous;}
    }
    pub fn set_search_highlight(&mut self,query:&str,active:Option<usize>) {
        self.search_query=fold_text(query.trim());self.active_match=active;
        self.refresh_highlights();
    }
    pub fn focus_input(&mut self,id:Option<dom_arena::NodeId>) {
        self.focused_input=id;
        for b in &mut self.layout_boxes {if let Some(input)=&mut b.input_data {input.is_focused=b.node_id==id;}}
        if let Some(web)=&mut self.web_layout {for b in &mut web.boxes {if let Some(input)=&mut b.input_data {input.is_focused=b.node_id==id;}}}
    }
    pub fn input(&self,id:dom_arena::NodeId)->Option<&FormInputLayout> {
        if let Some(web)=&self.web_layout {return web.boxes.iter().find(|b|b.node_id==Some(id)).and_then(|b|b.input_data.as_ref());}
        self.layout_boxes.iter().find(|b|b.node_id==Some(id)).and_then(|b|b.input_data.as_ref())
    }
    fn update_control(&mut self,id:dom_arena::NodeId,value:Option<String>,checked:Option<bool>)->bool {
        if self.input(id).is_none(){return false;}
        let apply=|input:&mut FormInputLayout|{if let Some(value)=&value {input.value=value.clone();}if let Some(checked)=checked {input.checked=checked;}};
        for b in &mut self.layout_boxes {if b.node_id==Some(id){if let Some(input)=&mut b.input_data {apply(input);}}}
        if let Some(web)=&mut self.web_layout {for b in &mut web.boxes {if b.node_id==Some(id){if let Some(input)=&mut b.input_data {apply(input);}}}}
        if let Some(arena)=&mut self.arena {if let Some(node)=arena.get_mut(id) {if let dom_arena::NodeData::Element{attributes,..}=&mut node.data {
            if let Some(value)=value {
                if let Some((_,old))=attributes.iter_mut().find(|(name,_)|name=="value"){*old=value;}else{attributes.push((String::from("value"),value));}
            }
            if let Some(checked)=checked {attributes.retain(|(name,_)|name!="checked");if checked {attributes.push((String::from("checked"),String::new()));}}
        }}}
        true
    }
    pub fn edit_input(&mut self,id:dom_arena::NodeId,ch:char)->bool {
        let Some(input)=self.input(id) else{return false;};
        if !matches!(input.input_type.as_str(),"text"|"textarea"|"password"|"email"|"search"|"url"|"tel"){return false;}
        if self.arena.as_ref().and_then(|a|a.get(id)).is_some_and(|node|node.get_attribute("disabled").is_some()||node.get_attribute("readonly").is_some()){return false;}
        let mut value=input.value.clone();
        if let Some(arena)=&self.arena {
            let bytes:usize=arena.nodes.iter().map(|node|match &node.data {
                dom_arena::NodeData::Text(text)|dom_arena::NodeData::Comment(text)=>text.len(),
                dom_arena::NodeData::Element{tag,attributes}=>tag.len()+attributes.iter().map(|(name,value)|name.len()+value.len()).sum::<usize>(),_=>0,
            }).sum();
            if bytes.saturating_add(value.len()+ch.len_utf8()+5)>crate::budget::MAX_TEXT_BYTES{return false;}
        }
        if ch=='\u{8}' {value.pop();}else if !ch.is_control() || (ch=='\n'&&input.input_type=="textarea") {
            if value.len()+ch.len_utf8()>crate::budget::MAX_TEXT_BYTES{return false;} value.push(ch);
        }else{return false;}
        self.update_control(id,Some(value),None)
    }
    pub fn toggle_input(&mut self,id:dom_arena::NodeId)->bool {
        let Some(input)=self.input(id)else{return false;};
        if input.input_type!="checkbox"||self.arena.as_ref().and_then(|a|a.get(id)).is_some_and(|node|node.get_attribute("disabled").is_some()){return false;}
        let checked=!input.checked;
        self.update_control(id,None,Some(checked))
    }
    pub fn hit_test_input_node(&self,x:i32,y:i32,vx:i32,vy:i32,scroll:i32)->Option<dom_arena::NodeId> {
        if let Some(web)=&self.web_layout {return web.boxes.iter().rev().find(|b|b.input_data.is_some()&&x-vx>=b.x&&x-vx<b.x+b.width as i32&&y-vy+scroll>=b.y&&y-vy+scroll<b.y+b.height as i32).and_then(|b|b.node_id);}
        self.hit_test_input(x,y,vx,vy,scroll).and_then(|index|self.layout_boxes[index].node_id)
    }
}

impl Document {
    pub fn form_owner(&self, control: dom_arena::NodeId) -> Option<dom_arena::NodeId> {
        self.arena.as_ref().and_then(|arena| form_owner(arena, control))
    }
    pub fn form_url(&self,submit:dom_arena::NodeId,base:&str)->Result<String,String> {
        let arena=self.arena.as_ref().ok_or_else(||String::from("Document has no HTML form"))?;
        let owner=form_owner(arena,submit).ok_or_else(||String::from("Control does not belong to a form"))?;
        let form=arena.get(owner).ok_or_else(||String::from("Form no longer exists"))?;
        let submit_node=arena.get(submit).ok_or_else(||String::from("Control no longer exists"))?;
        if submit_node.get_attribute("disabled").is_some(){return Err(String::from("Disabled control cannot submit a form"));}
        if submit_node.tag()==Some("button")&&!submit_node.get_attribute("type").unwrap_or("submit").eq_ignore_ascii_case("submit"){return Err(String::from("Button is not a submit control"));}
        let method=submit_node.get_attribute("formmethod").or_else(||form.get_attribute("method")).unwrap_or("get");
        if !method.eq_ignore_ascii_case("get"){return Err(format!("Unsupported form method: {}",method));}
        let enctype=submit_node.get_attribute("formenctype").or_else(||form.get_attribute("enctype")).unwrap_or("application/x-www-form-urlencoded");
        if !enctype.eq_ignore_ascii_case("application/x-www-form-urlencoded"){return Err(format!("Unsupported form encoding: {}",enctype));}
        let action=submit_node.get_attribute("formaction").or_else(||form.get_attribute("action")).unwrap_or("");
        let action=if action.is_empty(){String::from(base)}else{crate::resources::resolve(base,action)?};
        let (target,fragment)=action.split_once('#').map_or((action.as_str(),None),|(path,fragment)|(path,Some(fragment)));
        let target=target.split('?').next().unwrap_or(target);
        let mut query=String::new();
        for node in &arena.nodes {
            if !matches!(node.tag(),Some("input"|"textarea"|"button"|"select"))||form_owner(arena,node.id)!=Some(owner)||node.get_attribute("disabled").is_some(){continue;}
            if node.tag()==Some("select"){return Err(String::from("Unsupported form control type: select"));}
            let name=node.get_attribute("name").unwrap_or("");if name.is_empty(){continue;}
            let kind=if node.tag()==Some("button"){node.get_attribute("type").unwrap_or("submit")}else{node.get_attribute("type").unwrap_or("text")}.to_ascii_lowercase();
            let kind=kind.as_str();
            if matches!(kind,"reset"|"button")||(kind=="submit"&&node.id!=submit){continue;}
            if !matches!(kind,"text"|"textarea"|"password"|"email"|"search"|"url"|"tel"|"hidden"|"checkbox"|"submit"){return Err(format!("Unsupported form control type: {}",kind));}
            let input=self.input(node.id);
            let checked=input.map_or(node.get_attribute("checked").is_some(),|input|input.checked);
            if kind=="checkbox"&&!checked{continue;}
            let text;
            let value=if node.tag()==Some("button"){node.get_attribute("value").unwrap_or("")}
                else if let Some(input)=input{input.value.as_str()}
                else if node.tag()==Some("textarea"){text=arena.get_text_content(node.id);text.as_str()}
                else{node.get_attribute("value").unwrap_or(if kind=="checkbox"{"on"}else{""})};
            let value=if kind=="checkbox"&&value.is_empty()&&node.get_attribute("value").is_none(){"on"}else{value};
            if !query.is_empty(){query.push('&');}encode_form(name,&mut query);query.push('=');encode_form(value,&mut query);
        }
        let mut result=format!("{}?{}",target,query);
        if let Some(fragment)=fragment{result.push('#');result.push_str(fragment);}
        Ok(result)
    }
    pub fn toc(&self)->Vec<TocEntry> {
        let mut result:Vec<TocEntry>=Vec::new();
        if let (Some(arena),Some(web))=(&self.arena,&self.web_layout) {
            for node in &arena.nodes {if let Some(tag)=node.tag() {if tag.len()==2&&tag.starts_with('h') {
                if let Ok(level @ 1..=6)=tag[1..].parse::<u8>() {
                    let Some(b)=web.boxes.iter().find(|b|b.node_id==Some(node.id)&&b.height>0)else{continue;};
                    let title=arena.get_text_content(node.id);
                    let anchor=node.get_attribute("id").map(String::from).unwrap_or_else(||slug(&title));
                    result.push(TocEntry{title,level,y:b.y,anchor});
                }
            }}}
        }else{
            let nodes=self.reader_nodes.as_ref().unwrap_or(&self.nodes);
            for (node,b) in nodes.iter().zip(&self.layout_boxes) {if let DocNode::Heading{level,text}=node {
                let base=self.arena.as_ref().and_then(|arena|arena.nodes.iter().find(|node|node.tag().is_some_and(|tag|tag.len()==2&&tag.starts_with('h'))&&arena.get_text_content(node.id).trim()==text.trim()).and_then(|node|node.get_attribute("id")).map(String::from)).unwrap_or_else(||slug(text));
                let mut anchor=base.clone();let mut count=1;
                while result.iter().any(|entry|entry.anchor==anchor){anchor=format!("{}-{}",base,count);count+=1;}
                result.push(TocEntry{title:text.clone(),level:*level,y:b.y_offset,anchor});
            }}
        }
        result
    }
    pub fn anchor_y(&self,fragment:&str)->Option<i32> {
        let fragment=decode_fragment(fragment.trim_start_matches('#'))?;
        if fragment.is_empty(){return Some(0);}
        if let (Some(arena),Some(web))=(&self.arena,&self.web_layout) {
            if let Some(node)=arena.nodes.iter().find(|node|node.get_attribute("id")==Some(fragment.as_str())||node.get_attribute("name")==Some(fragment.as_str())) {
                let mut id=Some(node.id);
                while let Some(current)=id {
                    if let Some(b)=web.boxes.iter().find(|b|b.node_id==Some(current)||b.lines.iter().any(|line|line.runs.iter().any(|run|run.node_id==Some(current)))){return Some(b.y);}
                    id=arena.get(current).and_then(|node|node.parent);
                }
            }
        }
        self.toc().into_iter().find(|entry|entry.anchor==fragment).map(|entry|entry.y)
    }
    pub fn resident_bytes(&self)->usize {
        let mut bytes=core::mem::size_of::<Self>();
        let nodes_bytes=|nodes:&Vec<DocNode>|->usize{(nodes.capacity()-nodes.len())*core::mem::size_of::<DocNode>()+nodes.iter().map(|node|core::mem::size_of::<DocNode>()+match node {
            DocNode::Heading{text,..}|DocNode::Button{text,..}=>text.capacity(),
            DocNode::Paragraph{spans}|DocNode::ListItem{spans,..}=>spans.iter().map(span_bytes).sum(),
            DocNode::Image{..}|DocNode::DomImage{..}=>0,
            DocNode::CodeBlock{lang,lines}=>lang.capacity()+lines.iter().map(String::capacity).sum::<usize>(),
            DocNode::RawLines{lines}|DocNode::Blockquote{lines}=>lines.iter().map(String::capacity).sum(),
            DocNode::Table{headers,rows}=>headers.iter().chain(rows.iter().flatten()).map(String::capacity).sum(),
            DocNode::Input{input_type,name,value,placeholder,..}=>input_type.capacity()+name.capacity()+value.capacity()+placeholder.capacity(),
            DocNode::Rule=>0,
        }).sum::<usize>()};
        bytes+=nodes_bytes(&self.nodes);
        if let Some(nodes)=&self.reader_nodes {bytes+=nodes_bytes(nodes);}
        let mut image_allocations=Vec::new();
        for node in self.nodes.iter().chain(self.reader_nodes.iter().flat_map(|nodes|nodes.iter())) {
            if let DocNode::Image{pixels,..}|DocNode::DomImage{pixels,..}=node {
                let pointer=Rc::as_ptr(pixels);
                if !image_allocations.contains(&pointer){image_allocations.push(pointer);bytes+=pixels.capacity();}
            }
        }
        if let Some(arena)=&self.arena {bytes+=arena.nodes.capacity()*core::mem::size_of::<dom_arena::DomNode>();for node in &arena.nodes {bytes+=match &node.data {
            dom_arena::NodeData::Text(text)|dom_arena::NodeData::Comment(text)=>text.capacity(),
            dom_arena::NodeData::Element{tag,attributes}=>tag.capacity()+attributes.capacity()*core::mem::size_of::<(String,String)>()+attributes.iter().map(|(name,value)|name.capacity()+value.capacity()).sum::<usize>(),_=>0,
        };}}
        bytes+=self.external_stylesheets.iter().map(|(_,text)|text.capacity()).sum::<usize>();
        for b in &self.layout_boxes {bytes+=core::mem::size_of::<LayoutBox>()+lines_bytes(&b.lines);if let Some(input)=&b.input_data{bytes+=input_bytes(input);}if let Some(rows)=&b.table_rows{for row in rows {bytes+=core::mem::size_of::<TableRowLayout>();for cell in &row.cells{bytes+=core::mem::size_of::<TableCellLayout>()+lines_bytes(&cell.lines);}}}}
        if let Some(web)=&self.web_layout {for b in &web.boxes {bytes+=core::mem::size_of::<crate::parser::web::WebBox>()+lines_bytes(&b.lines);if let Some(input)=&b.input_data{bytes+=input_bytes(input);}}}
        bytes+=(self.layout_boxes.capacity()-self.layout_boxes.len())*core::mem::size_of::<LayoutBox>();
        bytes+=self.external_stylesheets.capacity()*core::mem::size_of::<(dom_arena::NodeId,String)>();
        if let Some(web)=&self.web_layout {bytes+=(web.boxes.capacity()-web.boxes.len())*core::mem::size_of::<crate::parser::web::WebBox>();}
        bytes+self.search_query.capacity()
    }
}

fn span_bytes(span:&StyledSpan)->usize {core::mem::size_of::<StyledSpan>()+span.text.capacity()+span.link.as_ref().map_or(0,String::capacity)}
fn input_bytes(input:&FormInputLayout)->usize {input.input_type.capacity()+input.name.capacity()+input.value.capacity()+input.placeholder.capacity()}
fn lines_bytes(lines:&Vec<TextLine>)->usize {lines.capacity()*core::mem::size_of::<TextLine>()+lines.iter().map(|line|line.runs.capacity()*core::mem::size_of::<TextRun>()+line.runs.iter().map(|run|span_bytes(&run.span)).sum::<usize>()).sum::<usize>()}
fn slug(text:&str)->String {
    let mut result=String::new();let mut separator=false;
    for c in fold_text(text).chars() {if c.is_alphanumeric()||c=='_' {if separator&&!result.is_empty(){result.push('-');}result.push(c);separator=false;}else if c.is_whitespace()||c=='-'{separator=true;}}
    result
}
fn form_owner(arena:&dom_arena::DocumentArena,id:dom_arena::NodeId)->Option<dom_arena::NodeId> {
    let node=arena.get(id)?;
    if let Some(owner)=node.get_attribute("form") {return arena.nodes.iter().find(|node|node.tag()==Some("form")&&node.get_attribute("id")==Some(owner)).map(|node|node.id);}
    let mut parent=node.parent;let mut depth=0;
    while let Some(id)=parent {depth+=1;if depth>crate::budget::MAX_DOM_DEPTH{return None;}let node=arena.get(id)?;if node.tag()==Some("form"){return Some(id);}parent=node.parent;}
    None
}
fn encode_form(text:&str,out:&mut String) {
    const HEX:&[u8]=b"0123456789ABCDEF";
    for byte in text.bytes() {match byte {b' '=>out.push('+'),b'A'..=b'Z'|b'a'..=b'z'|b'0'..=b'9'|b'*'|b'-'|b'.'|b'_'=>out.push(byte as char),_=>{out.push('%');out.push(HEX[(byte>>4) as usize] as char);out.push(HEX[(byte&15) as usize] as char);}}}
}
fn decode_fragment(text:&str)->Option<String> {
    let mut bytes=Vec::new();let mut source=text.bytes();
    while let Some(byte)=source.next(){if byte==b'%' {let hi=(source.next()? as char).to_digit(16)?;let lo=(source.next()? as char).to_digit(16)?;bytes.push((hi*16+lo) as u8);}else{bytes.push(byte);}}
    String::from_utf8(bytes).ok()
}

impl Document {
    fn refresh_highlights(&mut self) {
        if let Some(web)=&mut self.web_layout {web.set_search_highlight(&self.search_query,self.active_match);return;}
        let mut index=0usize;
        for b in &mut self.layout_boxes {
            b.search_highlights.clear();
            if self.search_query.is_empty(){continue;}
            let mut found=false; let mut y=4;
            for line in &b.lines {
                let text:String=line.runs.iter().map(|run|run.span.text.as_str()).collect();
                if fold_text(&text).contains(&self.search_query){b.search_highlights.push((y,line.height,self.active_match==Some(index)));found=true;}
                y+=line.height as i32;
            }
            if let Some(rows)=&b.table_rows {for row in rows {for cell in &row.cells {let mut y=row.y_rel+6;for line in &cell.lines {
                let text:String=line.runs.iter().map(|run|run.span.text.as_str()).collect();
                if fold_text(&text).contains(&self.search_query){b.search_highlights.push((y,line.height,self.active_match==Some(index)));found=true;}
                y+=line.height as i32;
            }}}}
            if found{index+=1;}
        }
    }
}

impl Document {
    /// Stage a bounded batch so an invalid mutation never partially changes a live DOM.
    pub fn apply_mutations_checked(&mut self,mutations:&[dom_arena::DomMutation])->Result<(),String> {
        use dom_arena::{DomMutation,NodeData};
        if mutations.len()>dom_arena::OCEL_JS_MAX_MUTATIONS{return Err(String::from("Mutation count budget exceeded"));}
        let arena=self.arena.as_ref().ok_or_else(||String::from("Document has no DOM"))?;
        validate_arena(arena,&self.external_stylesheets)?;
        let mut growth=0usize;let mut nodes=arena.nodes.len();
        for mutation in mutations {
            let size=match mutation {
                DomMutation::CreateNode{data,..}=>{nodes=nodes.saturating_add(1);match data {
                    NodeData::Text(text)|NodeData::Comment(text)=>text.len(),
                    NodeData::Element{tag,attributes}=>tag.len()+attributes.iter().map(|(name,value)|name.len()+value.len()).sum::<usize>(),_=>0,
                }},
                DomMutation::SetText{text,..}=>{nodes=nodes.saturating_add(1);text.len()},
                DomMutation::SetAttribute{key,val,..}=>key.len()+val.len(),
                DomMutation::RemoveAttribute{key,..}=>key.len(),
                DomMutation::SetDocumentTitle{title}=>title.len(),
                _=>0,
            };
            growth=growth.saturating_add(size);
            if growth>dom_arena::OCEL_JS_MAX_MUTATION_BYTES||nodes>crate::budget::MAX_DOM_NODES {
                return Err(String::from("Mutation allocation budget exceeded"));
            }
        }
        let current_bytes:usize=arena.nodes.iter().map(|node|match &node.data {
            NodeData::Text(text)|NodeData::Comment(text)=>text.len(),
            NodeData::Element{tag,attributes}=>tag.len()+attributes.iter().map(|(name,value)|name.len()+value.len()).sum::<usize>(),_=>0,
        }).sum();
        if current_bytes.saturating_add(growth)>crate::budget::MAX_TEXT_BYTES{return Err(String::from("Mutation exceeds document text budget"));}
        if mutations.iter().all(|mutation|matches!(mutation,DomMutation::SetDocumentTitle{..})){return Ok(());}
        let clone_bytes=arena.nodes.iter().map(|node|core::mem::size_of::<dom_arena::DomNode>()+match &node.data {
            NodeData::Text(text)|NodeData::Comment(text)=>text.len(),
            NodeData::Element{tag,attributes}=>tag.len()+attributes.len()*core::mem::size_of::<(String,String)>()+attributes.iter().map(|(name,value)|name.len()+value.len()).sum::<usize>(),_=>0,
        }).sum::<usize>();
        if self.resident_bytes().saturating_add(clone_bytes).saturating_add(growth.saturating_mul(4))>self.memory_budget{return Err(String::from("Mutation staging exceeds resident memory budget"));}
        let future_layout=current_bytes.saturating_add(growth).saturating_mul(64).saturating_add(nodes.saturating_mul(1024)).saturating_add(128*1024);
        if self.resident_bytes().saturating_add(clone_bytes).saturating_add(growth.saturating_mul(4)).saturating_add(future_layout)>self.memory_budget{return Err(String::from("Mutation layout exceeds resident memory budget"));}
        let mut staged=arena.clone();
        for mutation in mutations {
            if !staged.apply_mutation(mutation){return Err(String::from("Invalid or divergent DOM mutation"));}
            validate_arena(&staged,&self.external_stylesheets)?;
        }
        if !staged.validate(){return Err(String::from("Mutation creates invalid DOM links"));}
        self.arena=Some(staged);Ok(())
    }
}

impl Document {
    pub fn set_memory_budget(&mut self,bytes:usize){self.memory_budget=bytes.min(crate::budget::MAX_RESIDENT_BYTES);}
    fn reserve_layout(&self,width:u32)->Result<(),String> {
        let (text,nodes)=if let Some(arena)=&self.arena {
            let text=arena.nodes.iter().map(|node|match &node.data {
                dom_arena::NodeData::Text(text)|dom_arena::NodeData::Comment(text)=>text.len(),
                dom_arena::NodeData::Element{tag,attributes}=>tag.len()+attributes.iter().map(|(name,value)|name.len()+value.len()).sum::<usize>(),_=>0,
            }).sum::<usize>();
            (text,arena.nodes.len())
        }else{
            let mut total_text=0usize;
            let mut items=self.nodes.len();
            for node in &self.nodes {match node {
                DocNode::Heading{text,..}|DocNode::Button{text,..}=>total_text+=text.len(),
                DocNode::Paragraph{spans}|DocNode::ListItem{spans,..}=>total_text+=spans.iter().map(|span|span.text.len()).sum::<usize>(),
                DocNode::CodeBlock{lines,..}|DocNode::RawLines{lines}|DocNode::Blockquote{lines}=>total_text+=lines.iter().map(String::len).sum::<usize>(),
                DocNode::Table{headers,rows}=>{
                    total_text+=headers.iter().chain(rows.iter().flatten()).map(String::len).sum::<usize>();
                    items+=headers.len()+rows.iter().map(Vec::len).sum::<usize>();
                }
                DocNode::Input{value,placeholder,..}=>total_text+=value.len()+placeholder.len(),_=>(),
            };}
            (total_text,items)
        };
        let width=width.saturating_sub(40).max(1) as usize*100/self.zoom.max(1) as usize;
        let line_chars=(width/128).max(1);
        let reserve=text.saturating_mul(24).saturating_add(text.div_ceil(line_chars).saturating_mul(192))
            .saturating_add(nodes.saturating_mul(1024)).saturating_add(128*1024);
        if self.resident_bytes().saturating_add(reserve)>self.memory_budget{return Err(String::from("Layout exceeds resident memory budget"));}
        Ok(())
    }
}
