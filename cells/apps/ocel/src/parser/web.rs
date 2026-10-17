// SPDX-License-Identifier: MIT
//! Native DOM -> cascade -> Taffy -> paint boxes. Inline layout and painting
//! share normalized TrueType advances, kerning, baselines, and run rectangles.
use alloc::{rc::Rc, string::String, vec::Vec};
use dom_arena::{DocumentArena, NodeData, NodeId};
use taffy::prelude::*;
use crate::{doc::StyledSpan, draw::Color};
use super::css::{ComputedStyle, Stylesheet};
use ostd::typography::{normalized_chars, FontFace, TextFonts};

#[derive(Clone, Debug)]
pub struct TextRun {
    pub span: StyledSpan,
    pub px: f32,
    pub node_id: Option<NodeId>,
    pub x: f32,
    pub width: f32,
    pub line_height: f32,
}
impl TextRun {
    pub fn new(span: StyledSpan, px: f32, node_id: Option<NodeId>, line_height: f32) -> Self {
        Self { span, px, node_id, x: 0.0, width: 0.0, line_height }
    }
    pub fn face(&self) -> FontFace { self.span.face() }
}
#[derive(Clone, Debug)]
pub struct TextLine {
    pub runs: Vec<TextRun>,
    pub height: u32,
    pub width: f32,
    pub baseline: f32,
}
#[derive(Clone, Debug)]
pub struct WebBox {
    pub node_id:Option<NodeId>,
    pub x:i32,pub y:i32,pub width:u32,pub height:u32,
    pub background:Option<Color>,pub border_color:Color,
    pub border:[u32;4],pub lines:Vec<TextLine>,
    pub input_data: Option<crate::doc::FormInputLayout>,
    pub image: Option<(u32,u32,Rc<Vec<u8>>)>,
    highlights:Vec<(i32,u32,bool)>,
    control_px:f32,
}
#[derive(Clone, Debug, Default)]
pub struct WebLayout { pub boxes:Vec<WebBox>,pub height:i32 }
#[derive(Clone)]
struct TextContext { runs:Vec<TextRun>,line_height:u32,pre:bool,wrap_pre:bool,align:u8 }
struct Entry { taffy:taffy::NodeId, parent:Option<usize>, node_id:Option<NodeId>, hidden:bool,background:Option<Color>,border_color:Color,text:Option<Rc<TextContext>>,input_data:Option<crate::doc::FormInputLayout>,image:Option<(u32,u32,Rc<Vec<u8>>)> }
struct Builder<'a> {arena:&'a DocumentArena,sheet:Stylesheet,viewport:(u32,u32),tree:TaffyTree<Rc<TextContext>>,entries:Vec<Entry>,assets:&'a [crate::doc::DocNode],max_font:f32}

