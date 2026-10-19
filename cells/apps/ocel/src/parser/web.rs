// SPDX-License-Identifier: MIT
//! Native DOM -> cascade -> Taffy -> paint boxes. Inline layout and painting
//! share normalized TrueType advances, kerning, baselines, and run rectangles.
use alloc::{rc::Rc, string::String, vec::Vec};
use dom_arena::{DocumentArena, NodeData, NodeId};
use taffy::prelude::*;
use taffy::geometry::Point;
use taffy::tree::{Cache, Layout, LayoutInput, LayoutOutput, LayoutPartialTree, LayoutBlockContainer, LayoutFlexboxContainer, LayoutGridContainer, RoundTree, RunMode, TraversePartialTree, TraverseTree};
use taffy::tree::traits::CacheTree;
use crate::{doc::StyledSpan, draw::Color};
use super::css::{ComputedStyle, CssPosition, Stylesheet};
use ostd::typography::{normalized_chars, FontFace, TextFonts};

#[derive(Clone, Debug)]
pub struct TextRun {
    pub span: StyledSpan,
    pub px: f32,
    pub node_id: Option<NodeId>,
    pub x: f32,
    pub width: f32,
    pub line_height: f32,
    pub visible: bool,
}
impl TextRun {
    pub fn new(span: StyledSpan, px: f32, node_id: Option<NodeId>, line_height: f32) -> Self {
        Self { span, px, node_id, x: 0.0, width: 0.0, line_height, visible: true }
    }
    pub fn with_visible(span: StyledSpan, px: f32, node_id: Option<NodeId>, line_height: f32, visible: bool) -> Self {
        Self { span, px, node_id, x: 0.0, width: 0.0, line_height, visible }
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
    pub font_size:f32,pub line_height:f32,pub color:Color,
    pub display:Display,pub overflow:Point<taffy::style::Overflow>,
    pub padding:[u32;4],pub box_sizing:BoxSizing,
    pub(crate) position:CssPosition,
    pub inline:bool,pub atomic_inline:bool,
    highlights:Vec<(i32,u32,bool)>,
    fixed:bool,
    clip:BoxClip,
    control_px:f32,
    pub visible:bool,
}
#[derive(Clone, Debug, Default)]
pub struct WebLayout { pub boxes:Vec<WebBox>,pub height:i32 }
#[derive(Clone)]
struct TextContext { runs:Vec<TextRun>,line_height:u32,pre:bool,wrap_pre:bool,align:u8 }
#[derive(Clone)]
struct Entry {
    taffy:taffy::NodeId, parent:Option<usize>, node_id:Option<NodeId>, hidden:bool,
    background:Option<Color>,border_color:Color,text:Option<Rc<TextContext>>,
    input_data:Option<crate::doc::FormInputLayout>,image:Option<(u32,u32,Rc<Vec<u8>>)>,
    position:CssPosition,z_index:Option<i32>,containing:Option<usize>,placeholder:Option<taffy::NodeId>,
    font_size:f32,line_height:f32,color:Color,
    inline:bool,atomic_inline:bool,
    paint_order:usize,
    visible:bool,
}

#[derive(Clone,Copy,Debug,Default)]
struct BoxClip {x:Option<(i32,i32)>,y:Option<(i32,i32)>}
impl BoxClip {
    fn intersect_axis(old:Option<(i32,i32)>,new:(i32,i32))->Option<(i32,i32)>{
        Some(old.map_or(new,|(a,b)|(a.max(new.0),b.min(new.1))))
    }
    fn contains(self,x:i32,y:i32)->bool{
        self.x.map_or(true,|(a,b)|x>=a&&x<b)&&self.y.map_or(true,|(a,b)|y>=a&&y<b)
    }
    fn screen(self,view:(i32,i32,i32,i32),scroll:i32)->(i32,i32,i32,i32){
        let x=self.x.map_or((view.0,view.2),|(a,b)|(view.0.saturating_add(a).max(view.0),view.0.saturating_add(b).min(view.2)));
        let y=self.y.map_or((view.1,view.3),|(a,b)|(view.1.saturating_add(a).saturating_sub(scroll).max(view.1),view.1.saturating_add(b).saturating_sub(scroll).min(view.3)));
        (x.0,y.0,x.1,y.1)
    }
    fn scale(&mut self,scale:f32){
        for axis in [&mut self.x,&mut self.y]{if let Some((a,b))=axis{*a=(*a as f32*scale) as i32;*b=(*b as f32*scale) as i32;}}
    }
}
struct Builder<'a> {arena:&'a DocumentArena,sheet:Stylesheet,viewport:(u32,u32),tree:WebTree,entries:Vec<Entry>,assets:&'a [crate::doc::DocNode],max_font:f32}

// TaffyTree's convenience adapter does not expose a calc resolver. Keep the
// native algorithms and cache, supplying the browser's arena-backed resolver.
struct LayoutNode {
    style:Style, children:Vec<taffy::NodeId>, context:Option<Rc<TextContext>>, intrinsic:Size<f32>,
    layout:Layout, rounded:Layout, cache:Cache,parent:Option<taffy::NodeId>,
    inline_flow:bool,gap:Option<(FontFace,f32)>,break_before:bool,
    entry:Option<usize>,
}
#[derive(Default)]
struct WebTree { nodes:Vec<LayoutNode>, calcs:Rc<core::cell::RefCell<Vec<super::css::LinearCalc>>> }
impl WebTree {
    fn new_leaf(&mut self,style:Style)->Result<taffy::NodeId,taffy::TaffyError>{
        let id=taffy::NodeId::from(self.nodes.len());
        self.nodes.push(LayoutNode{style,children:Vec::new(),context:None,intrinsic:Size::ZERO,layout:Layout::default(),rounded:Layout::default(),cache:Cache::new(),parent:None,inline_flow:false,gap:None,break_before:false,entry:None});Ok(id)
    }
    fn new_leaf_with_context(&mut self,style:Style,context:Rc<TextContext>)->Result<taffy::NodeId,taffy::TaffyError>{
        let id=self.new_leaf(style)?;self.set_node_context(id,Some(context))?;Ok(id)
    }
    fn set_node_context(&mut self,id:taffy::NodeId,context:Option<Rc<TextContext>>)->Result<(),taffy::TaffyError>{self.nodes[usize::from(id)].context=context;Ok(())}
    fn set_style(&mut self,id:taffy::NodeId,style:Style)->Result<(),taffy::TaffyError>{self.nodes[usize::from(id)].style=style;Ok(())}
    fn set_children(&mut self,id:taffy::NodeId,children:&[taffy::NodeId])->Result<(),taffy::TaffyError>{
        for child in children{self.nodes[usize::from(*child)].parent=Some(id);}
        self.nodes[usize::from(id)].children=children.to_vec();Ok(())
    }
    fn layout(&self,id:taffy::NodeId)->Result<&Layout,taffy::TaffyError>{Ok(&self.nodes[usize::from(id)].rounded)}
}
struct LayoutView<'a> {tree:&'a mut WebTree,fonts:&'a mut TextFonts}
impl TraversePartialTree for LayoutView<'_> {
    type ChildIter<'a> = core::iter::Copied<core::slice::Iter<'a,taffy::NodeId>> where Self:'a;
    fn child_ids(&self,id:taffy::NodeId)->Self::ChildIter<'_>{self.tree.nodes[usize::from(id)].children.iter().copied()}
    fn child_count(&self,id:taffy::NodeId)->usize{self.tree.nodes[usize::from(id)].children.len()}
    fn get_child_id(&self,id:taffy::NodeId,index:usize)->taffy::NodeId{self.tree.nodes[usize::from(id)].children[index]}
}
impl TraverseTree for LayoutView<'_>{}
impl CacheTree for LayoutView<'_> {
    fn cache_get(&mut self,id:taffy::NodeId,input:&LayoutInput)->Option<LayoutOutput>{self.tree.nodes[usize::from(id)].cache.get(input)}
    fn cache_store(&mut self,id:taffy::NodeId,input:&LayoutInput,output:LayoutOutput){self.tree.nodes[usize::from(id)].cache.store(input,output);}
    fn cache_clear(&mut self,id:taffy::NodeId){self.tree.nodes[usize::from(id)].cache.clear();}
}
impl LayoutView<'_> {
    fn inline_layout(&mut self,id:taffy::NodeId,inputs:LayoutInput)->LayoutOutput {
        use taffy::util::{MaybeResolve,ResolveOrZero};
        let style=self.tree.nodes[usize::from(id)].style.clone();
        let calcs=self.tree.calcs.clone();
        let strut=self.tree.nodes[usize::from(id)].intrinsic;
        let mut first_baseline=None;
        let mut output=taffy::compute_leaf_layout(inputs,&style,|h,b|super::css::resolve_calc(&calcs,h,b),|known,available|{
            let limit=known.width.or(available.width.into_option()).unwrap_or(match available.width{AvailableSpace::MinContent=>0.0,_=>f32::MAX});
            let basis=Size{width:known.width.or(inputs.parent_size.width),height:inputs.parent_size.height};
            let mut x=0.0f32;let mut y=0.0f32;let mut width=0.0f32;
            let baseline=self.fonts.ascender(FontFace::UiRegular,strut.width)+(strut.height-self.fonts.line_height(FontFace::UiRegular,strut.width)).max(0.0)*0.5;
            let mut ascent=baseline;let mut descent=(strut.height-baseline).max(0.0);
            let mut row:Vec<(taffy::NodeId,Layout,f32)>=Vec::new();
            let count=self.tree.nodes[usize::from(id)].children.len();
            for index in 0..count {
                let child=self.tree.nodes[usize::from(id)].children[index];
                let node=&self.tree.nodes[usize::from(child)];
                let child_style=node.style.clone();let gap=node.gap;let force_break=node.break_before;
                let margin=child_style.margin.resolve_or_zero(basis.width,|h,b|super::css::resolve_calc(&calcs,h,b));
                let child_inputs=LayoutInput{
                    parent_size:basis,known_dimensions:Size::NONE,known_dimensions_are_definite:Size{width:true,height:true},
                    available_space:Size{width:if node.context.is_some(){AvailableSpace::MaxContent}else{AvailableSpace::Definite(limit)},height:AvailableSpace::MaxContent},
                    vertical_margins_are_collapsible:Line::FALSE,..inputs
                };
                let child_output=self.child_layout(child,child_inputs,None);
                let size=child_output.size;
                let gap=if x>0.0{gap.map_or(0.0,|(face,px)|self.fonts.advance(face,' ',px))}else{0.0};
                if !row.is_empty()&&(force_break||x+gap+margin.left+size.width+margin.right>limit) {
                    first_baseline.get_or_insert(y+ascent);
                    for (child,mut layout,base) in row.drain(..){layout.location.y+=y+ascent-base;self.set_unrounded_layout(child,&layout);}
                    width=width.max(x);y+=ascent+descent;x=0.0;ascent=baseline;descent=(strut.height-baseline).max(0.0);
                }
                let gap=if x>0.0{gap}else{0.0};
                let node=&self.tree.nodes[usize::from(child)];
                let child_baseline=if let Some(context)=&node.context {
                    context.runs.iter().map(|run|self.fonts.ascender(run.face(),run.px)+(run.line_height-self.fonts.line_height(run.face(),run.px)).max(0.0)*0.5).fold(0.0,f32::max)
                }else{child_output.baselines.first.unwrap_or(size.height)};
                let inset=Rect{
                    left:child_style.inset.left.maybe_resolve(basis.width,|h,b|super::css::resolve_calc(&calcs,h,b)),
                    right:child_style.inset.right.maybe_resolve(basis.width,|h,b|super::css::resolve_calc(&calcs,h,b)),
                    top:child_style.inset.top.maybe_resolve(basis.height,|h,b|super::css::resolve_calc(&calcs,h,b)),
                    bottom:child_style.inset.bottom.maybe_resolve(basis.height,|h,b|super::css::resolve_calc(&calcs,h,b)),
                };
                let offset=Point{x:inset.left.or(inset.right.map(|v|-v)).unwrap_or(0.0),y:inset.top.or(inset.bottom.map(|v|-v)).unwrap_or(0.0)};
                let padding=child_style.padding.resolve_or_zero(basis.width,|h,b|super::css::resolve_calc(&calcs,h,b));
                let border=child_style.border.resolve_or_zero(basis.width,|h,b|super::css::resolve_calc(&calcs,h,b));
                let layout=Layout{size,location:Point{x:x+gap+margin.left+offset.x,y:margin.top+offset.y},padding,border,margin,..Layout::default()};
                ascent=ascent.max(margin.top+child_baseline);
                descent=descent.max(margin.bottom+size.height-child_baseline);
                row.push((child,layout,margin.top+child_baseline));
                x+=gap+margin.left+size.width+margin.right;
            }
            if !row.is_empty() {
                first_baseline.get_or_insert(y+ascent);
                for (child,mut layout,base) in row.drain(..){layout.location.y+=y+ascent-base;self.set_unrounded_layout(child,&layout);}
                width=width.max(x);y+=ascent+descent;
            }
            Size{width,height:y}
        });
        output.baselines=taffy::tree::Baselines{first:first_baseline,last:first_baseline};output
    }
    fn child_layout(&mut self,id:taffy::NodeId,inputs:LayoutInput,block:Option<&mut taffy::BlockContext<'_>>)->LayoutOutput {
        if inputs.run_mode==RunMode::PerformHiddenLayout{return taffy::compute_hidden_layout(self,id);}
        taffy::compute_cached_layout(self,id,inputs,|tree,id,inputs|{
            if tree.tree.nodes[usize::from(id)].inline_flow{return tree.inline_layout(id,inputs);}
            let node=&tree.tree.nodes[usize::from(id)];
            match (node.style.display,!node.children.is_empty()) {
                (Display::None,_)=>taffy::compute_hidden_layout(tree,id),
                (Display::Block,true)=>taffy::compute_block_layout(tree,id,inputs,block),
                (Display::FlowRoot,true)=>taffy::compute_block_layout(tree,id,inputs,None),
                (Display::Flex,true)=>taffy::compute_flexbox_layout(tree,id,inputs),
                (Display::Grid,true)=>taffy::compute_grid_layout(tree,id,inputs),
                _=>taffy::compute_leaf_layout(inputs,&node.style,|handle,basis|super::css::resolve_calc(&tree.tree.calcs,handle,basis),|known,available|{
                    node.context.as_ref().map_or(node.intrinsic,|ctx|measure(tree.fonts,ctx,known,available))
                }),
            }
        })
    }
}
impl LayoutPartialTree for LayoutView<'_> {
    type CoreContainerStyle<'a> = &'a Style where Self:'a;
    type CustomIdent=String;
    fn get_core_container_style(&self,id:taffy::NodeId)->&Style{&self.tree.nodes[usize::from(id)].style}
    fn resolve_calc_value(&self,handle:*const (),basis:f32)->f32{super::css::resolve_calc(&self.tree.calcs,handle,basis)}
    fn set_unrounded_layout(&mut self,id:taffy::NodeId,layout:&Layout){self.tree.nodes[usize::from(id)].layout=*layout;}
    fn compute_child_layout(&mut self,id:taffy::NodeId,inputs:LayoutInput)->LayoutOutput{self.child_layout(id,inputs,None)}
}
impl LayoutBlockContainer for LayoutView<'_> {
    type BlockContainerStyle<'a> = &'a Style where Self:'a;
    type BlockItemStyle<'a> = &'a Style where Self:'a;
    fn get_block_container_style(&self,id:taffy::NodeId)->&Style{self.get_core_container_style(id)}
    fn get_block_child_style(&self,id:taffy::NodeId)->&Style{self.get_core_container_style(id)}
    fn compute_block_child_layout(&mut self,id:taffy::NodeId,inputs:LayoutInput,block:Option<&mut taffy::BlockContext<'_>>)->LayoutOutput{self.child_layout(id,inputs,block)}
}
impl LayoutFlexboxContainer for LayoutView<'_> {
    type FlexboxContainerStyle<'a> = &'a Style where Self:'a;
    type FlexboxItemStyle<'a> = &'a Style where Self:'a;
    fn get_flexbox_container_style(&self,id:taffy::NodeId)->&Style{self.get_core_container_style(id)}
    fn get_flexbox_child_style(&self,id:taffy::NodeId)->&Style{self.get_core_container_style(id)}
}
impl LayoutGridContainer for LayoutView<'_> {
    type GridContainerStyle<'a> = &'a Style where Self:'a;
    type GridItemStyle<'a> = &'a Style where Self:'a;
    fn get_grid_container_style(&self,id:taffy::NodeId)->&Style{self.get_core_container_style(id)}
    fn get_grid_child_style(&self,id:taffy::NodeId)->&Style{self.get_core_container_style(id)}
}
impl RoundTree for LayoutView<'_> {
    fn get_unrounded_layout(&self,id:taffy::NodeId)->Layout{self.tree.nodes[usize::from(id)].layout}
    fn set_final_layout(&mut self,id:taffy::NodeId,layout:&Layout){self.tree.nodes[usize::from(id)].rounded=*layout;}
}

