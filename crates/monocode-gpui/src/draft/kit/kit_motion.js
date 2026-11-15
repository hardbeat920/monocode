(()=>{
const {h,icon,clock}=mc;
const root=t=>typeof t==='string'?document.querySelector(t):(t||document.body);
const mount=(target,el)=>{const go=()=>root(target).append(el);if(document.body)go();else addEventListener('DOMContentLoaded',go)};
const word=n=>n[0].toUpperCase()+n.slice(1);

// Playback controls for motion, outside the content. Every part is optional:
//   states   an mc.states: a button flips to the next state, labelled with its name
//   labels   {stateName: 'Button text'} to say what the button does ("Collapse")
//   replay   a function: adds a Replay button
//   slow     N: a "Slow motion xN" switch (default 4 when speeds is not set)
//   speeds   [1, .5, .25]: speed chips instead of the switch
//   autoplay true or ms: an Auto-play switch that flips states (or calls replay)
//            on a loop, every `every` ms (default 1600)
//   pause    true: adds Pause/Play for the whole page's clock
const transport=(target,o={})=>{const el=h('div',{class:'mc-transport',role:'group','aria-label':'Playback'});
const off=[];let loop=0;
const btn=(label,onclick,ic)=>h('button',{class:'mc-button',type:'button',onclick},ic?icon(ic,{size:14}):null,label);
const flip=()=>{if(o.states)o.states.next();else if(o.replay)o.replay()};
if(o.states){const b=btn('',()=>o.states.next());
off.push(o.states.on((_,i)=>{const next=o.states.names[(i+1)%o.states.names.length];b.replaceChildren((o.labels&&o.labels[next])||word(next))}));el.append(b)}
if(o.replay)el.append(btn('Replay',o.replay,'replay'));
if(o.pause){const b=btn('',()=>clock.pause(!clock.paused));
off.push(clock.on(({paused})=>{b.replaceChildren(icon(paused?'play':'pause',{size:14}),paused?'Play':'Pause')}));el.append(b)}
const pressed=(b,on)=>b.setAttribute('aria-pressed',String(!!on));
if(Array.isArray(o.speeds)){const seg=h('div',{class:'mc-ctl-seg',role:'radiogroup','aria-label':'Speed'});
const chip=v=>({1:'1×',.5:'½×',.25:'¼×'})[v]||v+'×';
const bs=o.speeds.map(v=>h('button',{type:'button',role:'radio',onclick:()=>clock.setSpeed(v)},chip(v)));seg.append(...bs);
off.push(clock.on(({speed})=>bs.forEach((b,i)=>{const on=o.speeds[i]===speed;b.setAttribute('aria-checked',String(on));b.tabIndex=on?0:-1})));el.append(seg)}
else if(o.slow!==false){const n=typeof o.slow==='number'?o.slow:4;const b=btn('Slow motion ×'+n,()=>clock.setSpeed(clock.speed===1?1/n:1));
off.push(clock.on(({speed})=>pressed(b,speed!==1)));el.append(b)}
if(o.autoplay&&(o.states||o.replay)){const every=typeof o.autoplay==='number'?o.autoplay:o.every||1600;const b=btn('Auto-play',()=>on(!loop));
const on=v=>{pressed(b,v);clearTimeout(loop);loop=0;if(!v)return;
const step=()=>{if(!clock.paused)flip();loop=setTimeout(step,every)};loop=setTimeout(step,every)};
pressed(b,false);el.append(b)}
mount(target,el);
el.destroy=()=>{clearTimeout(loop);off.forEach(f=>f());el.remove()};return el};

Object.assign(mc,{transport});
})();