fn collect_styles(arena:&DocumentArena,id:NodeId,external:&[(NodeId,String)],sheet:&mut Stylesheet,viewport:(u32,u32)) {
    let Some(node)=arena.get(id) else{return;};
    if node.tag()==Some("style") {sheet.append(&arena.get_text_content(id),viewport);}
    if node.tag()==Some("link") {if let Some((_,css))=external.iter().find(|(n,_)|*n==id) {sheet.append(css,viewport);}}
    let mut child=node.first_child;while let Some(id)=child{collect_styles(arena,id,external,sheet,viewport);child=arena.get(id).and_then(|n|n.next_sibling);}
}
impl<'a> Builder<'a> {
    fn text_leaf(&mut self,runs:&mut Vec<TextRun>,style:&ComputedStyle,parent:usize)->Result<taffy::NodeId,taffy::TaffyError> {
        let context=Rc::new(TextContext {runs:core::mem::take(runs),line_height:pixel(style.line_height.max(1.0)),pre:style.pre,wrap_pre:style.wrap_pre,align:style.text_align});
        let leaf_style=Style{display:Display::Block,..Style::default()};
        let id=self.tree.new_leaf_with_context(leaf_style,context.clone())?;
        self.entries.push(Entry{taffy:id,parent:Some(parent),node_id:None,hidden:false,background:None,border_color:style.border_color,text:Some(context),input_data:None,image:None});Ok(id)
    }
    fn element(&mut self,id:NodeId,parent:Option<usize>,mut style:ComputedStyle)->Result<taffy::NodeId,taffy::TaffyError> {
        style.font_size=style.font_size.min(self.max_font);
        style.line_height=style.line_height.clamp(1.0,self.max_font*4.0);
        let tid=self.tree.new_leaf(Style::default())?;let index=self.entries.len();
        self.entries.push(Entry{taffy:tid,parent,node_id:Some(id),hidden:style.layout.display==Display::None,background:style.background,border_color:style.border_color,text:None,input_data:None,image:None});
        if style.layout.display==Display::None{self.tree.set_style(tid,style.layout)?;return Ok(tid);}
        if let Some(node)=self.arena.get(id) {
            if matches!(node.tag(),Some("input"|"textarea"|"button"|"select")) {
                let kind=if node.tag()==Some("input"){node.get_attribute("type").unwrap_or("text").to_ascii_lowercase()}
                    else if node.tag()==Some("button"){node.get_attribute("type").unwrap_or("submit").to_ascii_lowercase()}
                    else if node.tag()==Some("select"){String::from("select")}else{String::from("textarea")};
                if kind=="hidden" {style.layout.display=Display::None;self.entries[index].hidden=true;}
                let value=if node.tag()==Some("input"){String::from(node.get_attribute("value").unwrap_or(""))}
                    else{node.get_attribute("value").map(String::from).unwrap_or_else(||self.arena.get_text_content(id))};
                let checkbox=kind=="checkbox";
                style.layout.size.width=length(if checkbox{20.0}else if kind=="submit"||kind=="button"{160.0}else{300.0f32.min(self.viewport.0 as f32)});
                style.layout.size.height=length(if checkbox{20.0}else if kind=="textarea"{80.0}else{30.0});
                self.entries[index].input_data=Some(crate::doc::FormInputLayout{input_type:kind,name:String::from(node.get_attribute("name").unwrap_or("")),value,placeholder:String::from(node.get_attribute("placeholder").unwrap_or("")),checked:node.get_attribute("checked").is_some(),is_focused:false});
                self.tree.set_style(tid,style.layout)?;return Ok(tid);
            }
            if node.tag()==Some("img") {
                if let Some((w,h,pixels))=self.assets.iter().find_map(|asset|match asset {
                    crate::doc::DocNode::DomImage{node_id,width,height,pixels} if *node_id==id=>Some((*width,*height,pixels.clone())),_=>None}) {
                    let width=(w as f32).min(self.viewport.0 as f32).max(1.0);
                    style.layout.size.width=length(width);style.layout.size.height=length(h as f32*width/w.max(1) as f32);
                    self.entries[index].image=Some((w,h,pixels));
                } else {
                    let alt=node.get_attribute("alt").unwrap_or("[Image unavailable]");
                    let mut runs=Vec::new();append_text(&mut runs,alt,&style,None,id);
                    let context=Rc::new(TextContext{runs,line_height:pixel(style.line_height),pre:false,wrap_pre:true,align:0});
                    self.tree.set_node_context(tid,Some(context.clone()))?;self.entries[index].text=Some(context);
                }
                self.tree.set_style(tid,style.layout)?;return Ok(tid);
            }
        }
        let mut children=Vec::new();let mut runs=Vec::new();
        let arena=self.arena;
        let mut child=arena.get(id).and_then(|n|n.first_child);
        while let Some(cid)=child {
            let Some(node)=arena.get(cid) else{break;};
            match &node.data {
                NodeData::Text(text)=>append_text(&mut runs,text,&style,None,id),
                NodeData::Element{..}=>{
                    let mut child_style=self.sheet.compute(self.arena,cid,Some(&style),self.viewport);
                    child_style.font_size=child_style.font_size.min(self.max_font);
                    child_style.line_height=child_style.line_height.clamp(1.0,self.max_font*4.0);
                    if child_style.layout.display!=Display::None {
                        if child_style.inline && !contains_replaced(self.arena,cid) {self.inline(cid,&child_style,None,&mut runs);}
                        else {if !runs.is_empty(){children.push(self.text_leaf(&mut runs,&style,index)?);}children.push(self.element(cid,Some(index),child_style)?);}
                    }
                },_=>{}
            }
            child=node.next_sibling;
        }
        if !runs.is_empty(){children.push(self.text_leaf(&mut runs,&style,index)?);}
        self.tree.set_children(tid,&children)?;self.tree.set_style(tid,style.layout)?;Ok(tid)
    }
    fn inline(&self,id:NodeId,style:&ComputedStyle,link:Option<&str>,runs:&mut Vec<TextRun>) {
        let Some(node)=self.arena.get(id) else{return;};
        let link=node.get_attribute("href").or(link);
        if node.tag()==Some("br") {append_text(runs,"\u{2028}",style,link,id);return;}
        let mut child=node.first_child;while let Some(cid)=child {let Some(n)=self.arena.get(cid) else{break;};match &n.data{
            NodeData::Text(text)=>append_text(runs,text,style,link,id),
            NodeData::Element{..}=>{let mut s=self.sheet.compute(self.arena,cid,Some(style),self.viewport);s.font_size=s.font_size.min(self.max_font);s.line_height=s.line_height.clamp(1.0,self.max_font*4.0);if s.layout.display!=Display::None{self.inline(cid,&s,link,runs);}},_=>{}
        }child=n.next_sibling;}
    }
}
fn append_text(runs:&mut Vec<TextRun>,text:&str,style:&ComputedStyle,link:Option<&str>,node_id:NodeId) {
    let mut span=StyledSpan::plain(text);
    span.color=Some(style.color);span.bold=style.bold;span.italic=style.italic;
    span.code=style.mono;span.link=link.map(String::from);
    append_run(runs,TextRun::new(span,style.font_size,Some(node_id),style.line_height));
}

