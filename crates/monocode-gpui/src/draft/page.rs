//! The page a draft shows: the agent's HTML inside an opaque sandbox, themed
//! from the app, plus the scripts that let the app read the page and drive it
//! the way a person would (without ever being a person's input).
use monocode_core::visual_bridge::{FONT_HOSTS, SCRIPT_HOSTS, STYLE_HOSTS, csp_sources};

/// The colors a page is themed from: the app's own, so a mock of MonoCode
/// looks like MonoCode.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Palette {
    ink: gpui::Hsla,
    accent: gpui::Hsla,
    /// The content card's solid color.
    surface: gpui::Hsla,
    /// What menus and popovers are made of.
    popover: gpui::Hsla,
}
impl Palette {
    pub(crate) fn current() -> Self {
        Self {
            ink: crate::theme::content(1.),
            accent: crate::theme::accent_color(),
            surface: crate::theme::pane_surface(),
            popover: crate::theme::floating_fill(),
        }
    }
    /// A light or dark page, whatever the app is set to. The app's own theme
    /// is used when it already is that appearance; otherwise the defaults.
    pub(crate) fn forced(light: bool) -> Self {
        if light == crate::theme::is_light() {
            return Self::current();
        }
        let (ink, surface, popover) = if light {
            (0x171717, 0xffffff, 0xffffff)
        } else {
            (crate::theme::TEXT, crate::theme::CHAT, 0x1f1f1f)
        };
        Self {
            ink: gpui::rgb(ink).into(),
            accent: crate::theme::accent_color(),
            surface: gpui::rgb(surface).into(),
            popover: gpui::rgb(popover).into(),
        }
    }
    pub(crate) fn of(light: Option<bool>) -> Self {
        light.map_or_else(Self::current, Self::forced)
    }
    pub(crate) fn light(self) -> bool {
        let ink: gpui::Rgba = self.ink.into();
        ink.r * 0.2126 + ink.g * 0.7152 + ink.b * 0.0722 < 0.5
    }
}

fn hex(value: gpui::Hsla) -> String {
    let value: gpui::Rgba = value.into();
    format!(
        "#{:02x}{:02x}{:02x}",
        (value.r * 255.) as u8,
        (value.g * 255.) as u8,
        (value.b * 255.) as u8
    )
}

