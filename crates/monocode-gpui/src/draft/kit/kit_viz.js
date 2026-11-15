(()=>{
const {h,icon}=mc;
const NS='http://www.w3.org/2000/svg';
const s=(tag,attrs,...kids)=>{const e=document.createElementNS(NS,tag);
for(const[k,v]of Object.entries(attrs||{}))if(v!=null&&v!==false)e.setAttribute(k,String(v));
for(const c of kids.flat(9))if(c!=null&&c!==false)e.append(c instanceof Node?c:String(c));return e};
const root=t=>typeof t==='string'?document.querySelector(t):(t||document.body);
const mount=(target,el)=>{const go=()=>root(target).append(el);if(document.body)go();else addEventListener('DOMContentLoaded',go)};
const tint=(c,i)=>c?(c.startsWith('--')?`var(${c})`:c):`var(--chart-${(i%6)+1})`;
const short=v=>{if(typeof v!=='number'||!isFinite(v))return String(v);const a=Math.abs(v);
const t=(x,d)=>x.toFixed(d).replace(/\.0+$/,'');
return a>=1e9?t(v/1e9,a>=1e10?0:1)+'B':a>=1e6?t(v/1e6,a>=1e7?0:1)+'M':a>=1e4?t(v/1e3,0)+'k':a>=1e3?t(v/1e3,1)+'k':Number.isInteger(v)?String(v):t(v,a<1?2:1)};
const formatter=f=>typeof f==='function'?f:typeof f==='string'?(f.startsWith('$')||f.startsWith('€')||f.startsWith('£')?v=>f+short(v):v=>short(v)+f):short;
const nice=(lo,hi,count)=>{if(!(hi>lo)){hi=lo+1}const raw=(hi-lo)/Math.max(1,count),mag=10**Math.floor(Math.log10(raw)),r=raw/mag;
const step=(r>=7.5?10:r>=3.5?5:r>=1.5?2:1)*mag,a=Math.floor(lo/step+1e-9)*step,b=Math.ceil(hi/step-1e-9)*step,ticks=[];
for(let v=a;v<=b+step/2;v+=step)ticks.push(+v.toPrecision(12));return{lo:a,hi:b,ticks}};
const textWidth=t=>String(t).length*6.6+4;

// Charts: line, area, bar, stacked, hbar, scatter, donut. SVG, themed, responsive.
const chart=(target,spec)=>{
const el=h('div',{class:'mc-chart'}),legend=h('div',{class:'mc-legend'}),plot=h('div',{class:'mc-chart-plot'}),tip=h('div',{class:'mc-tip',role:'status'});
el.append(plot,tip);let cur=spec,hidden=new Set(),first=true,width=0;
const hideTip=()=>{tip.style.opacity='0'};
const showTip=(x,y,html)=>{tip.innerHTML=html;tip.style.opacity='1';const w=tip.offsetWidth,W=plot.clientWidth;
tip.style.left=Math.max(0,Math.min(W-w,x+12))+'px';tip.style.top=Math.max(0,y-tip.offsetHeight-10)+'px'};
const row=(c,name,value)=>`<div class="mc-tip-row"><i style="background:${c}"></i><span>${esc(name)}</span><b>${esc(value)}</b></div>`;
const draw=()=>{const o=cur,type=o.type||'line',H=o.height||(type==='donut'?220:240),W=Math.max(220,plot.clientWidth||600);width=W;
plot.style.height=H+'px';plot.replaceChildren();hideTip();
const series=(o.series||[]).map((x,i)=>({...x,color:tint(x.color,i),i})).filter(x=>!hidden.has(x.i));
const all=(o.series||[]).map((x,i)=>({...x,color:tint(x.color,i),i}));
const yf=formatter(o.y&&o.y.format),xf=formatter(o.x&&o.x.format),anim=first&&o.animate!==false;first=false;
legend.replaceChildren(...((all.length>1||type==='donut')&&o.legend!==false?(type==='donut'?(o.labels||[]).map((l,i)=>[l,tint(o.colors&&o.colors[i],i),i]):all.map(x=>[x.name||'Series '+(x.i+1),x.color,x.i])).map(([name,c,i])=>{
const b=h('button',{class:'mc-legend-item'+(hidden.has(i)?' off':''),type:'button'},h('i',{style:{background:c}}),name);
b.onclick=()=>{hidden.has(i)?hidden.delete(i):hidden.add(i);draw()};return b}):[]));
const svg=s('svg',{width:W,height:H,viewBox:`0 0 ${W} ${H}`,class:anim?'mc-animate':''});plot.append(svg);
if(type==='donut'){donut(svg,o,W,H,anim);return}
const labels=o.labels||[],hor=type==='hbar',scatter=type==='scatter',stacked=type==='stacked';
let vals=[];for(const x of series)for(const [j,v]of(x.values||[]).entries())vals.push(scatter?v[1]:stacked?series.reduce((a,y)=>a+Math.max(0,+(y.values[j])||0),0):v);
vals=vals.filter(v=>typeof v==='number'&&isFinite(v));
const zero=type!=='line'&&type!=='scatter'||(o.y&&o.y.zero);
let lo=Math.min(...vals,zero?0:Infinity),hi=Math.max(...vals,zero?0:-Infinity);
if(!isFinite(lo)){lo=0;hi=1}if(o.y&&o.y.min!=null)lo=o.y.min;if(o.y&&o.y.max!=null)hi=o.y.max;
if(!zero&&hi>lo){const pad=(hi-lo)*.08;lo-=pad;hi+=pad}
const yn=nice(lo,hi,Math.max(2,Math.round(H/56)));
const top=o.y&&o.y.label?28:8,bottom=(hor?22:24)+(o.x&&o.x.label?18:0);
const left=hor?Math.min(W*.35,Math.max(...labels.map(textWidth),20)+8):Math.max(...yn.ticks.map(t=>textWidth(yf(t))),18)+6,right=12;
const pw=W-left-right,ph=H-top-bottom;
if(o.y&&o.y.label)svg.append(s('text',{x:hor?left:0,y:12,class:'mc-axis-label'},o.y.label));
if(o.x&&o.x.label)svg.append(s('text',{x:left+pw/2,y:H-2,class:'mc-axis-label','text-anchor':'middle'},o.x.label));
const vy=v=>hor?left+(v-yn.lo)/(yn.hi-yn.lo)*pw:top+ph-(v-yn.lo)/(yn.hi-yn.lo)*ph;
const grid=s('g',{class:'mc-grid'});svg.append(grid);
for(const t of yn.ticks){if(hor){const x=vy(t);grid.append(s('line',{x1:x,x2:x,y1:top,y2:top+ph}),s('text',{x,y:top+ph+15,'text-anchor':'middle'},yf(t)))}
else{const y=vy(t);grid.append(s('line',{x1:left,x2:left+pw,y1:y,y2:y}),s('text',{x:left-6,y:y+4,'text-anchor':'end'},yf(t)))}}
if(scatter){let xs=[];for(const x of series)for(const p of x.values||[])xs.push(p[0]);
const xn=nice(o.x&&o.x.min!=null?o.x.min:Math.min(...xs),o.x&&o.x.max!=null?o.x.max:Math.max(...xs),Math.max(2,Math.round(W/90)));
const vx=v=>left+(v-xn.lo)/(xn.hi-xn.lo)*pw;
for(const t of xn.ticks)grid.append(s('text',{x:vx(t),y:top+ph+15,'text-anchor':'middle'},xf(t)));
const pts=[];const g=s('g',{});svg.append(g);
for(const x of series)for(const p of x.values||[]){const c=s('circle',{cx:vx(p[0]),cy:vy(p[1]),r:p[2]||4,fill:x.color,class:'mc-dot'});g.append(c);pts.push([vx(p[0]),vy(p[1]),x,p,c])}
svg.addEventListener('mousemove',e=>{const r=svg.getBoundingClientRect(),mx=e.clientX-r.left,my=e.clientY-r.top;let best=null,d=24*24;
for(const p of pts){const dd=(p[0]-mx)**2+(p[1]-my)**2;if(dd<d){d=dd;best=p}}pts.forEach(p=>p[4].classList.toggle('dim',!!best&&p!==best));
if(best)showTip(best[0],best[1],(best[3][3]?`<div class="mc-tip-title">${esc(best[3][3])}</div>`:'')+row(best[2].color,best[2].name||'',xf(best[3][0])+', '+yf(best[3][1])));else hideTip()});
svg.addEventListener('mouseleave',()=>{hideTip();pts.forEach(p=>p[4].classList.remove('dim'))});return}
const n=labels.length||Math.max(0,...series.map(x=>(x.values||[]).length)),band=(hor?ph:pw)/Math.max(1,n);
const center=j=>(hor?top:left)+band*(j+.5);
// Category labels, thinned so they never collide.
const every=hor?Math.ceil(16/band):Math.max(1,Math.ceil(Math.max(...labels.map(l=>textWidth(xf(l))),10)/band));
labels.forEach((l,j)=>{if(j%every)return;grid.append(hor?s('text',{x:left-8,y:center(j)+4,'text-anchor':'end'},l):s('text',{x:center(j),y:top+ph+16,'text-anchor':'middle'},xf(l)))});
const g=s('g',{});svg.append(g);const marks=[];
if(type==='bar'||type==='stacked'||hor){const inner=band*.72,k=stacked?1:Math.max(1,series.length),bw=inner/k,base=vy(Math.max(yn.lo,Math.min(0,yn.hi)));const acc=new Array(n).fill(0);
series.forEach((x,si)=>(x.values||[]).forEach((v,j)=>{if(typeof v!=='number')return;const from=stacked?acc[j]:0,to=from+v;if(stacked)acc[j]=to;
const a=vy(from),b=vy(to),off=center(j)-inner/2+(stacked?0:si*bw);
const rect=hor?s('rect',{x:Math.min(a,b),y:off+1,width:Math.max(1,Math.abs(b-a)),height:Math.max(1,bw-2),rx:3,fill:x.color,class:'mc-bar h',style:`--i:${j}`})
:s('rect',{x:off+1,y:Math.min(a,b),width:Math.max(1,bw-2),height:Math.max(1,Math.abs(b-a)),rx:3,fill:x.color,class:'mc-bar'+(v<0?' neg':''),style:`--i:${j}`});
g.append(rect);marks.push([j,rect])}));void base}
else{series.forEach(x=>{const pts=(x.values||[]).map((v,j)=>typeof v==='number'?[center(j),vy(v)]:null);
const segs=[];let curSeg=[];for(const p of pts){if(p)curSeg.push(p);else if(curSeg.length){segs.push(curSeg);curSeg=[]}}if(curSeg.length)segs.push(curSeg);
for(const seg of segs){const d=smooth(seg,o.curve!==false);
if(type==='area')g.append(s('path',{d:d+`L${seg[seg.length-1][0]},${vy(Math.max(yn.lo,0))}L${seg[0][0]},${vy(Math.max(yn.lo,0))}Z`,fill:x.color,class:'mc-area'}));
g.append(s('path',{d,stroke:x.color,class:'mc-line',pathLength:1}))}
pts.forEach((p,j)=>{if(!p)return;const c=s('circle',{cx:p[0],cy:p[1],r:3.5,fill:x.color,class:'mc-point'});g.append(c);marks.push([j,c])})})}
const cross=s('line',{class:'mc-cross',y1:top,y2:top+ph,x1:0,x2:0});if(hor)cross.setAttribute('class','mc-cross off');svg.append(cross);
const pick=j=>{marks.forEach(([k,m])=>m.classList.toggle(type==='bar'||type==='stacked'||hor?'dim':'on',type==='bar'||type==='stacked'||hor?k!==j:k===j))};
svg.addEventListener('mousemove',e=>{const r=svg.getBoundingClientRect(),p=hor?e.clientY-r.top:e.clientX-r.left;const j=Math.floor((p-(hor?top:left))/band);
if(j<0||j>=n){hideTip();pick(-1);cross.style.opacity='0';return}
if(!hor){cross.setAttribute('x1',center(j));cross.setAttribute('x2',center(j));cross.style.opacity=type==='line'||type==='area'?'1':'0'}
pick(j);const lines=series.filter(x=>typeof(x.values||[])[j]==='number').map(x=>row(x.color,x.name||'',yf(x.values[j]))).join('');
const total=stacked&&series.length>1?row('transparent','Total',yf(series.reduce((a,x)=>a+(+x.values[j]||0),0))):'';
showTip(hor?e.clientX-r.left:center(j),hor?center(j):e.clientY-r.top,`<div class="mc-tip-title">${esc(labels[j]!=null?xf(labels[j]):'#'+(j+1))}</div>`+lines+total)});
svg.addEventListener('mouseleave',()=>{hideTip();pick(-1);cross.style.opacity='0'})};
const donut=(svg,o,W,H,anim)=>{const vals=(o.values||((o.series||[])[0]||{}).values||[]).map((v,i)=>hidden.has(i)?0:Math.max(0,+v||0)),total=vals.reduce((a,b)=>a+b,0)||1;
const R=Math.min(H/2-6,W/2-6),r=R*(o.thickness?1-o.thickness:.62),cx=W/2,cy=H/2,yf=formatter(o.format||(o.y&&o.y.format));
const label=s('text',{x:cx,y:cy-2,'text-anchor':'middle',class:'mc-donut-value'},o.center!=null?o.center:yf(vals.reduce((a,b)=>a+b,0)));
const sub=s('text',{x:cx,y:cy+16,'text-anchor':'middle',class:'mc-donut-label'},o.centerLabel||'Total');
let a=-Math.PI/2;const arcs=[];
vals.forEach((v,i)=>{if(!v)return;const b=a+v/total*Math.PI*2,large=b-a>Math.PI?1:0,gap=vals.filter(Boolean).length>1?.012:0;
const p=(ang,rad)=>[cx+rad*Math.cos(ang),cy+rad*Math.sin(ang)];const[a1,a2]=[a+gap,b-gap];
const d=b-a>=Math.PI*2-1e-6?`M${cx-R},${cy}a${R},${R} 0 1,0 ${2*R},0a${R},${R} 0 1,0 ${-2*R},0M${cx-r},${cy}a${r},${r} 0 1,1 ${2*r},0a${r},${r} 0 1,1 ${-2*r},0`
:`M${p(a1,R)}A${R},${R} 0 ${large} 1 ${p(a2,R)}L${p(a2,r)}A${r},${r} 0 ${large} 0 ${p(a1,r)}Z`;
const c=tint(o.colors&&o.colors[i],i),path=s('path',{d,fill:c,class:'mc-arc','fill-rule':'evenodd',style:`--i:${i}`});
path.addEventListener('mouseenter',()=>{arcs.forEach(x=>x.classList.toggle('dim',x!==path));label.textContent=yf(v);sub.textContent=`${(o.labels||[])[i]??''} · ${Math.round(v/total*100)}%`});
path.addEventListener('mouseleave',()=>{arcs.forEach(x=>x.classList.remove('dim'));label.textContent=o.center!=null?o.center:yf(vals.reduce((x,y)=>x+y,0));sub.textContent=o.centerLabel||'Total'});
arcs.push(path);svg.append(path);a=b});svg.append(label,sub)};
const smooth=(pts,curve)=>{if(!curve||pts.length<3)return pts.map((p,i)=>(i?'L':'M')+p[0]+','+p[1]).join('');
let d=`M${pts[0][0]},${pts[0][1]}`;for(let i=0;i<pts.length-1;i++){const p0=pts[i-1]||pts[i],p1=pts[i],p2=pts[i+1],p3=pts[i+2]||p2,t=.18;
d+=`C${p1[0]+(p2[0]-p0[0])*t},${p1[1]+(p2[1]-p0[1])*t} ${p2[0]-(p3[0]-p1[0])*t},${p2[1]-(p3[1]-p1[1])*t} ${p2[0]},${p2[1]}`}return d};
el.append(legend);mount(target,el);
const ro=new ResizeObserver(()=>{if(Math.abs(plot.clientWidth-width)>1)draw()});
requestAnimationFrame(()=>{draw();ro.observe(plot)});
return{el,update(next){cur={...cur,...next};draw()}}};

// Flow diagrams: nodes laid out in ranks from their edges, connected by arrows.
const flow=(target,o)=>{const el=h('div',{class:'mc-flow'}),svg=s('svg',{class:'mc-flow-edges'}),layer=h('div',{class:'mc-flow-nodes'});el.append(svg,layer);
let dir='right';const nodes=o.nodes||[],edges=(o.edges||[]).map(e=>Array.isArray(e)?{from:e[0],to:e[1],label:e[2]}:e);
const byId=new Map(nodes.map(n=>[n.id,n])),rank=new Map(nodes.map(n=>[n.id,n.rank??0]));
for(let pass=0;pass<nodes.length;pass++){let moved=false;for(const e of edges){if(!byId.has(e.from)||!byId.has(e.to)||byId.get(e.to).rank!=null)continue;
const r=rank.get(e.from)+1;if(r>rank.get(e.to)&&r<nodes.length){rank.set(e.to,r);moved=true}}if(!moved)break}
const ranks=[];for(const n of nodes){const r=rank.get(n.id);(ranks[r]=ranks[r]||[]).push(n)}
for(let r=1;r<ranks.length;r++){if(!ranks[r])continue;const pos=new Map();ranks[r-1]&&ranks[r-1].forEach((n,i)=>pos.set(n.id,i));
const score=n=>{const p=edges.filter(e=>e.to===n.id&&pos.has(e.from)).map(e=>pos.get(e.from));return p.length?p.reduce((a,b)=>a+b,0)/p.length:Infinity};
ranks[r].sort((a,b)=>score(a)-score(b))}
const boxes=new Map();for(const n of nodes){const tone=n.tone?' '+n.tone:'';
const box=h('div',{class:'mc-node'+tone,tabindex:0,'data-id':n.id},n.icon?icon(n.icon,{size:15}):null,h('div',{class:'mc-node-text'},h('div',{class:'mc-node-label'},n.label??n.id),n.detail?h('div',{class:'mc-node-detail'},n.detail):null));
box.addEventListener('click',()=>{o.onSelect&&o.onSelect(n)});layer.append(box);boxes.set(n.id,box)}
const labels=edges.map(e=>e.label?h('div',{class:'mc-edge-label'},e.label):null);labels.forEach(l=>l&&layer.append(l));
let focused=null;
const layout=()=>{const W=el.clientWidth||600,R=ranks.length,gapCross=16;
// Ranks run left to right and continue on the next row when they do not fit
// across; 'down' stacks them top to bottom instead.
dir=o.direction==='down'?'down':'right';
const gapMain=Math.max(28,Math.min(96,o.gap||(dir==='right'?56:40)));
const per=dir==='right'?Math.max(1,Math.min(R,Math.floor((W+gapMain)/(140+gapMain)))):R,colW=(W-(per-1)*gapMain)/per;
for(const b of boxes.values())b.style.maxWidth=(dir==='right'?colW:Math.min(260,(W-gapCross*3)/Math.max(...ranks.map(r=>r?r.length:1))))+'px';
const size=new Map([...boxes].map(([id,b])=>[id,[b.offsetWidth,b.offsetHeight]]));
const pos=new Map(),place=(n,x,y)=>{const[w,hh]=size.get(n.id);pos.set(n.id,[x,y,w,hh]);const b=boxes.get(n.id);b.style.left=x+'px';b.style.top=y+'px'};
const along=ri=>Math.max(0,...(ranks[ri]||[]).map(n=>size.get(n.id)[dir==='right'?0:1]));
const across=ri=>(ranks[ri]||[]).reduce((a,n)=>a+size.get(n.id)[dir==='right'?1:0],0)+gapCross*Math.max(0,(ranks[ri]||[]).length-1);
let H;
if(dir==='right'){const rows=[];for(let ri=0;ri<R;ri++)(rows[Math.floor(ri/per)]=rows[Math.floor(ri/per)]||[]).push(ri);
const single=rows.length===1,rowGap=44;let y=4;
for(const row of rows){const rh=Math.max(0,...row.map(across)),used=row.reduce((a,ri)=>a+along(ri),0)+gapMain*(row.length-1);
let x=single?Math.max(0,(W-used)/2):0;
for(const ri of row){const cw=single?along(ri):colW;let c=y+(rh-across(ri))/2;
for(const n of ranks[ri]||[]){place(n,x+(cw-size.get(n.id)[0])/2,c);c+=size.get(n.id)[1]+gapCross}x+=cw+gapMain}
y+=rh+rowGap}H=y-rowGap+4}
else{let y=4;for(let ri=0;ri<R;ri++){let x=(W-across(ri))/2;
for(const n of ranks[ri]||[]){place(n,x,y+(along(ri)-size.get(n.id)[1])/2);x+=size.get(n.id)[0]+gapCross}y+=along(ri)+gapMain}H=y-gapMain+4}
el.style.height=H+'px';svg.setAttribute('width',W);svg.setAttribute('height',H);svg.setAttribute('viewBox',`0 0 ${W} ${H}`);svg.replaceChildren(s('defs',{},s('marker',{id:'mc-arrow',viewBox:'0 0 10 10',refX:9,refY:5,markerWidth:7,markerHeight:7,orient:'auto-start-reverse'},s('path',{d:'M0,1L9,5L0,9z',class:'mc-arrowhead'}))));
edges.forEach((e,i)=>{const a=pos.get(e.from),b=pos.get(e.to);if(!a||!b)return;let x1,y1,x2,y2,d;
if(dir==='right'){x1=a[0]+a[2];y1=a[1]+a[3]/2;x2=b[0]-2;y2=b[1]+b[3]/2;if(x2<x1+8){x1=a[0]+a[2]/2;y1=a[1]+a[3];x2=b[0]+b[2]/2;y2=b[1]-2;const k=Math.max(24,Math.abs(y2-y1)/2);d=`M${x1},${y1}C${x1},${y1+k} ${x2},${y2-k} ${x2},${y2}`}
else{const k=Math.max(20,(x2-x1)/2);d=`M${x1},${y1}C${x1+k},${y1} ${x2-k},${y2} ${x2},${y2}`}}
else{x1=a[0]+a[2]/2;y1=a[1]+a[3];x2=b[0]+b[2]/2;y2=b[1]-2;const k=Math.max(20,(y2-y1)/2);d=`M${x1},${y1}C${x1},${y1+k} ${x2},${y2-k} ${x2},${y2}`}
const path=s('path',{d,class:'mc-edge'+(e.dashed?' dashed':''),'marker-end':'url(#mc-arrow)','data-from':e.from,'data-to':e.to});svg.append(path);
const l=labels[i];if(l){const len=path.getTotalLength(),p=path.getPointAtLength(len/2);l.style.left=p.x+'px';l.style.top=p.y+'px'}});
apply()};
const apply=()=>{const on=focused&&new Set(focused);for(const[id,b]of boxes){b.classList.toggle('dim',!!on&&!on.has(id));b.classList.toggle('active',!!on&&on.has(id))}
svg.querySelectorAll('.mc-edge').forEach(p=>{const live=!!on&&on.has(p.dataset.from)&&on.has(p.dataset.to);p.classList.toggle('active',live);p.classList.toggle('dim',!!on&&!live)})};
mount(target,el);const ro=new ResizeObserver(()=>layout());requestAnimationFrame(()=>{layout();ro.observe(el)});
return{el,focus(ids){focused=ids==null?null:[].concat(ids);apply()},layout}};

// A stepper for explanations: one step at a time, with optional flow focus.
const steps=(target,list,o={})=>{let i=0,timer=null;const body=h('div',{class:'mc-step-body'}),title=h('div',{class:'mc-step-title'}),count=h('span',{class:'mc-label'}),
dots=h('div',{class:'mc-step-dots'},list.map((_,j)=>h('button',{type:'button','aria-label':'Step '+(j+1),onclick:()=>go(j)}))),
prev=h('button',{class:'mc-button small icon',type:'button','aria-label':'Previous step',onclick:()=>go(i-1)},icon('chevron-left',{size:14})),
next=h('button',{class:'mc-button small icon',type:'button','aria-label':'Next step',onclick:()=>go(i+1)},icon('chevron-right',{size:14})),
play=o.autoplay?h('button',{class:'mc-button small ghost',type:'button',onclick:()=>timer?stop():start()},icon('pause',{size:14})):null,
view=h('div',{class:'mc-step-view'});
const el=h('div',{class:'mc-steps',tabindex:-1},h('div',{class:'mc-step-head'},count,title,h('div',{class:'mc-spacer'}),play,prev,next),body,view,dots);
const go=j=>{i=Math.max(0,Math.min(list.length-1,j));const st=list[i];count.textContent=`${i+1} / ${list.length}`;title.textContent=st.title||'';body.textContent=st.body||'';
prev.disabled=i===0;next.disabled=i===list.length-1;[...dots.children].forEach((d,k)=>d.classList.toggle('on',k<=i));
if(o.flow)o.flow.focus(st.focus??null);view.replaceChildren();view.hidden=!st.render;if(st.render)try{st.render(view,i)}catch(e){console.error(e)}
o.onStep&&o.onStep(i,st)};
const start=()=>{stop();timer=setInterval(()=>go(i===list.length-1?0:i+1),Math.max(1200,o.autoplay));if(play)play.replaceChildren(icon('pause',{size:14}))};
const stop=()=>{clearInterval(timer);timer=null;if(play)play.replaceChildren(icon('play',{size:14}))};
el.addEventListener('keydown',e=>{if(e.key==='ArrowRight')go(i+1);else if(e.key==='ArrowLeft')go(i-1)});
mount(target,el);go(o.initial||0);if(o.autoplay)start();return{el,go,get index(){return i}}};

// Syntax highlighting for code inside a page.
const KW={js:'abstract as async await break case catch class const continue debugger default delete do else enum export extends false finally for from function get if implements import in instanceof interface let new null of package private protected public readonly return set static super switch this throw true try type typeof undefined var void while with yield',
rust:'as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while Some None Ok Err',
py:'and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield self',
go:'break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var nil true false',
c:'auto break case catch char class const continue default delete do double else enum extern false final float for fun func guard if import in inline int let long namespace new nil null override private protected public return self short signed sizeof static struct super switch template this throw true try typedef union unsigned using val var virtual void volatile when while',
sh:'if then else elif fi for in do done while until case esac function return local export echo exit set unset source',
sql:'select from where and or not insert into values update set delete create table index view drop alter add join left right inner outer on group by order having limit offset as distinct union all null is in like between case when then else end primary key references default',
css:'important',json:'true false null'};
const LANG={ts:'js',tsx:'js',jsx:'js',javascript:'js',typescript:'js',rs:'rust',python:'py',golang:'go',swift:'c',kotlin:'c',kt:'c',java:'c',cpp:'c','c++':'c',cs:'c',csharp:'c',bash:'sh',zsh:'sh',shell:'sh',yaml:'sh',yml:'sh',toml:'sh',postgres:'sql',scss:'css',less:'css'};
const esc=t=>String(t).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'})[c]);
const highlight=(src,lang)=>{const l=LANG[lang]||lang;
if(l==='html'||l==='xml'||l==='svg')return esc(src).replace(/(&lt;!--[\s\S]*?--&gt;)|(&lt;\/?)([\w:-]+)|([\w:-]+)(=)(&quot;[^&]*?&quot;)/g,(m,c,open,tag,attr,eq,val)=>c?`<span class="t-com">${c}</span>`:tag?`${open}<span class="t-key">${tag}</span>`:`<span class="t-attr">${attr}</span>${eq}<span class="t-str">${val}</span>`);
if(l==='diff')return src.split('\n').map(x=>`<span class="${x.startsWith('+')?'t-add':x.startsWith('-')?'t-del':x.startsWith('@@')?'t-com':''}">${esc(x)}</span>`).join('\n');
const words=new Set((KW[l]||KW.js).split(' ')),hash=l==='py'||l==='sh',dash=l==='sql',slash=!hash&&!dash;
const re=new RegExp([slash?'\\/\\/[^\\n]*|\\/\\*[\\s\\S]*?\\*\\/':null,hash?'#[^\\n]*':null,dash?'--[^\\n]*':null,
'"(?:\\\\[\\s\\S]|[^"\\\\\\n])*"|\'(?:\\\\[\\s\\S]|[^\'\\\\\\n])*\'|`(?:\\\\[\\s\\S]|[^`\\\\])*`','\\b\\d[\\d_]*(?:\\.\\d+)?(?:e[+-]?\\d+)?\\b|\\b0x[\\da-f]+\\b','[A-Za-z_$][\\w$]*'].filter(Boolean).join('|'),'gi');
let out='',last=0;for(const m of src.matchAll(re)){out+=esc(src.slice(last,m.index));const t=m[0],c=t[0];let cls='';
if(t.startsWith('//')||t.startsWith('/*')||(hash&&c==='#')||(dash&&t.startsWith('--')))cls='t-com';else if(c==='"'||c==="'"||c==='`')cls=l==='json'&&/^\s*:/.test(src.slice(m.index+t.length))?'t-attr':'t-str';
else if(/^\d|^0x/i.test(t))cls='t-num';else if(words.has(l==='sql'?t.toLowerCase():t))cls='t-key';else if(/^[A-Z][a-z]/.test(t))cls='t-type';else if(src[m.index+t.length]==='(')cls='t-fn';
out+=cls?`<span class="${cls}">${esc(t)}</span>`:esc(t);last=m.index+t.length}return out+esc(src.slice(last))};
const code=(source,o={})=>{const lines=highlight(String(source).replace(/\n$/,''),(o.lang||o.language||'js').toLowerCase()).split('\n'),marked=new Set(o.highlight||[]);
const pre=h('pre',{class:'mc-code-body'+(o.numbers?' numbered':'')+(o.wrap?' wrap':'')});
lines.forEach((l,i)=>{const line=h('span',{class:'mc-code-line'+(marked.has(i+1)?' marked':''),html:l||' '});pre.append(line)});
return h('div',{class:'mc-code'},o.title||o.lang?h('div',{class:'mc-code-head'},h('span',null,o.title||''),h('span',{class:'mc-label'},o.lang||'')):null,pre)};

// A sortable table; numbers align right and sort as numbers.
const table=(rows,o={})=>{const cols=(o.columns||Object.keys(rows[0]||{})).map(c=>typeof c==='string'?{key:c,label:c}:c);let key=null,dir=1;
const t=h('table',{class:'mc-table'}),tb=h('tbody');const numeric=c=>rows.every(r=>r[c.key]==null||typeof r[c.key]==='number');
const head=h('tr',null,cols.map(c=>{const th=h('th',{class:numeric(c)?'num':'','aria-sort':'none'},c.label??c.key);
if(o.sort!==false){th.classList.add('sortable');th.onclick=()=>{dir=key===c.key?-dir:1;key=c.key;[...head.children].forEach(x=>x.setAttribute('aria-sort','none'));th.setAttribute('aria-sort',dir>0?'ascending':'descending');fill()}}return th}));
const fill=()=>{const sorted=key==null?rows:[...rows].sort((a,b)=>{const x=a[key],y=b[key];return(x==null)-(y==null)||(x<y?-dir:x>y?dir:0)});
tb.replaceChildren(...sorted.map(r=>h('tr',null,cols.map(c=>{const v=r[c.key];return h('td',{class:numeric(c)?'num':''},c.format?c.format(v,r):v==null?'—':typeof v==='number'?v.toLocaleString():String(v))}))))};
t.append(h('thead',null,head),tb);fill();return h('div',{class:'mc-table-wrap',style:o.maxHeight?{maxHeight:o.maxHeight+'px'}:null},t)};

// Content a block can hold: an element, an HTML string, or a function that builds one.
const node=v=>typeof v==='function'?v():typeof v==='string'?h('div',{html:v}):v;

// Before/after: two renderings stacked, one revealed by dragging a handle.
const compare=(target,o={})=>{const labels=o.labels||['Before','After'];let pct=o.initial??50;
const range=h('input',{class:'mc-compare-range',type:'range',min:0,max:100,step:.5,value:pct,'aria-label':'Reveal '+labels[1]});
const el=h('div',{class:'mc-compare'},h('div',{class:'mc-compare-layer'},node(o.before)),h('div',{class:'mc-compare-layer after'},node(o.after)),
h('span',{class:'mc-compare-tag l'},labels[0]),h('span',{class:'mc-compare-tag r'},labels[1]),
h('div',{class:'mc-compare-line'},h('span',{class:'mc-compare-grip'},icon('chevron-left',{size:12}),icon('chevron-right',{size:12}))),range);
const set=v=>{pct=Math.max(0,Math.min(100,Number(v)));el.style.setProperty('--p',pct+'%');range.value=pct;o.onInput&&o.onInput(pct)};
range.addEventListener('input',()=>set(range.value));set(pct);mount(target,el);return{el,set,get value(){return pct}}};

// Numbered pins over any picture, paired with a list of notes that light up together.
const annotate=(target,o={})=>{const notes=o.notes||[];let active=-1;
const content=node(o.content!=null?o.content:o.src?h('img',{src:o.src,alt:o.alt||''}):null);
const pins=notes.map((n,i)=>h('button',{class:'mc-pin mc-pop',style:{left:n.x+'%',top:n.y+'%','--i':i},'aria-label':n.title||'Note '+(i+1)},String(i+1)));
const items=notes.map((n,i)=>h('li',{class:'mc-note-item'},h('b',null,n.title||''),n.text?h('span',null,n.text):null));
const paint=()=>[pins,items].forEach(list=>list.forEach((x,i)=>{x.classList.toggle('on',i===active);x.classList.toggle('dim',active>=0&&i!==active)}));
const pick=i=>{active=i;paint()};
pins.forEach((p,i)=>{p.onclick=()=>pick(i===active?-1:i);p.onmouseenter=()=>pick(i);p.onmouseleave=()=>pick(-1)});
items.forEach((li,i)=>{li.onclick=()=>pick(i===active?-1:i);li.onmouseenter=()=>pick(i);li.onmouseleave=()=>pick(-1)});
const el=h('div',{class:'mc-annotate'},h('div',{class:'mc-annotate-art'},content,pins),h('ol',{class:'mc-notes'},items));
mount(target,el);return{el,pick}};

// Events on a time axis. A number `at` is a moment; `end` makes it a span. Overlaps stack into lanes.
const timeline=(target,o={})=>{const ev=(o.events||[]).filter(e=>typeof e.at==='number');
const lo=o.min??Math.min(...ev.map(e=>e.at)),hi=o.max??Math.max(...ev.map(e=>e.end??e.at)),span=hi>lo?hi-lo:1,fmt=formatter(o.format);
const el=h('div',{class:'mc-timeline'}),track=h('div',{class:'mc-tl-track'});el.append(track);
const layout=()=>{const W=Math.max(240,el.clientWidth),lanes=[];track.replaceChildren();
const ticks=nice(lo,hi,Math.max(2,Math.floor(W/110))).ticks.filter(v=>v>=lo&&v<=hi);
for(const v of ticks){const at=(v-lo)/span*100;track.append(h('div',{class:'mc-tl-tick'+(at<3?' first':at>97?' last':''),style:{left:at+'%'}},h('span',null,fmt(v))))}
ev.forEach((e,i)=>{const x=(e.at-lo)/span*100,isSpan=e.end!=null,w=isSpan?Math.max(.6,(e.end-e.at)/span*100):0,
need=(Math.max(textWidth(e.label||''),textWidth(e.detail||'')*.92)+22)/W*100,right=!isSpan&&x+need>100;
const a=right?x-need:x,b=isSpan?x+Math.max(w,Math.min(need,8)):right?x:x+need;
let lane=lanes.findIndex(end=>end<=a);if(lane<0){lane=lanes.length;lanes.push(0)}lanes[lane]=b;
const label=[h('div',{class:'mc-tl-label'},e.label||''),e.detail?h('div',{class:'mc-tl-detail'},e.detail):null],top=lane*46+28+'px',tone='tone-'+(e.tone||'accent');
const item=isSpan?h('div',{class:'mc-tl-span mc-in '+tone,style:{left:x+'%',width:w+'%',top},title:[e.label,e.detail].filter(Boolean).join(' — ')},label)
:h('div',{class:'mc-tl-point mc-in '+tone+(right?' right':''),style:{[right?'right':'left']:(right?100-x:x)+'%',top}},h('i',{class:'mc-tl-dot'}),h('div',{class:'mc-tl-text'},label));
item.style.setProperty('--i',i);track.append(item)});
track.style.height=lanes.length*46+34+'px'};
mount(target,el);let last=0;new ResizeObserver(()=>{if(Math.abs(el.clientWidth-last)>1){last=el.clientWidth;layout()}}).observe(el);layout();return{el,layout}};

// A tiny inline trend for use inside stats, tables and text.
const spark=(values,o={})=>{const v=(values||[]).filter(x=>typeof x==='number'&&isFinite(x));if(!v.length)return h('span');
const W=o.width||96,H=o.height||28,n=v.length,lo=o.min??Math.min(...v),hi=o.max??Math.max(...v),pad=3,c=tint(o.color,o.index||0);
const px=i=>pad+(n>1?i/(n-1):.5)*(W-2*pad),py=x=>H-pad-(hi>lo?(x-lo)/(hi-lo):.5)*(H-2*pad),d='M'+v.map((x,i)=>px(i).toFixed(1)+' '+py(x).toFixed(1)).join('L');
return s('svg',{class:'mc-spark',viewBox:`0 0 ${W} ${H}`,width:W,height:H,role:'img','aria-label':o.label||'Trend'},
o.fill===false?null:s('path',{d:`${d}L${px(n-1).toFixed(1)} ${H}L${px(0).toFixed(1)} ${H}Z`,fill:c,opacity:.14}),
s('path',{d,fill:'none',stroke:c,'stroke-width':1.75,'stroke-linejoin':'round','stroke-linecap':'round'}),
o.last===false?null:s('circle',{cx:px(n-1).toFixed(1),cy:py(v[n-1]).toFixed(1),r:2.5,fill:c}))};

// A scrubber for when time is the point. Dragging calls onInput(t); {duration} adds Play.
const scrub=(target,o={})=>{const min=o.min??0,max=o.max??1,fmt=o.format||(x=>x.toFixed(2));let t=o.value??min,raf=0;
const play=h('button',{class:'mc-button ghost small icon','aria-label':'Play'},icon('play',{size:14}));
const input=h('input',{class:'mc-range wide',type:'range',min,max,step:o.step??(max-min)/400,value:t,'aria-label':o.label||'Time'}),out=h('output',null,fmt(t));
const el=h('div',{class:'mc-scrub'},o.duration?play:null,h('span',{class:'mc-label'},o.label||'Time'),input,out);
const set=x=>{t=Math.max(min,Math.min(max,Number(x)));input.value=t;out.textContent=fmt(t);o.onInput&&o.onInput(t)};
const stop=()=>{cancelAnimationFrame(raf);raf=0;play.replaceChildren(icon('play',{size:14}))};
const run=()=>{if(t>=max)set(min);const t0=performance.now(),from=t;play.replaceChildren(icon('pause',{size:14}));
const frame=()=>{const x=from+(performance.now()-t0)*mc.speed*(max-min)/o.duration;
if(x>=max){set(max);if(o.loop){set(min);return run()}return stop()}set(x);raf=requestAnimationFrame(frame)};raf=requestAnimationFrame(frame)};
play.onclick=()=>raf?stop():run();input.addEventListener('input',()=>{stop();set(input.value)});
mount(target,el);if(o.autoplay&&o.duration)run();return{el,set,play:run,pause:stop,get value(){return t}}};

let data;
Object.defineProperty(mc,'data',{get(){if(data===undefined){const el=document.getElementById('mc-data');try{data=el?JSON.parse(el.textContent):{}}catch(e){console.error('mc.data: '+e.message);data={}}}return data}});
Object.assign(mc,{svg:s,chart,flow,steps,code,highlight,table,compare,annotate,timeline,spark,scrub,format:short});
})();