fn empty_line(height:u32)->TextLine {
    TextLine { runs:Vec::new(),height,width:0.0,baseline:0.0 }
}
fn same_run(a:&TextRun,b:&TextRun)->bool {
    a.node_id==b.node_id && a.px==b.px && a.line_height==b.line_height
        && a.span.bold==b.span.bold && a.span.italic==b.span.italic
        && a.span.code==b.span.code && a.span.color==b.span.color && a.span.link==b.span.link
}
pub fn append_run(runs:&mut Vec<TextRun>,run:TextRun) {
    if let Some(last)=runs.last_mut().filter(|last|same_run(last,&run)) {
        last.span.text.push_str(&run.span.text);
    } else {runs.push(run);}
}
fn next_advance(fonts:&mut TextFonts,line:&TextLine,source:&TextRun,c:char)->f32 {
    let kern=line.runs.last().filter(|r|same_run(r,source))
        .and_then(|r|r.span.text.chars().next_back())
        .map_or(0.0,|left|fonts.kerning(source.face(),left,c,source.px));
    fonts.advance(source.face(),c,source.px)+kern
}
fn push_char(fonts:&mut TextFonts,line:&mut TextLine,source:&TextRun,c:char) {
    let advance=next_advance(fonts,line,source,c);
    line.baseline=line.baseline.max(fonts.ascender(source.face(),source.px));
    line.height=line.height.max(ceil_pixel(source.line_height.max(fonts.line_height(source.face(),source.px))));
    if let Some(last)=line.runs.last_mut().filter(|r|same_run(r,source)) {
        last.span.text.push(c);last.width+=advance;
    } else {
        let mut span=StyledSpan::plain("");
        span.bold=source.span.bold;span.italic=source.span.italic;span.code=source.span.code;
        span.link=source.span.link.clone();span.color=source.span.color;span.text.push(c);
        let mut run=TextRun::new(span,source.px,source.node_id,source.line_height);
        run.x=line.width;run.width=advance;line.runs.push(run);
    }
    line.width+=advance;
}
fn finish_line(fonts:&mut TextFonts,line:&mut TextLine,lines:&mut Vec<TextLine>,height:u32) {
    // The shared baseline also covers mixed-size ascenders and descenders.
    let descent=line.runs.iter().map(|r|
        fonts.line_height(r.face(),r.px)-fonts.ascender(r.face(),r.px))
        .fold(0.0f32,f32::max);
    line.height=line.height.max(ceil_pixel(line.baseline+descent));
    lines.push(core::mem::replace(line,empty_line(height)));
}
fn flush_word<'a>(fonts:&mut TextFonts,word:&mut Vec<(&'a TextRun,char)>,line:&mut TextLine,
    lines:&mut Vec<TextLine>,pending:&mut Option<&'a TextRun>,limit:f32,height:u32) {
    if word.is_empty(){return;}
    let mut width=0.0;let mut previous:Option<(&TextRun,char)>=None;
    for &(r,c) in word.iter() {
        width+=fonts.advance(r.face(),c,r.px);
        if let Some((left,l))=previous.filter(|(left,_)|same_run(left,r)) {
            width+=fonts.kerning(left.face(),l,c,left.px);
        }
        previous=Some((r,c));
    }
    let source=word[0].0;
    let space_source=pending.filter(|_|line.width>0.0);
    let space=space_source.map_or(0.0,|r|next_advance(fonts,line,r,' '));
    let boundary=if let Some(r)=space_source {
        if same_run(r,source) {fonts.kerning(source.face(),' ',word[0].1,source.px)}else{0.0}
    } else if line.width>0.0 {
        next_advance(fonts,line,source,word[0].1)-fonts.advance(source.face(),word[0].1,source.px)
    } else {0.0};
    if line.width>0.0 && line.width+space+width+boundary>limit {
        finish_line(fonts,line,lines,height);
    } else if let Some(r)=space_source {push_char(fonts,line,r,' ');}
    for (source,c) in word.drain(..) {
        if line.width>0.0 && line.width+next_advance(fonts,line,source,c)>limit {
            finish_line(fonts,line,lines,height);
        }
        push_char(fonts,line,source,c);
    }
    *pending=None;
}