fn collect_styles(arena:&DocumentArena,id:NodeId,external:&[(NodeId,String)],sheet:&mut Stylesheet,viewport:(u32,u32)) {
    let Some(node)=arena.get(id) else{return;};
    if node.tag()==Some("style") {sheet.append(&arena.get_text_content(id),viewport);}
    if node.tag()==Some("link") {if let Some((_,css))=external.iter().find(|(n,_)|*n==id) {sheet.append(css,viewport);}}
    let mut child=node.first_child;while let Some(id)=child{collect_styles(arena,id,external,sheet,viewport);child=arena.get(id).and_then(|n|n.next_sibling);}
}
impl<'a> Builder<'a> {
    fn inline_groups(&mut self,children:Vec<taffy::NodeId>)->Result<Vec<taffy::NodeId>,taffy::TaffyError>{
        let mut result=Vec::new();let mut group=Vec::new();let mut atomic=false;
        for child in children {
            let entry=&self.entries[self.tree.nodes[usize::from(child)].entry.unwrap()];
            let inline=entry.inline&&!entry.position.out_of_flow();
            if !inline {
                self.finish_inline_group(&mut result,&mut group,atomic)?;atomic=false;
                result.push(child);
            } else {atomic|=entry.atomic_inline||entry.text.is_none();group.push(child);}
        }
        self.finish_inline_group(&mut result,&mut group,atomic)?;Ok(result)
    }
    fn finish_inline_group(&mut self,result:&mut Vec<taffy::NodeId>,group:&mut Vec<taffy::NodeId>,atomic:bool)->Result<(),taffy::TaffyError>{
        if !atomic{result.append(group);return Ok(());}
        let tid=self.tree.new_leaf(Style{display:Display::Block,..Style::default()})?;
        self.tree.nodes[usize::from(tid)].inline_flow=true;
        let mut children=Vec::new();let mut pending=None;let mut force_break=false;
        for child in group.drain(..) {
            let index=self.tree.nodes[usize::from(child)].entry.unwrap();
            let original=self.entries[index].clone();
            let Some(context)=original.text.clone() else {
                self.tree.nodes[usize::from(child)].gap=pending.take();
                self.tree.nodes[usize::from(child)].break_before=core::mem::take(&mut force_break);
                children.push(child);continue;
            };
            self.entries[index].hidden=true;
            let mut words:Vec<(Vec<TextRun>,Option<(FontFace,f32)>,bool)>=Vec::new();
            let mut word=Vec::new();
            for run in &context.runs {
                let mut segment=String::new();
                for c in normalized_chars(&run.span.text) {
                    if c.is_whitespace()||c=='\u{2028}' {
                        if !segment.is_empty(){
                            let span=StyledSpan{text:core::mem::take(&mut segment),bold:run.span.bold,italic:run.span.italic,code:run.span.code,link:run.span.link.clone(),color:run.span.color};
                            append_run(&mut word,TextRun::new(span,run.px,run.node_id,run.line_height));
                        }
                        if !word.is_empty(){words.push((core::mem::take(&mut word),pending.take(),core::mem::take(&mut force_break)));}
                        if c=='\u{2028}'||(context.pre&&c=='\n'){force_break=true;pending=None;}
                        else{pending.get_or_insert((run.face(),run.px));}
                    }else{segment.push(c);}
                }
                if !segment.is_empty(){
                    let span=StyledSpan{text:segment,bold:run.span.bold,italic:run.span.italic,code:run.span.code,link:run.span.link.clone(),color:run.span.color};
                    append_run(&mut word,TextRun::new(span,run.px,run.node_id,run.line_height));
                }
            }
            if !word.is_empty(){words.push((word,pending.take(),core::mem::take(&mut force_break)));}
            for (runs,gap,break_before) in words {
                let context=Rc::new(TextContext{runs,line_height:context.line_height,pre:true,wrap_pre:false,align:0});
                let child=self.tree.new_leaf_with_context(Style{display:Display::Block,..Style::default()},context.clone())?;
                let node=&mut self.tree.nodes[usize::from(child)];node.gap=gap;node.break_before=break_before;node.entry=Some(self.entries.len());
                let mut entry=original.clone();entry.taffy=child;entry.text=Some(context);self.entries.push(entry);
                children.push(child);
            }
            self.tree.nodes[usize::from(tid)].intrinsic=Size{width:original.font_size,height:original.line_height};
        }
        if self.tree.nodes[usize::from(tid)].intrinsic.height==0.0 {
            if let Some(first)=children.first().and_then(|id|self.tree.nodes[usize::from(*id)].entry).map(|index|&self.entries[index]){
                self.tree.nodes[usize::from(tid)].intrinsic=Size{width:first.font_size,height:first.line_height};
            }
        }
        self.tree.set_children(tid,&children)?;result.push(tid);Ok(())
    }
    fn text_leaf(&mut self,runs:&mut Vec<TextRun>,style:&ComputedStyle,parent:usize)->Result<taffy::NodeId,taffy::TaffyError> {
        let context=Rc::new(TextContext {runs:core::mem::take(runs),line_height:pixel(style.line_height.max(1.0)),pre:style.pre,wrap_pre:style.wrap_pre,align:style.text_align});
        let leaf_style=Style{display:Display::Block,..Style::default()};
        let id=self.tree.new_leaf_with_context(leaf_style,context.clone())?;
        self.tree.nodes[usize::from(id)].entry=Some(self.entries.len());
        self.entries.push(Entry{taffy:id,parent:Some(parent),node_id:None,hidden:false,background:None,border_color:style.border_color,text:Some(context),input_data:None,image:None,position:CssPosition::Static,z_index:None,containing:None,placeholder:None,font_size:style.font_size,line_height:style.line_height,color:style.color,inline:true,atomic_inline:false,paint_order:self.entries.len(),visible:style.visible});Ok(id)
    }
    fn element(&mut self,id:NodeId,parent:Option<usize>,mut style:ComputedStyle,link:Option<&str>)->Result<taffy::NodeId,taffy::TaffyError> {
        let link=self.arena.get(id).and_then(|n|n.get_attribute("href")).or(link);
        style.font_size=style.font_size.min(self.max_font);
        style.line_height=style.line_height.clamp(1.0,self.max_font*4.0);
        let tid=self.tree.new_leaf(Style::default())?;let index=self.entries.len();
        self.tree.nodes[usize::from(tid)].entry=Some(index);
        self.entries.push(Entry{taffy:tid,parent,node_id:Some(id),hidden:style.layout.display==Display::None,background:style.background,border_color:style.border_color,text:None,input_data:None,image:None,position:style.position,z_index:style.z_index,containing:None,placeholder:None,font_size:style.font_size,line_height:style.line_height,color:style.color,inline:style.inline,atomic_inline:style.atomic_inline,paint_order:index,visible:style.visible});
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
                if style.layout.size.width.is_auto(){style.layout.size.width=length(if checkbox{20.0}else if kind=="submit"||kind=="button"{160.0}else{300.0f32.min(self.viewport.0 as f32)});}
                if style.layout.size.height.is_auto(){style.layout.size.height=length(if checkbox{20.0}else if kind=="textarea"{80.0}else{30.0});}
                self.entries[index].input_data=Some(crate::doc::FormInputLayout{input_type:kind,name:String::from(node.get_attribute("name").unwrap_or("")),value,placeholder:String::from(node.get_attribute("placeholder").unwrap_or("")),checked:node.get_attribute("checked").is_some(),is_focused:false});
                self.tree.set_style(tid,style.layout)?;return Ok(tid);
            }
            if node.tag()==Some("img") {
                if let Some((w,h,pixels))=self.assets.iter().find_map(|asset|match asset {
                    crate::doc::DocNode::DomImage{node_id,width,height,pixels} if *node_id==id=>Some((*width,*height,pixels.clone())),_=>None}) {
                    let attr_w = node.get_attribute("width").and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0);
                    let attr_h = node.get_attribute("height").and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0);
                    if style.layout.size.width.is_auto(){if let Some(width)=attr_w{style.layout.size.width=length(width);}}
                    if style.layout.size.height.is_auto(){if let Some(height)=attr_h{style.layout.size.height=length(height);}}
                    if style.layout.size.width.is_auto()||style.layout.size.height.is_auto(){style.layout.aspect_ratio=Some(w.max(1) as f32/h.max(1) as f32);}
                    self.tree.nodes[usize::from(tid)].intrinsic=Size{width:w as f32,height:h as f32};
                    self.entries[index].image=Some((w,h,pixels));
                } else {
                    let alt=node.get_attribute("alt").unwrap_or("[Image unavailable]");
                    let mut runs=Vec::new();append_text(&mut runs,alt,&style,link,id);
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
                NodeData::Text(text)=>append_text(&mut runs,text,&style,link,id),
                NodeData::Element{..}=>{
                    let mut child_style=self.sheet.compute(self.arena,cid,Some(&style),self.viewport);
                    child_style.font_size=child_style.font_size.min(self.max_font);
                    child_style.line_height=child_style.line_height.clamp(1.0,self.max_font*4.0);
                    if child_style.layout.display!=Display::None {
                        if child_style.inline && !child_style.atomic_inline && child_style.position==CssPosition::Static && !is_replaced(arena,cid) {self.inline(cid,&child_style,link,&mut runs,&mut children,index,&style)?;}
                        else {if !runs.is_empty(){children.push(self.text_leaf(&mut runs,&style,index)?);}children.push(self.element(cid,Some(index),child_style,link)?);}
                    }
                },_=>{}
            }
            child=node.next_sibling;
        }
        if !runs.is_empty(){children.push(self.text_leaf(&mut runs,&style,index)?);}
        let children=if matches!(style.layout.display,Display::Block|Display::FlowRoot){self.inline_groups(children)?}else{children};
        self.tree.set_children(tid,&children)?;self.tree.set_style(tid,style.layout)?;Ok(tid)
    }
    fn inline(&mut self,id:NodeId,style:&ComputedStyle,link:Option<&str>,runs:&mut Vec<TextRun>,children:&mut Vec<taffy::NodeId>,parent:usize,block_style:&ComputedStyle)->Result<(),taffy::TaffyError> {
        let arena=self.arena;let Some(node)=arena.get(id) else{return Ok(());};
        let link=node.get_attribute("href").or(link);
        if node.tag()==Some("br") {append_text(runs,"\u{2028}",style,link,id);return Ok(());}
        let mut child=node.first_child;
        while let Some(cid)=child {
            let Some(n)=arena.get(cid) else{break;};
            match &n.data {
                NodeData::Text(text)=>append_text(runs,text,style,link,id),
                NodeData::Element{..}=>{
                    let mut s=self.sheet.compute(arena,cid,Some(style),self.viewport);
                    s.font_size=s.font_size.min(self.max_font);s.line_height=s.line_height.clamp(1.0,self.max_font*4.0);
                    if s.layout.display!=Display::None {
                        if s.inline&&!s.atomic_inline&&s.position==CssPosition::Static&&!is_replaced(arena,cid){self.inline(cid,&s,link,runs,children,parent,block_style)?;}
                        else {
                            if !runs.is_empty(){children.push(self.text_leaf(runs,block_style,parent)?);}
                            children.push(self.element(cid,Some(parent),s,link)?);
                        }
                    }
                },_=>{}
            }
            child=n.next_sibling;
        }
        Ok(())
    }
}
fn append_text(runs:&mut Vec<TextRun>,text:&str,style:&ComputedStyle,link:Option<&str>,node_id:NodeId) {
    let mut span=StyledSpan::plain(text);
    span.color=Some(style.color);
    span.bold=style.bold;span.italic=style.italic;
    span.code=style.mono;span.link=link.map(String::from);
    append_run(runs,TextRun::with_visible(span,style.font_size,Some(node_id),style.line_height,style.visible));
}