fn background(palette: Palette) -> String {
    hex(palette.surface)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The theme of a page as CSS variables. A page that is already open receives
/// a new set through `__mcTheme` instead of reloading.
///
/// Everything on the thread follows the app. A box marked `data-theme="light"`
/// or `"dark"` (a mock of a screen, a brand) keeps that appearance whatever the
/// app is in, and sets `color` again because text inherits the computed color,
/// not the variable. `:where` keeps the page's own rules for the box winning,
/// and `body ` leaves `html` and `body` out, so a pinned root cannot turn the
/// whole page against the thread. `data-mc-flip` is the other theme, which the
/// audit measures the page in for a moment.
fn tokens(palette: Palette) -> String {
    let (light, dark) = (Palette::forced(true), Palette::forced(false));
    let other = if palette.light() { dark } else { light };
    format!(
        ":root{{{}}}:root[data-mc-flip]{{{}}}:where(body [data-theme=light]){{{};color:var(--foreground)}}:where(body [data-theme=dark]){{{};color:var(--foreground)}}",
        declarations(palette),
        declarations(other),
        declarations(light),
        declarations(dark),
    )
}

/// One theme's variables.
///
/// The scale is the app's own: borders are the ink at 10%, hover and chip
/// fills 6%, cards 3%, muted text 60%, with 6, 8 and 12 point corners. Type
/// is 11 for captions, 12 for labels, 13 for body and 15 for section titles.
fn declarations(palette: Palette) -> String {
    let foreground = hex(palette.ink);
    let accent = hex(palette.accent);
    let background = background(palette);
    let popover = hex(palette.popover);
    let on_primary = &background;
    let scheme = if palette.light() { "light" } else { "dark" };
    let mix = |percent: u32| format!("color-mix(in srgb,{foreground} {percent}%,transparent)");
    let (muted_foreground, muted, card, secondary, border, input, code) =
        (mix(60), mix(6), mix(3), mix(6), mix(10), mix(12), mix(6));
    format!(
        "--background:{background};--foreground:{foreground};--muted-foreground:{muted_foreground};--muted:{muted};--card:{card};--card-foreground:{foreground};--popover:{popover};--popover-foreground:{foreground};--secondary:{secondary};--secondary-foreground:{foreground};--border:{border};--input:{input};--ring:{accent};--primary:{foreground};--primary-foreground:{on_primary};--accent:{accent};--accent-foreground:#ffffff;--accent-surface:color-mix(in srgb,{accent} 16%,transparent);--accent-surface-foreground:{accent};--destructive:#f43f5e;--destructive-foreground:#ffffff;--destructive-surface:color-mix(in srgb,#f43f5e 16%,transparent);--warning:#f59e0b;--warning-foreground:#1a1a1a;--warning-surface:color-mix(in srgb,#f59e0b 16%,transparent);--success:#10b981;--success-foreground:#ffffff;--info:#38bdf8;--info-foreground:#0b1a24;--code-background:{code};--code-foreground:{foreground};--surface:var(--card);--link:{accent};--selection:color-mix(in srgb,{accent} {selection}%,transparent);--positive:var(--success);--negative:var(--destructive);--chart-1:{accent};--chart-2:#10b981;--chart-3:#f59e0b;--chart-4:#f43f5e;--chart-5:#a78bfa;--chart-6:#22d3ee;--font-sans:-apple-system,BlinkMacSystemFont,system-ui,sans-serif;--font-mono:ui-monospace,'SF Mono',Menlo,monospace;--font-size:13px;--text-xs:11px;--text-sm:12px;--text-base:13px;--text-lg:15px;--text-xl:18px;--text-2xl:24px;--leading:1.5;--syntax-ink:{syntax_ink}%;--radius-sm:6px;--radius:8px;--radius-lg:12px;color-scheme:{scheme}",
        selection = if palette.light() { 20 } else { 35 },
        // The bright chart colors only read as text on a dark thread.
        syntax_ink = if palette.light() { 45 } else { 0 },
    )
}

/// The script that restyles an open page.
pub(crate) fn retheme_script(palette: Palette) -> String {
    format!(
        "window.__mcTheme&&__mcTheme({})",
        serde_json::Value::String(tokens(palette)),
    )
}

/// The color a picture of the page is laid on for the agent: the page itself
/// is clear, over the thread.
pub(crate) fn backdrop(palette: Palette) -> [u8; 3] {
    let value: gpui::Rgba = palette.surface.into();
    [value.r, value.g, value.b].map(|c| (c * 255.).round() as u8)
}

/// MonoCode's component kit: `mc-*` classes and the `window.mc` helpers that
/// `visual_docs {topic:"kit"}` describes.
const KIT_STYLE: &str = concat!(
    include_str!("kit/kit.css"),
    include_str!("kit/kit_viz.css"),
    include_str!("kit/kit_controls.css"),
    include_str!("kit/kit_motion.css")
);
const KIT_SCRIPT: &str = concat!(
    include_str!("kit/kit.js"),
    "\n",
    include_str!("kit/kit_viz.js"),
    "\n",
    include_str!("kit/kit_controls.js"),
    "\n",
    include_str!("kit/kit_motion.js")
);

/// Runs first inside the page. It reports console output, errors, load and
/// layout to the wrapper, answers inspection, and carries out the agent's
/// clicks and typing as script events: never as real input, so the person's
/// own focus and typing are left alone.
const PAGE_PROBE: &str = r#"<script>(()=>{const post=m=>{try{parent.postMessage(Object.assign({__mc:1},m),'*')}catch(e){}};
const text=a=>a.map(x=>{try{return typeof x==='string'?x:JSON.stringify(x)}catch(e){return String(x)}}).join(' ').slice(0,500);
for(const l of ['log','info','warn','error']){const o=console[l];console[l]=(...a)=>{post({k:'c',l,t:text(a)});o.apply(console,a)}}
addEventListener('error',e=>post({k:'c',l:'error',t:(e.message||'Script error')+(e.lineno?' (line '+e.lineno+')':'')}));
addEventListener('unhandledrejection',e=>post({k:'c',l:'error',t:'Unhandled rejection: '+text([e.reason])}));
addEventListener('securitypolicyviolation',e=>post({k:'c',l:'error',t:'Blocked by the page policy: '+e.blockedURI+' ('+e.violatedDirective+')'}));
const failed=(m,u,why)=>post({k:'c',l:'error',t:'Request failed: '+String(m||'GET').toUpperCase()+' '+String(u).slice(0,300)+' ('+why+')'});
const pageFetch=window.fetch;
window.fetch=function(i,o){const m=o&&o.method||i&&i.method,u=i&&i.url||i;return pageFetch.apply(this,arguments).then(r=>{if(r.status>=400)failed(m,u,r.status);return r},e=>{if(!e||e.name!=='AbortError')failed(m,u,e);throw e})};
const xhrOpen=XMLHttpRequest.prototype.open,xhrs=new WeakMap();
XMLHttpRequest.prototype.open=function(m,u){if(!xhrs.has(this)){this.addEventListener('error',()=>failed(...xhrs.get(this),'network error'));this.addEventListener('load',()=>{if(this.status>=400)failed(...xhrs.get(this),this.status)})}xhrs.set(this,[m,u]);return xhrOpen.apply(this,arguments)};
const metrics=()=>{const r=document.documentElement;if(!r)return;post({k:'m',w:r.scrollWidth,vw:r.clientWidth,fontsReady:document.fonts.status==='loaded',failedImages:Array.from(document.images).filter(i=>i.complete&&!i.naturalWidth).length})};
document.fonts.ready.then(metrics);
document.fonts.addEventListener('loadingdone',metrics);
addEventListener('load',()=>{metrics();post({k:'l'})});
post({k:'p',v:String(performance.timeOrigin)});
let mode=false,touched=-1e9;
// The real clock: the kit's slow motion scales performance.now later.
const realNow=performance.now.bind(performance);
// A page message reaches the agent only right after the person's own input,
// so a page cannot talk to the agent by itself.
window.__mcTell=t=>{if(realNow()-touched>2000)return false;post({k:'n',t:String(t).slice(0,2000)});return true};
const label=el=>{try{return String(el.getAttribute('aria-label')||(el.innerText||'').trim().replace(/\s+/g,' ').slice(0,60)||el.alt||el.placeholder||el.tagName.toLowerCase())}catch(x){return ''}};
addEventListener('pointerdown',e=>{if(e.isTrusted&&!mode){touched=realNow();const t=e.target;post({k:'u',a:t&&t.closest?'clicked '+JSON.stringify(label(t.closest('button,a,input,select,textarea,summary,[role]')||t)):''})}},true);
addEventListener('keydown',e=>{if(!e.isTrusted)return;const t=e.composedPath()[0];
// Escape the page leaves alone, outside a text field, closes the app's full view.
if(e.key==='Escape'&&!(t&&(t.isContentEditable||/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)))){setTimeout(()=>{if(!e.defaultPrevented)post({k:'x'})});return}
if(!mode){touched=realNow();post({k:'u'})}},true);
// The app lets the wheel through to the thread unless the pointer is over
// something here that scrolls, so it is told where the pointer is.
let wheelOwn=false;
const scrollable=t=>{for(let n=t;n&&n.nodeType===1&&n!==document.documentElement&&n!==document.body;n=n.parentElement){const s=getComputedStyle(n);if(/(auto|scroll)/.test(s.overflowY+s.overflowX)&&(n.scrollHeight>n.clientHeight+1||n.scrollWidth>n.clientWidth+1))return true}return false};
const wheelTo=v=>{if(v!==wheelOwn){wheelOwn=v;post({k:'w',v})}};
addEventListener('mousemove',e=>{try{wheelTo(scrollable(e.target))}catch(x){}},{passive:true,capture:true});
document.addEventListener('mouseleave',()=>wheelTo(false));
const css=(el,o)=>Object.assign(el.style,o);
const ui=(tag,o,text)=>{const el=document.createElement(tag);el.setAttribute('data-mc-ui','');css(el,o);if(text)el.textContent=text;return el};
const ink='var(--foreground,#fff)',sheet='var(--popover,#222)',line='var(--border,rgba(255,255,255,.14))';
let hl=null,box=null,note=null,pending=null,pins=0;const placed=[];
const frame=()=>{const r=document.documentElement;return {w:Math.max(r.scrollWidth,1),h:Math.max(r.scrollHeight,1)}};
const close=()=>{if(box){box.remove();box=null}pending=null};
// A pin sits on the element it was placed on: its child indexes from <body>
// and the point inside its box. Every screen loads the same page, so a pin
// from a page laid out at another width lands on the same element; one
// without an element falls back to its place as a percentage of the page.
const path=el=>{const b=document.body,out=[];while(el&&el!==b){const p=el.parentElement;if(!p)return '';out.unshift(Array.prototype.indexOf.call(p.children,el));el=p}const s=out.join('.');return el===b&&s&&s.length<=160?s:''};
const anchor=(el,x,y)=>{const p=path(el);if(!p)return null;const r=el.getBoundingClientRect();if(!(r.width>0&&r.height>0))return null;const c=v=>Math.round(Math.min(1,Math.max(0,v))*1000)/1000;return {path:p,ax:c((x-scrollX-r.left)/r.width),ay:c((y-scrollY-r.top)/r.height)}};
const on=a=>{if(!a||typeof a.path!=='string'||!Number.isFinite(a.ax)||!Number.isFinite(a.ay))return null;let el=document.body;for(const i of a.path.split('.')){el=el&&el.children[+i];if(!el)return null}const r=el.getBoundingClientRect();return r.width>0&&r.height>0?{x:r.left+scrollX+a.ax*r.width,y:r.top+scrollY+a.ay*r.height}:null};
const where=q=>{const a=on(q.at);if(a)return a;if(q.px==null)return q;const f=frame();return {x:q.px*f.w/100,y:q.py*f.h/100}};
const replace=()=>{for(const {p,q} of placed){const w=where(q);p.style.left=w.x-11+'px';p.style.top=w.y-11+'px'}};
const pinLayout=new ResizeObserver(replace);if(document.documentElement)pinLayout.observe(document.documentElement);
addEventListener('DOMContentLoaded',()=>{if(document.documentElement)pinLayout.observe(document.documentElement)});addEventListener('load',replace);
const pin=(q,n)=>{const w=where(q);const p=ui('div',{position:'absolute',left:w.x-11+'px',top:w.y-11+'px',width:'22px',height:'22px',borderRadius:'11px',background:'var(--accent,#4da3f5)',color:'#fff',font:'600 12px/22px system-ui',textAlign:'center',zIndex:'2147483646',pointerEvents:'none',boxShadow:'0 2px 8px rgba(0,0,0,.35)'},String(n));document.documentElement.appendChild(p);placed.push({p,q})};
const open=e=>{close();const el=e.target;pending={el,x:e.pageX,y:e.pageY};
box=ui('div',{position:'absolute',left:Math.max(8,Math.min(e.pageX+12,document.documentElement.clientWidth-268))+'px',top:e.pageY+12+'px',width:'256px',padding:'8px',background:sheet,color:ink,border:'1px solid '+line,borderRadius:'12px',boxShadow:'0 20px 25px -5px rgba(0,0,0,.25)',font:'13px system-ui',zIndex:'2147483647',display:'flex',flexDirection:'column',gap:'8px'});
note=ui('textarea',{width:'100%',boxSizing:'border-box',minHeight:'56px',resize:'none',background:'transparent',color:'inherit',border:'1px solid '+line,borderRadius:'8px',padding:'6px 8px',font:'inherit',fontSize:matchMedia('(hover:none)').matches?'16px':'inherit',outline:'none'});note.placeholder='What should change here?';
const row=ui('div',{display:'flex',gap:'6px',justifyContent:'flex-end'});
const cancel=ui('button',{background:'transparent',color:'inherit',border:'1px solid '+line,borderRadius:'6px',padding:'0 10px',height:'26px',font:'12px system-ui',cursor:'pointer'},'Cancel');
const add=ui('button',{background:ink,color:sheet,border:'0',borderRadius:'6px',padding:'0 10px',height:'26px',font:'12px system-ui',cursor:'pointer'},'Add');
const commit=()=>{const text=note.value.trim();if(!text){close();return}const el=pending.el,f=frame(),at=anchor(el,pending.x,pending.y);pins++;pin({x:pending.x,y:pending.y,at},pins);
post({k:'f',f:{note:text.slice(0,600),tag:el.tagName.toLowerCase(),name:label(el),x:Math.round(1000*pending.x/f.w)/10,y:Math.round(1000*pending.y/f.h)/10,at}});close()};
cancel.onclick=close;add.onclick=commit;
note.addEventListener('keydown',k=>{k.stopPropagation();if(k.key==='Enter'&&!k.shiftKey){k.preventDefault();commit()}else if(k.key==='Escape')close()},true);
row.append(cancel,add);box.append(note,row);document.documentElement.appendChild(box);note.focus()};
/* iOS clicks only what has a click handler of its own or an ancestor's below the window: one on the body makes all of it a place for a note. */
const tappable=()=>{};
const setMode=on=>{mode=!!on;if(!mode){close();if(hl)hl.style.display='none'}
if(document.body)document.body[mode?'addEventListener':'removeEventListener']('click',tappable);
let s=document.getElementById('__mcmode');if(on&&!s){s=document.createElement('style');s.id='__mcmode';s.setAttribute('data-mc-ui','');s.textContent='html,html *{cursor:crosshair!important}';document.head.appendChild(s)}else if(!on&&s)s.remove();
if(on&&!hl){hl=ui('div',{position:'fixed',pointerEvents:'none',border:'2px solid var(--accent,#4da3f5)',borderRadius:'4px',background:'rgba(77,163,245,.12)',zIndex:'2147483645',display:'none'});document.documentElement.appendChild(hl)}};
addEventListener('mousemove',e=>{if(!mode||pending||!hl)return;const t=e.target;if(!t||!t.closest||t.closest('[data-mc-ui]')||t===document.documentElement){hl.style.display='none';return}const r=t.getBoundingClientRect();css(hl,{display:'block',left:r.left+'px',top:r.top+'px',width:r.width+'px',height:r.height+'px'})},true);
for(const type of ['pointerdown','mousedown','mouseup','click']){addEventListener(type,e=>{if(!mode||!e.target.closest||e.target.closest('[data-mc-ui]'))return;e.preventDefault();e.stopPropagation();if(type==='click'&&!pending)open(e)},true)}
/* An element keeps its ref for as long as it is on the page, so the agent can act on several from one look. The prefix is per page load, so a ref from an earlier version never reaches a new element. */
const refs=new Map(),refOf=new WeakMap(),load=(Math.random()*1296|0).toString(36);let nextRef=0;
const rgba=c=>{let m=/rgba?\(([^)]+)\)/.exec(c);if(m){const p=m[1].split(/[ ,\/]+/).filter(Boolean).map(Number);return {r:p[0],g:p[1],b:p[2],a:p.length>3?p[3]:1}}
// color-mix() computes to color(srgb …) in WebKit, with channels from 0 to 1.
m=/color\(srgb ([^)]+)\)/.exec(c);if(m){const p=m[1].split(/[ \/]+/).filter(Boolean).map(Number);return {r:p[0]*255,g:p[1]*255,b:p[2]*255,a:p.length>3?p[3]:1}}return null};
const lum=c=>{const f=v=>{v/=255;return v<=.03928?v/12.92:Math.pow((v+.055)/1.055,2.4)};return .2126*f(c.r)+.7152*f(c.g)+.0722*f(c.b)};
const base=()=>{const v=getComputedStyle(document.documentElement).getPropertyValue('--background').trim(),m=/^#([0-9a-f]{6})$/i.exec(v);return m?{r:parseInt(m[1].slice(0,2),16),g:parseInt(m[1].slice(2,4),16),b:parseInt(m[1].slice(4,6),16)}:{r:255,g:255,b:255}};
const under=el=>{let bg=base();const chain=[];for(let n=el;n;n=n.parentElement)chain.push(n);for(const n of chain.reverse()){const c=rgba(getComputedStyle(n).backgroundColor);if(c&&c.a>0){bg=c.a>=1?{r:c.r,g:c.g,b:c.b}:{r:c.r*c.a+bg.r*(1-c.a),g:c.g*c.a+bg.g*(1-c.a),b:c.b*c.a+bg.b*(1-c.a)}}}return bg};
const measure=el=>{const fg=rgba(getComputedStyle(el).color);if(!fg)return null;const bg=under(el),a=fg.a<1?{r:fg.r*fg.a+bg.r*(1-fg.a),g:fg.g*fg.a+bg.g*(1-fg.a),b:fg.b*fg.a+bg.b*(1-fg.a)}:fg;const l1=lum(a),l2=lum(bg);return {fg:a,bg,ratio:(Math.max(l1,l2)+.05)/(Math.min(l1,l2)+.05)}};
const same=(a,b)=>Math.abs(a.r-b.r)+Math.abs(a.g-b.g)+Math.abs(a.b-b.b)<6;
// A page with its own prefers-color-scheme rules flips with the app's
// appearance, which the audit cannot change from inside the frame.
const ownScheme=()=>{const walk=rs=>{for(const r of rs){if(r.media&&/prefers-color-scheme/.test(r.media.mediaText))return true;if(r.cssRules&&walk(r.cssRules))return true}return false};
for(const s of document.styleSheets){if(s.ownerNode&&s.ownerNode.id==='mc-theme')continue;try{if(walk(s.cssRules))return true}catch(x){}}return false};
// The page is measured again under the other theme's tokens (data-mc-flip),
// so text that only reads in one of them is caught whichever the app is in.
const otherTheme=texts=>{const doc=document.documentElement;if(!texts.length||!document.getElementById('mc-theme')||ownScheme())return null;
const k=document.createElement('style');k.setAttribute('data-mc-ui','');k.textContent='*,*::before,*::after{transition:none!important;animation-play-state:paused!important}';(document.head||doc).appendChild(k);
try{doc.setAttribute('data-mc-flip','');return texts.map(t=>measure(t.el))}finally{doc.removeAttribute('data-mc-flip');getComputedStyle(document.body).color;k.remove()}};
const contrast=(texts,say)=>{const doc=document.documentElement,now=/light/.test(getComputedStyle(doc).colorScheme)?'light':'dark',other=now==='light'?'dark':'light',flipped=otherTheme(texts);
texts.forEach((t,i)=>{const a=t.m,b=flipped&&flipped[i],failA=a.ratio<t.min,failB=!!b&&b.ratio<t.min;if(!failA&&!failB)return;
const name=JSON.stringify(label(t.el)),theme=failA?now:other,where=b?' in '+theme:'';
if(b){const fixedInk=same(a.fg,b.fg),fixedFill=same(a.bg,b.bg);
if(t.el.closest('body [data-theme]')){if(fixedInk&&!fixedFill)return say('Text '+name+' is in a data-theme box but sits on the thread, so it fails in '+theme+'. Give the box its own background')}
else if(!fixedInk&&fixedFill)return say('Text '+name+' follows the app theme but its box does not, so it fails in '+theme+'. Put data-theme="light" or "dark" on the box, or give the text its own color');
else if(fixedInk&&!fixedFill)return say('Text '+name+' has a fixed color on the thread, so it fails in '+theme+'. Color it from a token such as var(--foreground) or var(--muted-foreground)')}
say('Low contrast '+(failA?a:b).ratio.toFixed(1)+':1 on '+name+where)})};
const audit=()=>{const out=[],say=t=>{if(out.length<12&&!out.includes(t))out.push(t)},texts=[];
if(stripped)say('The outermost element had a background in the app\'s own color; it was removed, because the page sits on the thread\'s background. Leave it off.');
const doc=document.documentElement;let seen=0;
for(const el of document.body?document.body.querySelectorAll('*'):[]){if(seen++>1500)break;if(el.closest('[data-mc-ui]'))continue;
const r=el.getBoundingClientRect();if(r.width<=0||r.height<=0)continue;const st=getComputedStyle(el);if(st.visibility==='hidden'||st.display==='none'||+st.opacity===0)continue;
const own=Array.from(el.childNodes).some(n=>n.nodeType===3&&n.textContent.trim());
if(own){const m=measure(el);if(m){const size=parseFloat(st.fontSize),big=size>=24||(size>=18.66&&+st.fontWeight>=700);texts.push({el,m,min:big?3:4.5})}}
const tag=el.tagName;const control=/^(BUTTON|A|INPUT|SELECT|TEXTAREA|SUMMARY)$/.test(tag)||el.getAttribute('role')==='button';
if(control&&!el.disabled&&(r.height<24||r.width<24)&&!(tag==='INPUT'&&/^(checkbox|radio)$/.test(el.type)))say('Small target '+Math.round(r.width)+'×'+Math.round(r.height)+' on '+JSON.stringify(label(el)));
if(/^(INPUT|SELECT|TEXTAREA)$/.test(tag)&&el.type!=='hidden'&&el.type!=='submit'&&el.type!=='button'&&!el.getAttribute('aria-label')&&!el.getAttribute('aria-labelledby')&&!(el.labels&&el.labels.length)&&!el.title)say('Field without a label'+(el.placeholder?' (placeholder '+JSON.stringify(el.placeholder)+')':''));
if((tag==='BUTTON'||el.getAttribute('role')==='button')&&!(el.innerText||'').trim()&&!el.getAttribute('aria-label')&&!el.title)say('Button without a name');
if(tag==='IMG'&&!el.hasAttribute('alt'))say('Image without alt text');
if(st.overflow!=='visible'&&el.scrollWidth>el.clientWidth+2&&own&&st.textOverflow!=='ellipsis'&&el.clientWidth>0)say('Text is clipped in '+JSON.stringify(label(el)))}
contrast(texts,say);
const f=frame();if(f.w>doc.clientWidth+2)say('The page is '+(f.w-doc.clientWidth)+'px wider than its viewport');
return out};
const fire=(el,type,C,init)=>el.dispatchEvent(new C(type,Object.assign({bubbles:true,cancelable:true,composed:true,view:window},init)));
const click=el=>{el.scrollIntoView({block:'nearest',inline:'nearest'});
const r=el.getBoundingClientRect(),i={button:0,buttons:1,pointerId:1,pointerType:'mouse',isPrimary:true,clientX:r.x+r.width/2,clientY:r.y+r.height/2};
fire(el,'pointerover',PointerEvent,i);fire(el,'mouseover',MouseEvent,i);fire(el,'pointermove',PointerEvent,i);fire(el,'mousemove',MouseEvent,i);
fire(el,'pointerdown',PointerEvent,i);const go=fire(el,'mousedown',MouseEvent,i);
if(go&&el.focus&&(el.isContentEditable||/^(TEXTAREA|SELECT)$/.test(el.tagName)||(el.tagName==='INPUT'&&!/^(checkbox|radio|button|submit|reset|range|color|file|image)$/.test(el.type))))el.focus({preventScroll:true});
i.buttons=0;fire(el,'pointerup',PointerEvent,i);fire(el,'mouseup',MouseEvent,i);el.click()};
const typing=(text,replace,ref)=>{if(ref!=null){const t=refs.get(ref);if(!t||!t.isConnected)return 'Element reference expired; inspect the draft again';if(document.activeElement!==t){t.scrollIntoView({block:'nearest',inline:'nearest'});if(t.focus)t.focus({preventScroll:true})}if(document.activeElement!==t&&!t.isContentEditable)return 'That element does not take typing'}
const el=document.activeElement;
if(!el||el===document.body||el===document.documentElement)return 'Pass the field\'s reference, or click an input before typing';
const edit=(type,data,inputType)=>fire(el,type,InputEvent,{data,inputType});
if(el.isContentEditable){if(replace)document.execCommand('selectAll');document.execCommand(text?'insertText':'delete',false,text||null);return ''}
if(el.tagName==='SELECT'){const o=Array.from(el.options).find(o=>o.text.trim()===text||o.value===text);if(!o)return 'No option matches '+JSON.stringify(text);el.value=o.value;fire(el,'input',Event);fire(el,'change',Event);return ''}
if(el.tagName!=='INPUT'&&el.tagName!=='TEXTAREA')return 'The focused element is not editable';
const set=Object.getOwnPropertyDescriptor(el.tagName==='INPUT'?HTMLInputElement.prototype:HTMLTextAreaElement.prototype,'value').set;
const typed=el.tagName==='TEXTAREA'||/^(text|search|email|url|tel|password)?$/.test(el.type);
if(!typed){set.call(el,text);edit('input',text,'insertText');fire(el,'change',Event);return ''}
if(replace){set.call(el,'');edit('input',null,'deleteContentBackward')}
for(const ch of text){const enter=ch==='\n',key={key:enter?'Enter':ch,bubbles:true,cancelable:true,composed:true};
if(el.dispatchEvent(new KeyboardEvent('keydown',key))){el.dispatchEvent(new KeyboardEvent('keypress',key));
if(enter&&el.tagName==='INPUT'){if(el.form)el.form.requestSubmit()}
else if(el.dispatchEvent(new InputEvent('beforeinput',{data:enter?null:ch,inputType:enter?'insertLineBreak':'insertText',bubbles:true,cancelable:true,composed:true}))){set.call(el,el.value+ch);edit('input',enter?null:ch,enter?'insertLineBreak':'insertText')}}
el.dispatchEvent(new KeyboardEvent('keyup',key))}
fire(el,'change',Event);return ''};
addEventListener('message',e=>{const d=e.data;if(e.source!==parent||!d)return;
if(typeof d.__mcComment==='boolean')setMode(d.__mcComment);
if(d.__mcState&&typeof d.__mcState==='object'&&typeof window.__mcSet==='function')window.__mcSet(d.__mcState);
if(d.__mcPin&&Number.isFinite(d.__mcPin.x)&&Number.isFinite(d.__mcPin.y)){pins++;pin({px:d.__mcPin.x,py:d.__mcPin.y,at:d.__mcPin.at},pins)}
if(typeof d.__mcTheme==='string'){const s=document.getElementById('mc-theme');if(s){const k=document.createElement('style');k.textContent='*,*::before,*::after{transition:none!important}';document.head.appendChild(k);s.textContent=d.__mcTheme;getComputedStyle(document.body).color;setTimeout(()=>k.remove(),80)}}
if(d.__mcAct){const a=d.__mcAct;let err='';try{if(a.kind==='click'){const el=refs.get(a.ref);if(!el||!el.isConnected)err='Element reference expired; inspect the draft again';else click(el)}else if(a.kind==='type')err=typing(String(a.text),!!a.replace,a.ref);else if(a.kind==='set')err=typeof window.__mcSet==='function'?window.__mcSet(a.values)||'':'The page has no mc.state to set'}catch(x){err=String(x&&x.message||x)}
post({k:'r',id:a.id,err})}
if(typeof d.__mcInspect==='string'){for(const[k,el]of refs)if(!el.isConnected)refs.delete(k);
const elements=[];for(const el of document.querySelectorAll('button,input:not([type=hidden]),select,textarea,a[href],summary,[role=button],[role=switch],[role=tab],[role=checkbox],[role=menuitem],[contenteditable=""],[contenteditable=true],[tabindex]:not([tabindex="-1"])')){
const r=el.getBoundingClientRect(),style=getComputedStyle(el);if(r.width<=0||r.height<=0||style.visibility==='hidden'||style.display==='none'||el.disabled||el.closest('[data-mc-ui]'))continue;
const name=el.getAttribute('aria-label')||(el.labels&&Array.from(el.labels).map(l=>l.textContent).join(' '))||el.textContent||el.placeholder||el.tagName;
let ref=refOf.get(el);if(!ref){ref=load+'-'+(++nextRef);refOf.set(el,ref)}refs.set(ref,el);
elements.push({ref,name:String(name).trim().replace(/\s+/g,' ').slice(0,160),role:el.getAttribute('role')||el.tagName.toLowerCase(),x:r.x,y:r.y,width:r.width,height:r.height,value:el.getAttribute('aria-checked')??el.getAttribute('aria-selected')??(typeof el.value==='string'?el.value:null)});if(elements.length>=128)break}
metrics();post({k:'s',id:d.__mcInspect,elements,issues:audit(),text:(document.body.innerText||'').slice(0,8000),url:location.href,title:document.title})}});
const outline=()=>{const out=[],y0=-document.documentElement.getBoundingClientRect().top;let seen=0;
const put=(k,b,n)=>out.push([k,Math.round(b.left),Math.round(b.top+y0),Math.round(b.width),Math.round(b.height),n]);
const walk=el=>{if(out.length>=64||seen++>2000||el.hasAttribute('data-mc-ui'))return;const st=getComputedStyle(el);if(st.display==='none'||st.visibility==='hidden'||+st.opacity===0)return;
const b=el.getBoundingClientRect(),tag=el.tagName.toUpperCase();
if(b.width>=4&&b.height>=4){if(/^(IMG|SVG|CANVAS|VIDEO|PICTURE|IFRAME|OBJECT)$/.test(tag)){put('m',b,1);return}
if(tag==='TABLE'){put('g',b,Math.max(1,el.rows?el.rows.length:1));return}
if(Array.from(el.childNodes).some(n=>n.nodeType===3&&n.textContent.trim())){const lh=parseFloat(st.lineHeight)||parseFloat(st.fontSize)*1.4||18;put('t',b,Math.max(1,Math.min(99,Math.round(b.height/lh))));return}
const c=rgba(st.backgroundColor);if((c&&c.a>.04)||parseFloat(st.borderTopWidth)>0)put('b',b,1)}
for(const c of el.children)walk(c)};
if(document.body)for(const c of document.body.children)walk(c);return out};
// A full-bleed box painted in the app's own background would sit as a slab on
// the thread; the page is clear, so such a fill is taken off and reported.
let stripped=0;
const clearFills=()=>{const d=document.documentElement,b=document.body;if(!b)return;const cs=getComputedStyle(d),tones=['--background','--popover'].map(k=>/^#([0-9a-f]{6})$/i.exec(cs.getPropertyValue(k).trim())).filter(Boolean).map(m=>({r:parseInt(m[1].slice(0,2),16),g:parseInt(m[1].slice(2,4),16),b:parseInt(m[1].slice(4,6),16)})),vw=d.clientWidth,vh=Math.max(b.getBoundingClientRect().height,1);
for(const el of b.children){if(el.hasAttribute('data-mc-ui')||/^(SCRIPT|STYLE)$/.test(el.tagName))continue;const r=el.getBoundingClientRect();if(r.width<vw*.95||r.height<vh*.9)continue;const c=rgba(getComputedStyle(el).backgroundColor);if(c&&c.a>=.9&&tones.some(t=>Math.abs(t.r-c.r)<=2&&Math.abs(t.g-c.g)<=2&&Math.abs(t.b-c.b)<=2)){el.style.setProperty('background-color','transparent','important');stripped++}}};
const h=()=>{const r=document.documentElement;if(!r)return;try{clearFills()}catch(e){}post({k:'h',v:Math.ceil(r.scrollHeight>r.clientHeight?r.scrollHeight:r.getBoundingClientRect().height)});metrics();try{post({k:'o',v:outline()})}catch(e){}};
const o=new ResizeObserver(h);if(document.documentElement)o.observe(document.documentElement);
addEventListener('DOMContentLoaded',()=>{if(document.documentElement)o.observe(document.documentElement);if(document.body)o.observe(document.body);h()});addEventListener('load',h);})();</script>"#;