/// Shared measured wrapping for HTML, Markdown, plain text, and table cells.
pub fn wrap_runs(fonts:&mut TextFonts,runs:&[TextRun],width:f32,height:u32,
    pre:bool,wrap_pre:bool,align:u8)->Vec<TextLine> {
    let limit=width.max(1.0);let mut lines=Vec::new();let mut line=empty_line(height);
    let mut pending=None;let mut word=Vec::new();
    for run in runs {for c in normalized_chars(&run.span.text) {
        if c=='\u{2028}' || (pre && c=='\n') {
            flush_word(fonts,&mut word,&mut line,&mut lines,&mut pending,limit,height);
            finish_line(fonts,&mut line,&mut lines,height);pending=None;
        } else if pre {
            // Expand tabs into measured spaces, rather than attempting a tab glyph.
            let count=if c=='\t' {4}else{1};let c=if c=='\t' {' '}else{c};
            for _ in 0..count {
                if wrap_pre && line.width>0.0 && line.width+next_advance(fonts,&line,run,c)>limit {
                    finish_line(fonts,&mut line,&mut lines,height);
                }
                push_char(fonts,&mut line,run,c);
            }
        } else if c.is_whitespace() {
            flush_word(fonts,&mut word,&mut line,&mut lines,&mut pending,limit,height);
            pending.get_or_insert(run);
        } else {word.push((run,c));}
    }}
    flush_word(fonts,&mut word,&mut line,&mut lines,&mut pending,limit,height);
    if !line.runs.is_empty() || (pre && lines.is_empty()) {finish_line(fonts,&mut line,&mut lines,height);}
    for line in &mut lines {
        let offset=match align {1=>(width-line.width).max(0.0)*0.5,2=>(width-line.width).max(0.0),_=>0.0};
        for run in &mut line.runs {run.x+=offset;}
    }
    lines
}
fn wrap(fonts:&mut TextFonts,context:&TextContext,width:f32)->Vec<TextLine> {
    wrap_runs(fonts,&context.runs,width,context.line_height,context.pre,context.wrap_pre,context.align)
}
fn intrinsic_widths(fonts:&mut TextFonts,context:&TextContext)->(f32,f32) {
    let mut maximum=0.0f32;let mut longest=0.0f32;
    let mut width=0.0f32;let mut word=0.0f32;
    let mut previous:Option<(&TextRun,char)>=None;let mut pending:Option<&TextRun>=None;
    for run in &context.runs {for c in normalized_chars(&run.span.text) {
        if c=='\u{2028}' || (context.pre && c=='\n') {
            maximum=maximum.max(width);longest=longest.max(word);
            width=0.0;word=0.0;previous=None;pending=None;continue;
        }
        if !context.pre && c.is_whitespace() {
            longest=longest.max(word);word=0.0;pending.get_or_insert(run);continue;
        }
        if let Some(space_source)=pending.filter(|_|width>0.0) {
            width+=fonts.advance(space_source.face(),' ',space_source.px);
            if let Some((left,l))=previous.filter(|(left,_)|same_run(left,space_source)) {
                width+=fonts.kerning(left.face(),l,' ',left.px);
            }
            previous=Some((space_source,' '));
        }
        pending=None;
        let count=if context.pre && c=='\t' {4}else{1};
        let c=if context.pre && c=='\t' {' '}else{c};
        for _ in 0..count {
            let glyph=fonts.advance(run.face(),c,run.px);
            let kern=previous.filter(|(left,_)|same_run(left,run))
                .map_or(0.0,|(left,l)|fonts.kerning(left.face(),l,c,left.px));
            width+=glyph+kern;
            if c.is_whitespace() {longest=longest.max(word);word=0.0;}
            else {
                word+=glyph+if previous.is_some_and(|(_,l)|!l.is_whitespace()) {kern}else{0.0};
            }
            previous=Some((run,c));
        }
    }}
    maximum=maximum.max(width);longest=longest.max(word);
    (maximum,if context.pre && !context.wrap_pre {maximum}else{longest})
}
fn measure(fonts:&mut TextFonts,context:&TextContext,known:Size<Option<f32>>,available:Size<AvailableSpace>)->Size<f32> {
    let (max_width,longest)=intrinsic_widths(fonts,context);
    let width=known.width.unwrap_or_else(||match available.width {
        AvailableSpace::Definite(w)=>w.max(0.0),AvailableSpace::MinContent=>longest,AvailableSpace::MaxContent=>max_width,
    });
    let lines=wrap(fonts,context,width);
    let actual=lines.iter().map(|l|l.width).fold(0.0f32,f32::max);
    Size{width:known.width.unwrap_or(actual.min(width)),height:known.height.unwrap_or(lines.iter().map(|l|l.height).sum::<u32>() as f32)}
}