fn empty_line(height:u32)->TextLine {
    TextLine { runs:Vec::new(),height,width:0.0,baseline:0.0 }
}
fn same_run(a:&TextRun,b:&TextRun)->bool {
    a.node_id==b.node_id && a.px==b.px && a.line_height==b.line_height
        && a.span.bold==b.span.bold && a.span.italic==b.span.italic
        && a.span.code==b.span.code && a.span.color==b.span.color && a.span.link==b.span.link && a.visible==b.visible
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
        let mut run=TextRun::with_visible(span,source.px,source.node_id,source.line_height,source.visible);
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
            if !run.visible { continue; }
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
    let mut builder=Builder{arena,sheet,viewport,tree:WebTree::default(),entries:Vec::new(),assets,max_font:super::css::MAX_FONT_SIZE/zoom.max(1.0)};
    let mut root_style=builder.sheet.compute(arena,arena.root,None,viewport);
    root_style.layout.size.width=length(viewport.0 as f32);
    // The document node represents the initial containing block, not an
    // auto-height HTML element. Normal auto-height descendants remain indefinite.
    root_style.layout.size.height=length(viewport.1 as f32);
    builder.tree.calcs=root_style.calcs.clone();
    // An out-of-flow placeholder records the hypothetical static position but
    // contributes no size. Positioned subtrees are then laid out once, after
    // their actual containing block is known, in ancestor-before-child order.
    let root=builder.element(arena.root,None,root_style,None)?;
    for index in 0..builder.entries.len() {
        if !builder.entries[index].position.out_of_flow(){continue;}
        let mut containing=builder.entries[index].parent;
        if builder.entries[index].position==CssPosition::Fixed {containing=None;}
        else {while let Some(p)=containing {
            if builder.entries[p].position!=CssPosition::Static {break;}
            containing=builder.entries[p].parent;
        }}
        builder.entries[index].containing=containing;
        if let Some(parent)=builder.entries[index].parent {
            let placeholder=builder.tree.new_leaf(Style{position:Position::Absolute,size:Size{width:length(0.0),height:length(0.0)},..Style::default()})?;
            let tid=builder.entries[index].taffy;
            let children=&mut builder.tree.nodes[usize::from(builder.entries[parent].taffy)].children;
            if let Some(child)=children.iter_mut().find(|child|**child==tid){*child=placeholder;}
            builder.entries[index].placeholder=Some(placeholder);
        }
    }
    let mut view=LayoutView{tree:&mut builder.tree,fonts};
    taffy::compute_root_layout(&mut view,root,Size{width:AvailableSpace::Definite(viewport.0 as f32),height:AvailableSpace::Definite(viewport.1 as f32)});
    taffy::round_layout(&mut view,root);
    let mut result=WebLayout::default();let mut origins:Vec<(i32,i32)>=Vec::new();
    let mut fixed_flags=Vec::new();let mut descendant_clips:Vec<BoxClip>=Vec::new();
    let mut boxes:Vec<Option<WebBox>>=Vec::with_capacity(builder.entries.len());
    for index in 0..builder.entries.len() {
        let entry=&builder.entries[index];let mut base=entry.parent.map_or((0,0),|p|origins[p]);
        if entry.position.out_of_flow() {
            let mut area=(viewport.0 as f32,viewport.1 as f32);base=(0,0);
            if let Some(cb)=entry.containing {
                let l=builder.tree.layout(builder.entries[cb].taffy)?;
                let origin=origins[cb];
                base=(origin.0.saturating_add(l.border.left as i32),origin.1.saturating_add(l.border.top as i32));
                area=(l.size.width-l.border.left-l.border.right,l.size.height-l.border.top-l.border.bottom);
            }
            let mut fallback=entry.parent.map_or((0,0),|p|origins[p]);
            if let Some(placeholder)=entry.placeholder {
                let l=builder.tree.layout(placeholder)?;
                fallback.0=fallback.0.saturating_add(l.location.x as i32);
                fallback.1=fallback.1.saturating_add(l.location.y as i32);
            }
            let node=&mut builder.tree.nodes[usize::from(entry.taffy)];
            if node.style.inset.left.is_auto()&&node.style.inset.right.is_auto(){node.style.inset.left=length((fallback.0-base.0) as f32);}
            if node.style.inset.top.is_auto()&&node.style.inset.bottom.is_auto(){node.style.inset.top=length((fallback.1-base.1) as f32);}
            let containing=builder.tree.new_leaf(Style{display:Display::Block,size:Size{width:length(area.0.max(0.0)),height:length(area.1.max(0.0))},..Style::default()})?;
            builder.tree.set_children(containing,&[entry.taffy])?;
            let mut view=LayoutView{tree:&mut builder.tree,fonts};
            taffy::compute_root_layout(&mut view,containing,Size{width:AvailableSpace::Definite(area.0),height:AvailableSpace::Definite(area.1)});
            taffy::round_layout(&mut view,containing);
        }
        let l=builder.tree.layout(entry.taffy)?;
        if !entry.position.out_of_flow() {
            let stop=entry.parent.map(|p|builder.entries[p].taffy);
            let mut parent=builder.tree.nodes[usize::from(entry.taffy)].parent;
            while let Some(id)=parent {
                if Some(id)==stop{break;}
                let node=&builder.tree.nodes[usize::from(id)];
                base.0=base.0.saturating_add(node.rounded.location.x as i32);
                base.1=base.1.saturating_add(node.rounded.location.y as i32);
                parent=node.parent;
            }
        }
        let x=base.0.saturating_add(l.location.x as i32);let y=base.1.saturating_add(l.location.y as i32);origins.push((x,y));
        let inheritance=if entry.position.out_of_flow(){entry.containing}else{entry.parent};
        let fixed=entry.position==CssPosition::Fixed||inheritance.is_some_and(|p|fixed_flags[p]);fixed_flags.push(fixed);
        let clip=inheritance.map_or(BoxClip::default(),|p|descendant_clips[p]);
        let mut children_clip=clip;
        let overflow=builder.tree.nodes[usize::from(entry.taffy)].style.overflow;
        if overflow.x!=taffy::style::Overflow::Visible {
            children_clip.x=BoxClip::intersect_axis(children_clip.x,(x.saturating_add(l.border.left as i32),x.saturating_add(l.size.width as i32).saturating_sub(l.border.right as i32)));
        }
        if overflow.y!=taffy::style::Overflow::Visible {
            children_clip.y=BoxClip::intersect_axis(children_clip.y,(y.saturating_add(l.border.top as i32),y.saturating_add(l.size.height as i32).saturating_sub(l.border.bottom as i32)));
        }
        descendant_clips.push(children_clip);
        if entry.hidden {boxes.push(None);continue;}
        let border=[pixel(l.border.left),pixel(l.border.right),pixel(l.border.top),pixel(l.border.bottom)];
        let bw=pixel(l.size.width);let bh=pixel(l.size.height);
        let lines=entry.text.as_ref().map_or_else(Vec::new,|text|wrap(fonts,text,l.size.width));
        if !fixed {result.height=result.height.max(clip.y.map_or(y.saturating_add(bh as i32),|(_,end)|end.min(y.saturating_add(bh as i32))));}
        let node_style=&builder.tree.nodes[usize::from(entry.taffy)].style;
        boxes.push(Some(WebBox{node_id:entry.node_id,x,y,width:bw,height:bh,background:entry.background,border_color:entry.border_color,border,lines,input_data:entry.input_data.clone(),image:entry.image.clone(),highlights:Vec::new(),control_px:14.0,fixed,clip,font_size:entry.font_size,line_height:entry.line_height,color:entry.color,display:node_style.display,overflow,padding:[pixel(l.padding.left),pixel(l.padding.right),pixel(l.padding.top),pixel(l.padding.bottom)],box_sizing:node_style.box_sizing,position:entry.position,inline:entry.inline,atomic_inline:entry.atomic_inline,visible:entry.visible}));
    }
    for index in stacking_order(&builder.entries,&builder.tree) {
        if let Some(b)=boxes[index].take(){result.boxes.push(b);}
    }
    // White default canvas is a web default, independent of the viewer chrome theme.
    for b in &mut result.boxes {
        if b.node_id == Some(arena.root) {
            if b.background.is_none() { b.background = Some(Color::rgb(255,255,255)); }
            b.height = b.height.max(height);
            break;
        }
    }
    Ok(result)
}