/// The wrapper keeps the probe's reports where the app can read them.
const WRAPPER_SCRIPT: &str = r#"<script>window.__mc={log:[],loaded:false,h:0,w:0,vw:0,fontsReady:false,failedImages:0,userControl:false,wheel:false,feedback:[],messages:[],trail:[],issues:[]};
const __frame=()=>document.querySelector('iframe')&&document.querySelector('iframe').contentWindow;
window.__mcTheme=t=>{const m=/color-scheme:(\w+)/.exec(t);if(m)document.documentElement.style.colorScheme=m[1];const f=__frame();if(f)f.postMessage({__mcTheme:t},'*')};
window.__mcSend=m=>{const f=__frame()||window;f.postMessage(m,'*')};
addEventListener('message',e=>{const d=e.data,s=window.__mc;if(!d||d.__mc!==1)return;
if(d.k==='c'){if(s.log.length<50)s.log.push({level:String(d.l).slice(0,8),text:String(d.t).slice(0,500)})}
else if(d.k==='l')s.loaded=true;
else if(d.k==='w')s.wheel=d.v===true;
else if(d.k==='p')s.page=d.v;
else if(d.k==='u'){s.userControl=true;if(d.a&&s.trail.length<40)s.trail.push(String(d.a).slice(0,100))}
else if(d.k==='v'&&d.v&&typeof d.v==='object')s.state=d.v;
else if(d.k==='f'){if(s.feedback.length<50&&d.f)s.feedback.push(d.f)}
else if(d.k==='n'){if(s.messages.length<20&&typeof d.t==='string')s.messages.push(d.t.slice(0,2000))}
else if(d.k==='r')s.result={id:d.id,err:d.err||''};
else if(d.k==='s'){s.inspection=d.id;s.issues=d.issues||[];s.elements=d.elements;s.text=d.text;s.url=d.url;s.title=d.title}
else if(d.k==='m'){s.w=d.w;s.vw=d.vw;s.fontsReady=d.fontsReady;s.failedImages=d.failedImages}
else if(d.k==='h'&&Number.isFinite(d.v))s.h=Math.min(d.v,20000);
else if(d.k==='o'&&Array.isArray(d.v))s.outline=d.v.slice(0,64);
else if(d.k==='x')s.escape=true;
if(d.k!=='c')try{webkit.messageHandlers.mcWake.postMessage(0)}catch(x){try{__mcWake('')}catch(y){}}});</script>"#;