pub fn paint_lines(fonts:&mut TextFonts,surf:&mut ostd::display::ViSurface,lines:&[TextLine],
    x:f32,y:f32,color:Color,clip:(i32,i32,i32,i32)) {
    let mut top=y;
    for line in lines {
        for run in &line.runs {
            let run_top=top+line.baseline-fonts.ascender(run.face(),run.px);
            crate::draw::text(fonts,surf,x+run.x,run_top,&run.span.text,run.face(),run.px,
                run.span.color.unwrap_or(color),clip);
        }
        top+=line.height as f32;
    }
}

fn pixel(value:f32)->u32 { (value.max(0.0) as i32) as u32 }
pub fn ceil_pixel(value:f32)->u32 {
    let whole=value.max(0.0) as u32;
    whole+u32::from(value>whole as f32)
}

#[cfg(test)]
pub fn layout(fonts:&mut TextFonts,arena:&DocumentArena,external:&[(NodeId,String)],width:u32,height:u32)->Result<WebLayout,taffy::TaffyError> {
    layout_with_nodes_zoom(fonts,arena,external,width,height,&[],1.0)
}

pub fn layout_with_nodes_zoom(fonts:&mut TextFonts,arena:&DocumentArena,external:&[(NodeId,String)],width:u32,height:u32,assets:&[crate::doc::DocNode],zoom:f32)->Result<WebLayout,taffy::TaffyError> {
    let viewport=(width.max(1),height.max(1));let mut sheet=Stylesheet::default();collect_styles(arena,arena.root,external,&mut sheet,viewport);
    let mut builder=Builder{arena,sheet,viewport,tree:TaffyTree::new(),entries:Vec::new(),assets,max_font:super::css::MAX_FONT_SIZE/zoom.max(1.0)};
    let mut root_style=builder.sheet.compute(arena,arena.root,None,viewport);
    root_style.layout.size.width=length(viewport.0 as f32);
    let root=builder.element(arena.root,None,root_style)?;
    builder.tree.compute_layout_with_measure(root,Size{width:AvailableSpace::Definite(viewport.0 as f32),height:AvailableSpace::MaxContent},|inputs,_,context,style| {
        taffy::compute_leaf_layout(inputs,style,|_,_|0.0,|known,available|context.as_ref().map_or(Size::ZERO,|ctx|measure(fonts,ctx,known,available)))
    })?;
    let mut result=WebLayout::default();let mut origins:Vec<(i32,i32)>=Vec::new();
    for entry in &builder.entries {
        let l=builder.tree.layout(entry.taffy)?;let (px,py)=entry.parent.map_or((0,0),|p|origins[p]);let x=px.saturating_add(l.location.x as i32);let y=py.saturating_add(l.location.y as i32);origins.push((x,y));
        if entry.hidden {continue;}
        let border=[pixel(l.border.left),pixel(l.border.right),pixel(l.border.top),pixel(l.border.bottom)];
        let (bx,by,bw,bh,lines)=if let Some(text)=&entry.text {(x,y,pixel(l.size.width),pixel(l.size.height),wrap(fonts,text,l.size.width))}else{(x,y,pixel(l.size.width),pixel(l.size.height),Vec::new())};
        result.height=result.height.max(by.saturating_add(bh as i32));
        result.boxes.push(WebBox{node_id:entry.node_id,x:bx,y:by,width:bw,height:bh,background:entry.background,border_color:entry.border_color,border,lines,input_data:entry.input_data.clone(),image:entry.image.clone(),highlights:Vec::new(),control_px:14.0});
    }
    // White default canvas is a web default, independent of the viewer chrome theme.
    if let Some(root)=result.boxes.first_mut(){if root.background.is_none(){root.background=Some(Color::rgb(255,255,255));}root.height=root.height.max(height);}
    Ok(result)
}