/// Sort only the children of each stacking context, then emit each context as
/// an indivisible unit. A descendant's high z-index cannot escape its context.
fn stacking_order(entries:&[Entry],tree:&WebTree)->Vec<usize> {
    let mut contexts=Vec::with_capacity(entries.len());let mut owners=Vec::with_capacity(entries.len());
    let mut positioned=Vec::with_capacity(entries.len());
    let mut children:Vec<Vec<(i8,i32,usize,usize)>>=(0..entries.len()).map(|_|Vec::new()).collect();
    for (index,entry) in entries.iter().enumerate() {
        let item=entry.parent.is_some_and(|p|matches!(tree.nodes[usize::from(entries[p].taffy)].style.display,Display::Flex|Display::Grid));
        let context=index==0||entry.position==CssPosition::Fixed||(entry.z_index.is_some()&&(entry.position!=CssPosition::Static||item));
        contexts.push(context);
        let owner=entry.parent.map_or(0,|p|if contexts[p]{p}else{owners[p]});owners.push(owner);
        let raised=entry.position!=CssPosition::Static||entry.parent.is_some_and(|p|!contexts[p]&&positioned[p]);positioned.push(raised);
        if index==0{continue;}
        let z=if context{entry.z_index.unwrap_or(0)}else{0};
        let phase=if context&&z<0{-1}else if context&&z>0{2}else if context||raised{1}else{0};
        children[owner].push((phase,z,entry.paint_order,index));
    }
    for group in &mut children{group.sort_unstable();}
    fn emit(index:usize,contexts:&[bool],children:&[Vec<(i8,i32,usize,usize)>],out:&mut Vec<usize>){
        out.push(index);
        for &(_,_,_,child) in &children[index] {
            if contexts[child]{emit(child,contexts,children,out);}else{out.push(child);}
        }
    }
    let mut out=Vec::with_capacity(entries.len());if !entries.is_empty(){emit(0,&contexts,&children,&mut out);}out
}