/// The probe and wrapper as one user script for a page the app does not
/// author, such as a dev server: they share the top window.
pub(crate) fn browser_probe() -> String {
    let wrapper = WRAPPER_SCRIPT
        .trim_start_matches("<script>")
        .trim_end_matches("</script>")
        .replace(
            "if(!d||d.__mc!==1)return;",
            "if(e.source!==window||!d||d.__mc!==1)return;",
        );
    format!(
        "{wrapper}{}",
        PAGE_PROBE
            .trim_start_matches("<script>")
            .trim_end_matches("</script>")
            // In the top window the parent is the window itself.
            .replace("e.source!==parent", "e.source!==window")
    )
}

/// The agent's page inside a sandboxed iframe, clear over the thread.
/// Nothing in it can reach the app or the filesystem. It can call HTTPS and
/// WSS servers, from a null origin and without cookies.
pub(crate) fn sandbox(body: &str, palette: Palette) -> String {
    let theme = tokens(palette);
    let policy = format!(
        "script-src 'unsafe-inline' 'wasm-unsafe-eval' {}; style-src 'unsafe-inline' {}; img-src data: blob: https:; font-src data: {}; media-src data: blob: https:",
        csp_sources(&SCRIPT_HOSTS),
        csp_sources(&STYLE_HOSTS),
        csp_sources(&FONT_HOSTS),
    );
    let csp = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; {policy}; worker-src blob:; connect-src https: wss:; frame-src 'none'; base-uri 'none'; form-action 'none'\"><meta name=viewport content=\"width=device-width,initial-scale=1\">"
    );
    let source = format!(
        "<!doctype html>{csp}<style id=mc-theme>{theme}</style><style>*{{box-sizing:border-box}}html{{background:transparent!important;scrollbar-width:none}}html::-webkit-scrollbar{{display:none}}:where(:focus-visible){{outline:2px solid var(--ring);outline-offset:-2px}}body{{margin:0;font:var(--font-size)/1.5 var(--font-sans);color:var(--foreground);background:transparent!important;-webkit-font-smoothing:antialiased}}a{{color:var(--link)}}::selection{{background:var(--selection)}}{KIT_STYLE}</style>{PAGE_PROBE}<script>{KIT_SCRIPT}</script><body>{body}"
    );
    // The frame and its page share one color scheme: a browser paints an
    // opaque backdrop behind a frame whose scheme differs from its parent's.
    let scheme = if palette.light() { "light" } else { "dark" };
    format!(
        "<!doctype html><style>:root{{color-scheme:{scheme}}}html,body,iframe{{margin:0;border:0;width:100%;height:100%;overflow:hidden;background:transparent}}</style>{WRAPPER_SCRIPT}<iframe sandbox=\"allow-scripts\" referrerpolicy=\"no-referrer\" srcdoc=\"{}\"></iframe>",
        escape(&source)
    )
}