impl WebLayout {
    pub fn scale(&mut self,scale:f32) {
        self.height=(self.height as f32*scale) as i32;
        for b in &mut self.boxes { b.x=(b.x as f32*scale) as i32;b.y=(b.y as f32*scale) as i32;b.width=(b.width as f32*scale) as u32;b.height=(b.height as f32*scale) as u32;
            for width in &mut b.border {*width=(*width as f32*scale) as u32;} crate::doc::scale_lines(&mut b.lines,scale); }
        for b in &mut self.boxes {b.control_px*=scale;}
    }
    pub fn render(&self,fonts:&mut TextFonts,surf:&mut ostd::display::ViSurface,view:(i32,i32,u32,u32),scroll:i32) {
        let (vx,vy,vw,vh)=view;let clip=(vx,vy,vx.saturating_add(vw as i32),vy.saturating_add(vh as i32));
        fn rect(surf:&mut ostd::display::ViSurface,x:i32,y:i32,w:u32,h:u32,color:Color,clip:(i32,i32,i32,i32)) {
            let x0=x.max(clip.0);let y0=y.max(clip.1);let x1=x.saturating_add(w as i32).min(clip.2);let y1=y.saturating_add(h as i32).min(clip.3);
            if x1>x0&&y1>y0 {crate::draw::fill_rect(surf,x0,y0,(x1-x0) as u32,(y1-y0) as u32,color);}
        }
        for b in &self.boxes {
            let x=vx.saturating_add(b.x);let y=vy.saturating_add(b.y).saturating_sub(scroll);
            if y.saturating_add(b.height as i32)<=clip.1||y>=clip.3 {continue;}
            if let Some(bg)=b.background {rect(surf,x,y,b.width,b.height,bg,clip);}
            let [left,right,top,bottom]=b.border;
            rect(surf,x,y,left,b.height,b.border_color,clip);
            rect(surf,x.saturating_add(b.width.saturating_sub(right) as i32),y,right,b.height,b.border_color,clip);
            rect(surf,x,y,b.width,top,b.border_color,clip);
            rect(surf,x,y.saturating_add(b.height.saturating_sub(bottom) as i32),b.width,bottom,b.border_color,clip);
            if let Some((iw,ih,pixels))=&b.image {
                crate::draw::draw_image_clipped(surf,x,y,b.width,b.height,pixels,(*iw*4) as usize,*ih,clip);continue;
            }
            if let Some(input)=&b.input_data {
                let color=if input.is_focused{crate::draw::theme::ACCENT_CYAN}else{b.border_color};
                rect(surf,x,y,b.width,b.height,crate::draw::theme::BG_INPUT,clip);
                rect(surf,x,y,b.width,1,color,clip);rect(surf,x,y,1,b.height,color,clip);
                rect(surf,x,y+b.height.saturating_sub(1) as i32,b.width,1,color,clip);
                rect(surf,x+b.width.saturating_sub(1) as i32,y,1,b.height,color,clip);
                if input.input_type=="checkbox" {
                    if input.checked {rect(surf,x+4,y+4,b.width.saturating_sub(8),b.height.saturating_sub(8),crate::draw::theme::ACCENT_CYAN,clip);}
                } else {
                    let text=if !matches!(input.input_type.as_str(),"text"|"textarea"|"password"|"email"|"search"|"url"|"tel"|"submit"|"button") {
                        "Unsupported control (read-only)"
                    }else if input.value.is_empty(){if input.input_type=="submit"{"Submit"}else{&input.placeholder}}else{&input.value};
                    let text_clip=(clip.0.max(x),clip.1.max(y),clip.2.min(x+b.width as i32),clip.3.min(y+b.height as i32));
                    if input.input_type=="password" {
                        let advance=fonts.measure(FontFace::UiRegular,"•",b.control_px);
                        for index in 0..input.value.chars().count() {crate::draw::text(fonts,surf,x as f32+6.0+index as f32*advance,(y+4) as f32,"•",FontFace::UiRegular,b.control_px,crate::draw::theme::TEXT_PRIMARY,text_clip);}
                    }else{crate::draw::text(fonts,surf,(x+6) as f32,(y+4) as f32,text,FontFace::UiRegular,b.control_px,crate::draw::theme::TEXT_PRIMARY,text_clip);}
                }
                continue;
            }
            for &(offset,height,active) in &b.highlights {
                rect(surf,x,y+offset,b.lines.iter().map(|line|line.width as u32).max().unwrap_or(1),height,if active{Color::rgb(255,185,70)}else{Color::rgb(255,240,140)},clip);
            }
            paint_lines(fonts,surf,&b.lines,x as f32,y as f32,Color::rgb(32,32,32),clip);
        }
    }
    pub fn search(&self,query:&str)->Vec<i32> {
        let query=crate::doc::fold_text(query.trim());
        if query.is_empty(){return Vec::new();}
        let mut result=Vec::new();let mut text=String::new();
        for b in &self.boxes {for line in &b.lines {text.clear();for run in &line.runs {text.push_str(&run.span.text);}if crate::doc::fold_text(&text).contains(&query){result.push(b.y);break;}}}result
    }
    pub fn hit_link(&self,x:i32,y:i32)->Option<String> {
        for b in self.boxes.iter().rev() {
            let mut line_y=b.y;
            for line in &b.lines {
                if y>=line_y && y<line_y+line.height as i32 {
                    for run in &line.runs {
                        let left=b.x as f32+run.x;
                        if (x as f32)>=left && (x as f32)<left+run.width {
                            if let Some(link)=&run.span.link {return Some(link.clone());}
                        }
                    }
                }
                line_y+=line.height as i32;
            }
        }
        None
    }
    pub fn hit_node(&self,x:i32,y:i32)->Option<NodeId> {
        for b in self.boxes.iter().rev() {
            let mut line_y=b.y;
            for line in &b.lines {
                if y>=line_y&&y<line_y.saturating_add(line.height as i32) {
                    for run in &line.runs {
                        let left=b.x as f32+run.x;
                        if (x as f32)>=left && (x as f32)<left+run.width {return run.node_id;}
                    }
                }
                line_y=line_y.saturating_add(line.height as i32);
            }
            if x>=b.x&&x<b.x.saturating_add(b.width as i32)&&y>=b.y&&y<b.y.saturating_add(b.height as i32) {if let Some(id)=b.node_id{return Some(id);}}
        }
        None
    }
}

#[cfg(test)]
#[path="web_tests.rs"]
mod tests;

impl WebLayout {
    pub fn set_search_highlight(&mut self,query:&str,active:Option<usize>) {
        let mut index=0usize;
        for b in &mut self.boxes {
            b.highlights.clear();if query.is_empty(){continue;}
            let mut found=false;let mut y=0;
            for line in &b.lines {
                let text:String=line.runs.iter().map(|run|run.span.text.as_str()).collect();
                if crate::doc::fold_text(&text).contains(query){b.highlights.push((y,line.height,active==Some(index)));found=true;}
                y+=line.height as i32;
            }
            if found{index+=1;}
        }
    }
}

fn contains_replaced(arena:&DocumentArena,id:NodeId)->bool {
    let Some(node)=arena.get(id)else{return false;};
    if matches!(node.tag(),Some("input"|"textarea"|"button"|"img"|"select")){return true;}
    let mut child=node.first_child;
    while let Some(id)=child {if contains_replaced(arena,id){return true;}child=arena.get(id).and_then(|node|node.next_sibling);}
    false
}