impl WebLayout {
    pub fn scale(&mut self,scale:f32) {
        self.height=(self.height as f32*scale) as i32;
        for b in &mut self.boxes { b.x=(b.x as f32*scale) as i32;b.y=(b.y as f32*scale) as i32;b.width=(b.width as f32*scale) as u32;b.height=(b.height as f32*scale) as u32;
            for width in b.border.iter_mut().chain(b.padding.iter_mut()) {*width=(*width as f32*scale) as u32;} crate::doc::scale_lines(&mut b.lines,scale); }
        for b in &mut self.boxes {b.control_px*=scale;b.font_size*=scale;b.line_height*=scale;b.clip.scale(scale);}
    }
    fn paint_boxes(&self,view:(i32,i32,u32,u32),scroll:i32)->impl Iterator<Item=(&WebBox,i32,i32,(i32,i32,i32,i32))> {
        let (vx,vy,vw,vh)=view;let view_clip=(vx,vy,vx.saturating_add(vw as i32),vy.saturating_add(vh as i32));
        self.boxes.iter().filter_map(move|b|{
            if !b.visible { return None; }
            let scroll=if b.fixed{0}else{scroll};
            let clip=b.clip.screen(view_clip,scroll);
            let x=vx.saturating_add(b.x);let y=vy.saturating_add(b.y).saturating_sub(scroll);
            (clip.2>clip.0&&clip.3>clip.1&&x.saturating_add(b.width as i32)>clip.0&&x<clip.2&&y.saturating_add(b.height as i32)>clip.1&&y<clip.3).then_some((b,x,y,clip))
        })
    }
    pub fn render(&self,fonts:&mut TextFonts,surf:&mut ostd::display::ViSurface,view:(i32,i32,u32,u32),scroll:i32) {
        let boxes=self.paint_boxes(view,scroll);
        fn rect(surf:&mut ostd::display::ViSurface,x:i32,y:i32,w:u32,h:u32,color:Color,clip:(i32,i32,i32,i32)) {
            let x0=x.max(clip.0);let y0=y.max(clip.1);let x1=x.saturating_add(w as i32).min(clip.2);let y1=y.saturating_add(h as i32).min(clip.3);
            if x1>x0&&y1>y0 {crate::draw::fill_rect(surf,x0,y0,(x1-x0) as u32,(y1-y0) as u32,color);}
        }
        let (vx,vy,vw,vh) = view;
        rect(surf, vx, vy, vw, vh, Color::rgb(255,255,255), (vx, vy, vx.saturating_add(vw as i32), vy.saturating_add(vh as i32)));
        for (b,x,y,clip) in boxes {
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
    pub fn hit_link(&self,x:i32,y:i32,scroll:i32)->Option<String> {
        for b in self.boxes.iter().rev() {
            if !b.visible { continue; }
            let y=if b.fixed{y}else{y.saturating_add(scroll)};
            if !b.clip.contains(x,y){continue;}
            let mut line_y=b.y;
            for line in &b.lines {
                if y>=line_y && y<line_y+line.height as i32 {
                    for run in &line.runs {
                        let left=b.x as f32+run.x;
                        if (x as f32)>=left && (x as f32)<left+run.width {
                            return run.span.link.clone();
                        }
                    }
                }
                line_y+=line.height as i32;
            }
            if x>=b.x&&x<b.x.saturating_add(b.width as i32)&&y>=b.y&&y<b.y.saturating_add(b.height as i32)&&b.node_id.is_some(){return None;}
        }
        None
    }
    pub fn hit_input_node(&self,x:i32,y:i32,scroll:i32)->Option<NodeId>{
        let id=self.hit_node(x,y,scroll)?;
        self.boxes.iter().any(|b|b.node_id==Some(id)&&b.input_data.is_some()).then_some(id)
    }
    pub fn hit_node(&self,x:i32,y:i32,scroll:i32)->Option<NodeId> {
        for b in self.boxes.iter().rev() {
            if !b.visible { continue; }
            let y=if b.fixed{y}else{y.saturating_add(scroll)};
            if !b.clip.contains(x,y){continue;}
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

impl WebLayout {
    /// A committed native layout snapshot. Coordinates are viewport-relative,
    /// including fixed positioning; inline boxes use their measured run union.
    pub fn browser_layout(&self, arena: &DocumentArena, scroll: i32) -> Vec<dom_arena::BrowserNodeLayout> {
        use alloc::{collections::BTreeMap, format};
        use dom_arena::BrowserNodeLayout;
        fn styles(px: f32, line_height: f32, color: Color, display: &str) -> Vec<(String, String)> {
            alloc::vec![
                ("font-size".into(), format!("{px}px")),
                ("line-height".into(), format!("{line_height}px")),
                ("color".into(), format!("rgba({},{},{},{})", color.r, color.g, color.b, color.a as f32 / 255.0)),
                ("display".into(), display.into()),
            ]
        }
        fn overflow(value: taffy::style::Overflow) -> &'static str {
            match value {
                taffy::style::Overflow::Visible => "visible",
                taffy::style::Overflow::Hidden => "hidden",
                taffy::style::Overflow::Scroll => "scroll",
                taffy::style::Overflow::Clip => "clip",
            }
        }
        let mut layout = BTreeMap::<NodeId, BrowserNodeLayout>::new();
        for b in &self.boxes {
            if let Some(node) = b.node_id {
                let display = if b.atomic_inline { "inline-block" } else if b.inline { "inline" } else { match b.display {
                    Display::Block => "block", Display::FlowRoot => "flow-root", Display::Flex => "flex",
                    Display::Grid => "grid", Display::None => "none",
                }};
                let mut computed = styles(b.font_size, b.line_height, b.color, display);
                computed.push(("overflow-x".into(), overflow(b.overflow.x).into()));
                computed.push(("overflow-y".into(), overflow(b.overflow.y).into()));
                computed.push(("position".into(), match b.position { CssPosition::Static=>"static",CssPosition::Relative=>"relative",CssPosition::Absolute=>"absolute",CssPosition::Fixed=>"fixed" }.into()));
                computed.push(("box-sizing".into(), if b.box_sizing == BoxSizing::BorderBox {"border-box"} else {"content-box"}.into()));
                let content_width = b.width.saturating_sub(b.border[0]+b.border[1]+b.padding[0]+b.padding[1]);
                let content_height = b.height.saturating_sub(b.border[2]+b.border[3]+b.padding[2]+b.padding[3]);
                computed.push(("width".into(), if b.inline && !b.atomic_inline { "auto".into() } else { format!("{}px", if b.box_sizing == BoxSizing::BorderBox {b.width} else {content_width}) }));
                computed.push(("height".into(), if b.inline && !b.atomic_inline { "auto".into() } else { format!("{}px", if b.box_sizing == BoxSizing::BorderBox {b.height} else {content_height}) }));
                for (index,side) in ["left","right","top","bottom"].into_iter().enumerate() {
                    computed.push((format!("padding-{side}"),format!("{}px",b.padding[index])));
                    computed.push((format!("border-{side}-width"),format!("{}px",b.border[index])));
                }
                layout.insert(node, BrowserNodeLayout {
                    node, x: b.x as f32, y: b.y.saturating_sub(if b.fixed { 0 } else { scroll }) as f32,
                    width: b.width as f32, height: b.height as f32, styles: computed,
                });
            }
        }
        let elements: alloc::collections::BTreeSet<NodeId> = layout.keys().copied().collect();
        for b in &self.boxes {
            let mut y = b.y.saturating_sub(if b.fixed { 0 } else { scroll }) as f32;
            for line in &b.lines {
                for run in &line.runs {
                    let Some(node) = run.node_id else { continue; };
                    if elements.contains(&node) { continue; }
                    let x = b.x as f32 + run.x;
                    let right = x + run.width;
                    let bottom = y + line.height as f32;
                    let mut current = Some(node);
                    while let Some(id) = current {
                        if elements.contains(&id) { break; }
                        layout.entry(id).and_modify(|old| {
                            let right = (old.x + old.width).max(right);
                            let bottom = (old.y + old.height).max(bottom);
                            old.x = old.x.min(x); old.y = old.y.min(y);
                            old.width = right - old.x; old.height = bottom - old.y;
                        }).or_insert_with(|| BrowserNodeLayout {
                            node: id, x, y, width: run.width, height: line.height as f32,
                            styles: if id == node {
                                let mut computed = styles(run.px, run.line_height, run.span.color.unwrap_or(b.color), "inline");
                                computed.push(("width".into(),"auto".into())); computed.push(("height".into(),"auto".into()));
                                computed
                            } else { Vec::new() },
                        });
                        current = arena.get(id).and_then(|node| node.parent);
                    }
                }
                y += line.height as f32;
            }
        }
        layout.into_values().collect()
    }

    /// Synchronous CSSOM queries use this same cascade even when display:none
    /// or disconnection means the queried element has no generated box.
    pub fn browser_node_layout(&self, arena: &DocumentArena, external: &[(NodeId,String)], node: NodeId,
        width: u32, height: u32, scroll: i32) -> Result<dom_arena::BrowserNodeLayout,String> {
        use alloc::format;
        if !arena.get(node).is_some_and(|node| node.is_element()) { return Err("CSSOM requires an element".into()); }
        let mut sheet = Stylesheet::default();
        collect_styles(arena, arena.root, external, &mut sheet, (width,height));
        let mut ancestors = Vec::new();
        let mut cursor = Some(node);
        while let Some(id) = cursor {
            if ancestors.len() > crate::budget::MAX_DOM_DEPTH { return Err("CSSOM ancestry exceeds depth budget".into()); }
            ancestors.push(id);
            cursor = arena.get(id).and_then(|node| node.parent);
        }
        let mut parent = None;
        for id in ancestors.into_iter().rev() {
            parent = Some(sheet.compute(arena,id,parent.as_ref(),(width,height)));
        }
        let style = parent.ok_or("CSSOM style unavailable")?;
        let mut result = self.browser_layout(arena,scroll).into_iter().find(|entry| entry.node == node)
            .unwrap_or(dom_arena::BrowserNodeLayout { node, x:0.0,y:0.0,width:0.0,height:0.0,styles:Vec::new() });
        let display = if style.layout.display == Display::None { "none" } else if style.atomic_inline { "inline-block" } else if style.inline { "inline" } else {
            match style.layout.display { Display::Block => "block", Display::FlowRoot => "flow-root", Display::Flex => "flex", Display::Grid => "grid", Display::None => "none" }
        };
        let mut computed = alloc::vec![
            ("display".into(),display.into()),
            ("font-size".into(),format!("{}px",style.font_size)),
            ("line-height".into(),format!("{}px",style.line_height)),
            ("color".into(),format!("rgba({},{},{},{})",style.color.r,style.color.g,style.color.b,style.color.a as f32/255.0)),
            ("box-sizing".into(),if style.layout.box_sizing == BoxSizing::BorderBox { "border-box".into() } else { "content-box".into() }),
            ("position".into(),match style.position { CssPosition::Static=>"static",CssPosition::Relative=>"relative",CssPosition::Absolute=>"absolute",CssPosition::Fixed=>"fixed" }.into()),
        ];
        for (key,value) in core::mem::take(&mut result.styles) {
            if !computed.iter().any(|(name,_)| name == &key) { computed.push((key,value)); }
        }
        result.styles = computed;
        Ok(result)
    }
}

fn is_replaced(arena:&DocumentArena,id:NodeId)->bool {
    arena.get(id).is_some_and(|node|matches!(node.tag(),Some("input"|"textarea"|"button"|"img"|"select")))
}
