(()=>{
const h=(tag,attrs,...kids)=>{const el=document.createElement(tag);
for(const[k,v]of Object.entries(attrs||{})){if(v==null||v===false)continue;
if(k==='class')el.className=v;else if(k==='style'&&typeof v==='object')Object.assign(el.style,v);
else if(k==='html')el.innerHTML=v;else if(k.startsWith('on')&&typeof v==='function')el.addEventListener(k.slice(2).toLowerCase(),v);
else el.setAttribute(k,v===true?'':v)}
for(const kid of kids.flat(9))if(kid!=null&&kid!==false)el.append(kid instanceof Node?kid:String(kid));return el};
const ICONS={check:'M4 12l5 5L20 6',x:'M6 6l12 12M18 6L6 18',plus:'M12 5v14M5 12h14',minus:'M5 12h14',
'chevron-down':'M6 9l6 6 6-6','chevron-right':'M9 6l6 6-6 6','chevron-left':'M15 6l-6 6 6 6','chevron-up':'M6 15l6-6 6 6',
'arrow-right':'M5 12h14M13 6l6 6-6 6','arrow-left':'M19 12H5M11 6l-6 6 6 6',search:'M11 18a7 7 0 1 1 0-14 7 7 0 0 1 0 14zM20 20l-4-4',
settings:'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12h2M3 12h2M12 3v2M12 19v2M17 7l1.4-1.4M5.6 18.4L7 17M17 17l1.4 1.4M5.6 5.6L7 7',
user:'M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21a8 8 0 0 1 16 0',file:'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8zM14 3v5h5',
folder:'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z',trash:'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
copy:'M9 9h10v10H9zM5 15V5h10',edit:'M4 20h4L19 9l-4-4L4 16zM14 6l4 4',play:'M7 5l12 7-12 7z',pause:'M8 5v14M16 5v14',
replay:'M4 12a8 8 0 1 0 2.3-5.7M4 4v5h5',more:'M5 12h.01M12 12h.01M19 12h.01',menu:'M4 7h16M4 12h16M4 17h16',
bell:'M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 21h4',mail:'M3 6h18v12H3zM3 7l9 6 9-6',home:'M3 11l9-7 9 7M5 10v10h14V10',
star:'M12 3l2.8 5.8 6.2.9-4.5 4.4 1 6.3L12 17.5 6.5 20.4l1-6.3L3 9.7l6.2-.9z',heart:'M12 20s-8-5-8-11a4.5 4.5 0 0 1 8-2.8A4.5 4.5 0 0 1 20 9c0 6-8 11-8 11z',
link:'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
lock:'M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3',calendar:'M4 6h16v14H4zM4 10h16M9 3v4M15 3v4',clock:'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2',
download:'M12 4v12M6 11l6 6 6-6M5 20h14',upload:'M12 20V8M6 13l6-6 6 6M5 4h14',filter:'M4 5h16l-6 8v6l-4-1v-5z',
sun:'M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5L19 19M5 19l1.5-1.5M17.5 6.5L19 5',
moon:'M20 14A8 8 0 0 1 10 4a8 8 0 1 0 10 10z',info:'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01',
alert:'M12 3l10 18H2zM12 10v4M12 17h.01',sparkle:'M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z',
terminal:'M4 5h16v14H4zM8 10l3 2-3 2M13 15h3',code:'M9 8l-4 4 4 4M15 8l4 4-4 4',git:'M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9'};
const icon=(name,attrs)=>{const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('class','mc-icon'+(attrs&&attrs.class?' '+attrs.class:''));
if(attrs&&attrs.size){svg.style.width=svg.style.height=attrs.size+'px'}
const p=document.createElementNS('http://www.w3.org/2000/svg','path');p.setAttribute('d',ICONS[name]||ICONS.info);svg.append(p);return svg};
const root=t=>typeof t==='string'?document.querySelector(t):(t||document.body);
// One clock for the page. Speed and pause slow CSS and Web Animations, and the
// time scripts read (performance.now, Date.now, requestAnimationFrame
// timestamps) and the delay of their timers follow it, so script-driven motion
// slows with the rest.
const real=performance.now.bind(performance),realDate=Date.now,rAF=window.requestAnimationFrame.bind(window),rawTimeout=window.setTimeout,rawInterval=window.setInterval;
let speed=1,paused=false,virtual=real(),mark=virtual;const watchers=new Set();
const rate=()=>paused?0:speed;
const tick=()=>{const t=real();virtual+=(t-mark)*rate();mark=t;return virtual};
const pace=()=>{const r=rate();for(const a of document.getAnimations())if(a.playbackRate!==r)a.playbackRate=r};
addEventListener('animationstart',pace,true);addEventListener('transitionrun',pace,true);
const animate=Element.prototype.animate;
Element.prototype.animate=function(...args){const a=animate.apply(this,args);a.playbackRate=rate();return a};
performance.now=()=>tick();
Date.now=()=>realDate()+(tick()-real());
window.requestAnimationFrame=f=>rAF(()=>f(tick()));
window.setTimeout=(f,d,...a)=>rawTimeout(f,typeof f==='function'?(Number(d)||0)/Math.max(speed,.05):d,...a);
window.setInterval=(f,d,...a)=>rawInterval(f,typeof f==='function'?(Number(d)||0)/Math.max(speed,.05):d,...a);
const sync=()=>{document.documentElement.style.setProperty('--mc-speed',String(rate()));pace();watchers.forEach(f=>{try{f({speed,paused})}catch(e){console.error(e)}})};
const setSpeed=v=>{tick();speed=Math.max(.05,Number(v)||1);sync()};
const pause=on=>{tick();paused=on!==false;sync()};
const now=()=>tick();
const clock={get speed(){return speed},get paused(){return paused},setSpeed,pause,resume:()=>pause(false),now,
// Calls f now and whenever speed or pause changes; returns a stop function.
on(f){watchers.add(f);f({speed,paused});return()=>watchers.delete(f)}};
// Named states shared by anything that listens, for a trigger that flips a page
// between them: states(['expanded','collapsed']).
const states=(names,o={})=>{let i=Math.max(0,names.indexOf(o.initial));const subs=new Set();
const go=n=>{const j=typeof n==='number'?(n+names.length)%names.length:names.indexOf(n);if(j<0||j===i)return s;i=j;subs.forEach(f=>{try{f(names[i],i)}catch(e){console.error(e)}});return s};
const s={names,get name(){return names[i]},get index(){return i},go,next:()=>go(i+1),
// Calls f now and on every change; returns a stop function.
on(f){subs.add(f);try{f(names[i],i)}catch(e){console.error(e)}return()=>subs.delete(f)}};return s};
const slider=(label,o={})=>{const fmt=o.format||(v=>String(v)+(o.unit||''));
const input=h('input',{class:'mc-range',type:'range',min:o.min??0,max:o.max??100,step:o.step??1,value:o.value??0,'aria-label':label});
const out=h('output',null,fmt(Number(input.value)));
input.addEventListener('input',()=>{const v=Number(input.value);out.textContent=fmt(v);o.onInput&&o.onInput(v)});
const el=h('label',{class:'mc-field'},label,input,out);el.input=input;return el};
const toggle=(label,value,onChange)=>{const sw=h('button',{class:'mc-switch',role:'switch','aria-checked':String(!!value),'aria-label':label});
sw.addEventListener('click',()=>{const v=sw.getAttribute('aria-checked')!=='true';sw.setAttribute('aria-checked',String(v));onChange&&onChange(v)});
return h('label',{class:'mc-field'},sw,label)};
const tabs=(labels,selected,onChange)=>{const bar=h('div',{class:'mc-tabs',role:'tablist'});
const select=i=>{[...bar.children].forEach((b,j)=>b.setAttribute('aria-selected',String(i===j)));onChange&&onChange(i)};
labels.forEach((l,i)=>bar.append(h('button',{class:'mc-tab',role:'tab','aria-selected':String(i===selected),onclick:()=>select(i)},l)));
bar.select=i=>{[...bar.children].forEach((b,j)=>b.setAttribute('aria-selected',String(i===j)))};return bar};
const stagger=(els,start=0)=>{[...els].forEach((el,i)=>el.style.setProperty('--i',String(start+i)));return els};
// Variants side by side in one row, all alive at once, so one shared trigger
// (o.states, a transport) drives them together and they can be compared live.
const side=(target,list,o)=>{const cols=list.map((v,index)=>{const stage=h('div',{class:'mc-stage'+(o.stage?' '+o.stage:'')});
if(o.stageHeight)stage.style.minHeight=o.stageHeight+'px';
return{v,index,stage,cleanup:null,el:h('div',{class:'mc-side-col'},h('div',{class:'mc-side-title'},v.name),v.note?h('p',{class:'mc-side-note'},v.note):null,stage)}});
const draw=c=>{if(typeof c.cleanup==='function'){try{c.cleanup()}catch(e){console.error(e)}}c.cleanup=null;c.stage.replaceChildren();
try{c.cleanup=c.v.render(c.stage,{index:c.index,name:c.v.name,replay:()=>draw(c),speed:()=>speed,now,states:o.states,clock})}catch(e){console.error(e);c.stage.append(h('div',{class:'mc-muted'},String(e)))}};
const demo=h('div',{class:'mc-side'},cols.map(c=>c.el));
const mount=()=>{root(target).append(demo);cols.forEach(draw);requestAnimationFrame(pace)};
if(document.body)mount();else addEventListener('DOMContentLoaded',mount);
return{show:()=>cols.forEach(draw),replay:()=>cols.forEach(draw),stages:cols.map(c=>c.stage),demo}};
// A variant carousel: a stage with tabs, and optional replay and playback speed.
const variants=(target,list,o={})=>{if(o.layout==='side')return side(target,list,o);let index=Math.min(o.initial||0,list.length-1),cleanup=null;
const stage=h('div',{class:'mc-stage'+(o.stage?' '+o.stage:'')});
if(o.stageHeight)stage.style.minHeight=o.stageHeight+'px';
// The stage keeps the tallest height its content reached, so a replay or tab
// that starts smaller doesn't shrink it and move everything below. A new width
// reflows the content, so the floor starts over.
const base=stage.style.minHeight;let floorWidth=0;
const natural=()=>{const c=getComputedStyle(stage);return stage.clientHeight-parseFloat(c.paddingTop)-parseFloat(c.paddingBottom)};
new ResizeObserver(([e])=>{const r=e.contentRect;if(Math.abs(r.width-floorWidth)>1){floorWidth=r.width;stage.style.minHeight=base;stage.style.minHeight=natural()+'px';return}
if(r.height>(parseFloat(stage.style.minHeight)||0))stage.style.minHeight=r.height+'px'}).observe(stage);
const note=h('span',{class:'mc-note'});
const show=i=>{index=(i+list.length)%list.length;bar.select(index);if(typeof cleanup==='function'){try{cleanup()}catch(e){console.error(e)}}
cleanup=null;stage.replaceChildren();const v=list[index];note.textContent=v.note||'';
try{cleanup=v.render(stage,{index,name:v.name,replay:()=>show(index),speed:()=>speed,now,states:o.states,clock})}catch(e){console.error(e);stage.append(h('div',{class:'mc-muted'},String(e)))}
requestAnimationFrame(pace)};
const bar=tabs(list.map(v=>v.name),index,show);
// Chrome is opt-in: tabs only for several variants, Replay when there is
// something to replay, Playback only when asked for.
const replay=h('button',{class:'mc-button ghost small',title:'Replay',onclick:()=>show(index)},icon('replay',{size:14}),'Replay');
const wantReplay=o.replay===true||(o.replay!==false&&list.length>1);
const head=list.length>1?bar:o.title?h('span',{class:'mc-title'},o.title):null;
const top=head||o.controls||wantReplay?h('div',{class:'mc-demo-bar'},head,h('div',{class:'mc-spacer'}),o.controls||null,wantReplay?replay:null):null;
const foot=list.some(v=>v.note)||o.playback===true?h('div',{class:'mc-demo-foot'},note,o.playback===true?slider('Playback',{min:.1,max:2,step:.05,value:speed,format:v=>v.toFixed(2)+'×',onInput:setSpeed}):null):null;
const demo=h('div',{class:'mc-demo'},top,stage,foot);
demo.tabIndex=-1;demo.addEventListener('keydown',e=>{if(e.key==='ArrowRight')show(index+1);else if(e.key==='ArrowLeft')show(index-1);else if(e.key==='r')show(index)});
const mount=()=>{root(target).append(demo);show(index)};
if(document.body)mount();else addEventListener('DOMContentLoaded',mount);
if(o.autoplay){setInterval(()=>show(index),Math.max(800,o.autoplay))}
return{show,stage,demo,get index(){return index}}};
window.mc={h,icon,icons:Object.keys(ICONS),slider,toggle,tabs,stagger,variants,setSpeed,now,clock,states,get speed(){return speed},
series:i=>getComputedStyle(document.documentElement).getPropertyValue('--chart-'+((i%6)+1)).trim()};
})();
