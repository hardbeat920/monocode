(()=>{
const {h,icon}=mc;
const root=t=>typeof t==='string'?document.querySelector(t):(t||document.body);
const mount=(target,el)=>{const go=()=>root(target).append(el);if(document.body)go();else addEventListener('DOMContentLoaded',go)};
const cssName=k=>'--s-'+k.replace(/[A-Z]/g,m=>'-'+m.toLowerCase());
const attrName=k=>'s'+k[0].toUpperCase()+k.slice(1);

// One store for what the controls change. Each key is mirrored to a CSS custom
// property (--s-radius) and booleans to a data attribute (data-s-glow="on"), so a
// page can react in plain CSS. The values also go to the agent, who sees them in
// the next draft snapshot as `state`.
// The page has one store: mc.state() returns it, and a later mc.state({...})
// or mc.controls adds its keys (with their starting values) to the same store
// instead of starting a rival one the agent can't see.
let page=null;
const state=init=>{if(page){const fresh=Object.entries(init||{}).filter(([k])=>!page.has(k));if(fresh.length)page.add(Object.fromEntries(fresh));return page}
const base={...init},cur={...base},subs=new Set(),el=document.documentElement;let timer=0;
const put=(k,v)=>{if(typeof v==='boolean')el.dataset[attrName(k)]=v?'on':'off';else if(v==null)el.style.removeProperty(cssName(k));else el.style.setProperty(cssName(k),String(v))};
const tell=()=>{clearTimeout(timer);timer=setTimeout(()=>{try{const v=JSON.stringify(cur);if(v.length<4000)parent.postMessage({__mc:1,k:'v',v:JSON.parse(v)},'*')}catch(e){}},120)};
for(const[k,v]of Object.entries(cur))put(k,v);tell();
const publish=()=>{subs.forEach(f=>{try{f({...cur})}catch(e){console.error(e)}});tell()};
const s={get:k=>cur[k],
set(k,v){if(cur[k]===v)return s;cur[k]=v;put(k,v);publish();return s},
patch(o){let changed=false;for(const[k,v]of Object.entries(o))if(cur[k]!==v){cur[k]=v;put(k,v);changed=true}if(changed)publish();return s},
reset(){return s.patch(base)},
has:k=>k in cur,
add(o){Object.assign(base,o);for(const[k,v]of Object.entries(o)){cur[k]=v;put(k,v)}publish();return s},
// Calls f now and after every change; returns a function that stops it.
on(f){subs.add(f);try{f({...cur})}catch(e){console.error(e)}return()=>subs.delete(f)},
get values(){return{...cur}}};
page=s;
// The agent's {kind:'set'} action patches the store, by its own keys.
window.__mcSet=o=>{if(!o||typeof o!=='object'||Array.isArray(o))return 'values must be an object';
const bad=Object.keys(o).filter(k=>!(k in cur));if(bad.length)return 'Unknown state '+bad.join(', ')+'; this page has '+(Object.keys(cur).join(', ')||'none');
s.patch(o);return ''};
return s};

// Sends a line to the agent, as from the person: for a control the page
// offers, such as "Use this one". Works only straight after the person's own
// click or key press; returns whether it went.
const send=text=>{const ok=!!(window.__mcTell&&window.__mcTell(String(text)));
if(!ok)console.warn('mc.send works only from the person\'s own click or key press');return ok};

const label=c=>c.label||c.key[0].toUpperCase()+c.key.slice(1);

// Builds one control and wires it to the store. Returns {el, kind?, stop}.
const build=(c,s,off)=>{const bind=f=>off.push(s.on(f));const set=v=>{s.set(c.key,v);c.onChange&&c.onChange(v)};
if(c.type==='slider'){const fmt=c.format||(v=>v+(c.unit||''));
const input=h('input',{class:'mc-range',type:'range',min:c.min??0,max:c.max??100,step:c.step??1,'aria-label':label(c)});
const out=h('output',null,'');input.oninput=()=>set(Number(input.value));
bind(v=>{input.value=v[c.key];out.textContent=fmt(Number(v[c.key]))});return{input,out}}
if(c.type==='text'){const input=h('input',{class:'mc-input',type:'text',placeholder:c.placeholder,'aria-label':label(c)});
input.oninput=()=>set(input.value);bind(v=>{if(input.value!==String(v[c.key]??''))input.value=v[c.key]??''});return{input}}
if(c.type==='textarea'){const input=h('textarea',{class:'mc-textarea',rows:c.rows||3,placeholder:c.placeholder,'aria-label':label(c)});
input.oninput=()=>set(input.value);bind(v=>{if(input.value!==String(v[c.key]??''))input.value=v[c.key]??''});return{input}}
if(c.type==='toggle'){const input=h('button',{class:'mc-switch',type:'button',role:'switch','aria-label':label(c)});
input.onclick=e=>{e.preventDefault();set(!s.get(c.key))};bind(v=>input.setAttribute('aria-checked',String(!!v[c.key])));return{input,toggle:true}}
if(c.type==='checkbox'){const box=h('input',{type:'checkbox'});box.onchange=()=>set(box.checked);
bind(v=>{box.checked=!!v[c.key]});return{input:h('label',{class:'mc-ctl-check'},box,label(c)),bare:true}}
if(c.type==='segmented'||c.type==='radio'){const opts=c.options.map(o=>typeof o==='object'?o:{value:o,label:String(o)});
if(c.type==='radio'){const name='mc-ctl-'+c.key;const inputs=opts.map(o=>h('input',{type:'radio',name,value:o.value,onchange:()=>set(o.value)}));
bind(v=>inputs.forEach((i,n)=>{i.checked=opts[n].value===v[c.key]}));
return{input:h('div',{class:'mc-ctl-group',role:'radiogroup','aria-label':label(c)},opts.map((o,n)=>h('label',{class:'mc-ctl-check'},inputs[n],o.label)))}}
const input=h('div',{class:'mc-ctl-seg',role:'radiogroup','aria-label':label(c)},opts.map(o=>h('button',{role:'radio',type:'button',onclick:()=>set(o.value)},o.label)));
input.onkeydown=e=>{const d=e.key==='ArrowRight'?1:e.key==='ArrowLeft'?-1:0;if(!d)return;e.preventDefault();
const i=(opts.findIndex(o=>o.value===s.get(c.key))+d+opts.length)%opts.length;set(opts[i].value);input.children[i].focus()};
bind(v=>[...input.children].forEach((b,i)=>{const on=opts[i].value===v[c.key];b.setAttribute('aria-checked',String(on));b.tabIndex=on?0:-1}));return{input}}
if(c.type==='select'){const opts=c.options.map(o=>typeof o==='object'?o:{value:o,label:String(o)});
const sel=h('select',{'aria-label':label(c)},opts.map(o=>h('option',{value:o.value},o.label)));
sel.onchange=()=>set(opts.find(o=>String(o.value)===sel.value).value);
bind(v=>{sel.value=v[c.key]});return{input:h('span',{class:'mc-ctl-select'},sel,icon('chevron-down'))}}
if(c.type==='stepper'){const show=v=>(c.prefix||'')+v+(c.unit||'');
const num=h('input',{class:'mc-input',type:'text',inputmode:'numeric','aria-label':label(c)});
const clamp=n=>Math.max(c.min??-1e9,Math.min(c.max??1e9,n));
const step=d=>set(clamp(s.get(c.key)+d*(c.step||1)));
num.onchange=()=>{const n=parseFloat(num.value.replace(/[^\d.+-]/g,''));set(Number.isFinite(n)?clamp(n):s.get(c.key));num.value=show(s.get(c.key))};
num.onfocus=()=>{num.value=s.get(c.key)};num.onblur=()=>{num.value=show(s.get(c.key))};
num.onkeydown=e=>{if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();step(e.key==='ArrowUp'?1:-1)}};
bind(v=>{if(document.activeElement!==num)num.value=show(v[c.key])});
return{input:h('span',{class:'mc-ctl-stepper'},
h('button',{class:'mc-button icon',type:'button','aria-label':'Decrease '+label(c),onclick:()=>step(-1)},icon('minus',{size:14})),num,
h('button',{class:'mc-button icon',type:'button','aria-label':'Increase '+label(c),onclick:()=>step(1)},icon('plus',{size:14})))}}
if(c.type==='swatch'){const input=h('div',{class:'mc-ctl-swatch',role:'radiogroup','aria-label':label(c)},c.options.map(col=>
h('button',{role:'radio',type:'button',style:'--c:'+col,'aria-label':col,onclick:()=>set(col)})));
bind(v=>[...input.children].forEach((b,i)=>b.setAttribute('aria-checked',String(c.options[i]===v[c.key]))));return{input}}
throw new Error('mc.controls: unknown type "'+c.type+'"')};

// Controls outside the content that change it. `spec` lists them; each has a key
// into the store, a type, and type options:
//   slider {min,max,step,unit,format}  text/textarea {placeholder}  toggle  checkbox
//   segmented/radio/select {options: ['a','b'] or [{value,label}]}
//   stepper {min,max,step,prefix,unit}  swatch {options: [colors]}
//   {type:'button',label,icon?,primary?,onClick}  {type:'sep'}
// Every control takes label? and onChange?; `value` seeds the store when no
// state is passed. layout: 'rail' (beside), 'bar' (above) or 'dock' (floating).
const controls=(target,spec,o={})=>{const layout=o.layout||'rail';
const s=o.state||state(Object.fromEntries(spec.filter(c=>c.key&&'value'in c).map(c=>[c.key,c.value])));
const el=h('div',{class:'mc-ctl '+layout+(layout==='dock'?' mc-surface':''),role:'group','aria-label':o.title||'Controls'});
const off=[];
if(o.title&&layout==='rail')el.append(h('div',{class:'mc-ctl-head'},o.title));
for(const c of spec){
if(c.type==='sep'){el.append(h('div',{class:'mc-ctl-sep'}));continue}
if(c.type==='button'){el.append(h('button',{class:'mc-button '+(c.primary?'primary':'ghost'),type:'button',onclick:c.onClick},c.icon?icon(c.icon,{size:14}):null,c.label));continue}
const b=build(c,s,off);
if(b.bare){el.append(h('div',{class:'mc-ctl-field'},b.input));continue}
el.append(h('div',{class:'mc-ctl-field'+(b.toggle?' toggle':'')},
h('span',{class:'mc-ctl-row'},h('span',null,label(c)),b.out||null),b.input))}
mount(target,el);
el.state=s;el.destroy=()=>{off.forEach(f=>f());el.remove()};return el};

Object.assign(mc,{state,controls});
Object.assign(mc,{send});
})();