/// Passes what the page tells the wrapper on to the phone app: state, notes,
/// the person's input, page messages and the page's height. The phone's web
/// view has no other way to read it. WebKit on iOS sends touches only where the
/// outer document listens for them, and the sandboxed page's own listeners do
/// not count: without these, a tap reached the page as a bare click, so nothing
/// but a slider saw `pointerdown` and `mc.send` refused every button.
const PHONE_BRIDGE: &str = r#"<script>for(const type of ['touchstart','pointerdown'])addEventListener(type,()=>{},{passive:true});addEventListener('message',e=>{const d=e.data;if(!d||d.__mc!==1||!/^[vfunh]$/.test(d.k))return;try{window.ReactNativeWebView.postMessage(JSON.stringify(d))}catch(x){}});</script>"#;

/// The same sandboxed page for the phone's web view, laid out at the phone's
/// width. The phone drives it with `__mcSend`, as the desktop does.
pub(crate) fn phone(body: &str, palette: Palette) -> String {
    sandbox(body, palette).replacen(
        "<!doctype html>",
        &format!(
            "<!doctype html><meta name=viewport content=\"width=device-width,initial-scale=1,viewport-fit=cover\">{PHONE_BRIDGE}"
        ),
        1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_markup_cannot_escape_the_opaque_iframe() {
        let html = sandbox(
            "\"><script>window.top.postMessage('x','*')</script>",
            Palette::forced(false),
        );
        assert!(html.contains("sandbox=\"allow-scripts\""));
        assert!(!html.contains("allow-same-origin"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn pages_load_code_only_from_listed_cdns_and_connect_only_over_tls() {
        let page = sandbox("<p>x</p>", Palette::forced(false));
        assert!(
            page.contains("script-src 'unsafe-inline' 'wasm-unsafe-eval' https://cdn.jsdelivr.net")
        );
        assert!(!page.contains("'wasm-unsafe-eval' https:;"));
        // WebAssembly may compile, but strings still may not run as code.
        assert!(!page.contains("'unsafe-eval'"));
        assert!(page.contains("connect-src https: wss:;"));
        assert!(page.contains("img-src data: blob: https:;"));
        assert!(page.contains("media-src data: blob: https:;"));
        assert!(page.contains("frame-src 'none'"));
        assert!(page.contains("form-action 'none'"));
    }

    #[test]
    fn open_pages_are_rethemed_in_place_with_the_tokens_a_new_page_starts_with() {
        let dark = Palette::forced(false);
        let light = Palette::forced(true);
        let page = sandbox("<p>x</p>", dark);
        assert!(page.contains(&escape(&format!(
            "<style id=mc-theme>{}</style>",
            tokens(dark)
        ))));
        // The wrapper takes the first color-scheme as the page's own.
        let scheme = |palette| {
            let tokens = tokens(palette);
            let at = tokens.find("color-scheme:").unwrap() + "color-scheme:".len();
            tokens[at..at + 4].to_string()
        };
        assert_eq!(scheme(light), "ligh");
        assert_eq!(scheme(dark), "dark");
        let script = retheme_script(light);
        assert!(script.starts_with("window.__mcTheme&&__mcTheme(\""));
        assert!(script.contains("--background:#ffffff"));
        // The page is clear over the thread in either theme.
        assert!(page.contains("background:transparent!important;-webkit-font-smoothing"));
        assert_eq!(backdrop(light), [255, 255, 255]);
    }

    /// A syntax error anywhere in a script stops all of it, so an inspection
    /// waits for an answer that never comes. QuickJS parses each one, wrapped
    /// in a function that is never called, so nothing needs a DOM.
    #[test]
    fn every_page_script_parses() {
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        let strip = |s: &str| {
            s.trim_start_matches("<script>")
                .trim_end_matches("</script>")
                .to_string()
        };
        for (name, source) in [
            ("probe", strip(PAGE_PROBE)),
            ("wrapper", strip(WRAPPER_SCRIPT)),
            ("phone bridge", strip(PHONE_BRIDGE)),
            ("browser probe", browser_probe()),
            ("retheme", retheme_script(Palette::forced(true))),
        ] {
            context.with(|ctx| {
                let result = ctx.eval::<(), _>(format!("(function(){{\n{source}\n}})"));
                if result.is_err() {
                    let error = ctx.catch();
                    panic!("the {name} script does not parse: {error:?}");
                }
            });
        }
    }

    #[test]
    fn a_pinned_box_keeps_its_theme_and_the_audit_can_flip_the_rest() {
        for palette in [Palette::forced(false), Palette::forced(true)] {
            let theme = tokens(palette);
            let current = declarations(palette);
            let other = declarations(Palette::forced(!palette.light()));
            assert!(theme.starts_with(&format!(":root{{{current}}}")));
            assert!(theme.contains(&format!(":root[data-mc-flip]{{{other}}}")));
            // Pinned boxes are the same in either app theme, re-declare the
            // inherited color, lose to the page's own rules and skip the root.
            for (name, light) in [("light", true), ("dark", false)] {
                let pinned = declarations(Palette::forced(light));
                assert!(theme.contains(&format!(
                    ":where(body [data-theme={name}]){{{pinned};color:var(--foreground)}}"
                )));
            }
            assert!(theme.contains("--text-lg:15px"));
        }
        // The audit measures the other theme with transitions held still.
        assert!(PAGE_PROBE.contains("doc.setAttribute('data-mc-flip','')"));
        assert!(PAGE_PROBE.contains("transition:none!important"));
    }

    #[test]
    fn the_phone_gets_the_same_sandbox_with_a_bridge_to_the_app() {
        let page = phone("<p>x</p>", Palette::forced(false));
        assert!(page.starts_with("<!doctype html><meta name=viewport"));
        assert!(page.contains("ReactNativeWebView.postMessage"));
        assert!(page.contains("sandbox=\"allow-scripts\""));
        assert!(page.contains("connect-src https: wss:;"));
        // Only the outer document is changed; the page itself is not.
        assert_eq!(page.matches(PHONE_BRIDGE).count(), 1);
    }

    #[test]
    fn the_browser_probe_reads_its_own_window_not_a_parent() {
        let probe = browser_probe();
        assert!(probe.contains("e.source!==window||!d||d.__mc!==1"));
        assert!(!probe.contains("e.source!==parent"));
        assert!(!probe.contains("<script>"));
    }

    #[test]
    fn the_page_carries_the_optional_controls_kit() {
        assert!(KIT_SCRIPT.contains("Object.assign(mc,{state,controls})"));
        assert!(KIT_STYLE.contains(".mc-ctl.rail"));
        // The wrapper keeps what the page reports for the agent's snapshot.
        assert!(WRAPPER_SCRIPT.contains("d.k==='v'"));
        assert!(WRAPPER_SCRIPT.contains("d.k==='n'"));
        // The agent sets state by name, and the page speaks only after input.
        assert!(KIT_SCRIPT.contains("window.__mcSet="));
        assert!(PAGE_PROBE.contains("a.kind==='set'"));
        // Another screen's state and pins reach the page by name, not as input.
        assert!(PAGE_PROBE.contains("window.__mcSet(d.__mcState)"));
        assert!(PAGE_PROBE.contains("pin({px:d.__mcPin.x,py:d.__mcPin.y,at:d.__mcPin.at},pins)"));
        // Pins follow their element when the page lays out again.
        assert!(PAGE_PROBE.contains("new ResizeObserver(replace)"));
        // The probe can run before a dev server's page has a root.
        assert!(!PAGE_PROBE.contains("new ResizeObserver(replace).observe"));
        assert!(PAGE_PROBE.contains("realNow()-touched>2000"));
    }
}
