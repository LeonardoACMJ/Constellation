import {TAU,point,wedge,entries,fit} from './geometry.mjs';
const $ = id => document.getElementById(id);
const star = (name) => ({id:crypto.randomUUID(),name});
const folder = (name,children) => ({...star(name),children});
const initialTree = () => folder('Your universe',[
 folder('Create',[star('ComfyUI'),star('Photoshop'),folder('Writing',[star('Obsidian'),star('Notes'),folder('Projects',[star('Constellation'),star('ProjHub'),star('Ideas')])]),star('Blender')]),
 folder('Play',[star('Overwatch'),star('Fortnite'),folder('Libraries',[star('Steam'),star('Epic Games')]),star('Discord')]),
 folder('Build',[star('VS Code'),star('Terminal'),folder('Projects',[star('ProjHub'),star('Hydrus tools'),star('Constellation')]),star('GitHub')]),
 star('Firefox'),folder('Everyday',[star('Music'),star('Explorer'),star('Calendar'),star('Calculator')]),star('Hydrus')]);
const defaultProfiles = () => [
 {id:crypto.randomUUID(),binding:{code:'KeyF',ctrl:true,shift:true,alt:false,meta:false},mode:'click',hover:true},
 {id:crypto.randomUUID(),binding:{code:'KeyG',ctrl:true,shift:true,alt:false,meta:false},mode:'hold',hover:true}
];
let saved;try {saved=JSON.parse(localStorage.getItem('constellation-lab-v1'));}catch{}
let tree=saved?.tree || initialTree(), profiles=saved?.profiles || defaultProfiles();
let radius=saved?.radius || 144, delay=saved?.delay ?? 60;
let active=null, path=[], pages=new Map(), hover=null, pending=null, pendingKey='', recording=null;
let anchor={x:0,y:0};
const stage=$('stage'), svg=$('radial'), ns='http://www.w3.org/2000/svg';
function persist(){try{localStorage.setItem('constellation-lab-v1',JSON.stringify({tree,profiles,radius,delay}));}catch{status('Storage unavailable; changes last for this session.');}}
function status(s){$('status').textContent=s;}
function bindingLabel(b){return b.mouse!==undefined?`Mouse ${b.mouse+1}`:[b.ctrl?'Ctrl':null,b.shift?'Shift':null,b.alt?'Alt':null,b.meta?'Win':null,b.code.replace('Key','').replace('Digit','')].filter(Boolean).join(' + ');}
function renderProfiles(){
 $('profiles').replaceChildren();
 for(const p of profiles){
  const row=document.createElement('div');row.className='profile';
  const top=document.createElement('div');top.className='profile-top';
  const bind=document.createElement('button');bind.className='binding';bind.textContent=recording===p.id?'Press a shortcut…':bindingLabel(p.binding);
  bind.onclick=()=>{close();recording=p.id;renderProfiles();status('Recording: press keys or a mouse button. Escape cancels.');};
  const remove=document.createElement('button');remove.className='text-button';remove.textContent='Remove';remove.onclick=()=>{if(active?.id===p.id)close();profiles=profiles.filter(x=>x.id!==p.id);persist();renderProfiles();};
  top.append(bind,remove);row.append(top);
  const label=document.createElement('label');label.textContent='Activation';
  const select=document.createElement('select');for(const [value,text] of [['click','Press to toggle'],['hold','Hold & release']]){const o=new Option(text,value);select.add(o);}select.value=p.mode;select.onchange=()=>{close();p.mode=select.value;persist();};label.append(select);row.append(label);
  const folders=document.createElement('label');folders.textContent='Constellations';
  const choice=document.createElement('select');choice.add(new Option('Open on hover','hover'));choice.add(new Option('Open on click','click'));choice.value=p.hover?'hover':'click';choice.onchange=()=>{p.hover=choice.value==='hover';persist();};folders.append(choice);row.append(folders);$('profiles').append(row);
 }
}
function refreshParents(){
 const select=$('parent'), previous=select.value;select.replaceChildren();
 function walk(n,depth){select.add(new Option('· '.repeat(depth)+n.name,n.id));for(const child of n.children || [])if(child.children)walk(child,depth+1);}
 walk(tree,0);if([...select.options].some(o=>o.value===previous))select.value=previous;
 let count=0;function tally(n){count++;n.children?.forEach(tally);}tree.children.forEach(tally);
 $('metrics').textContent=`${tree.children.length} root items · ${count} total`;
}
function find(n,id){if(n.id===id)return n;for(const c of n.children || []){const found=find(c,id);if(found)return found;}}
function el(type,attrs={},text){const e=document.createElementNS(ns,type);for(const [k,v]of Object.entries(attrs))e.setAttribute(k,v);if(text!==undefined)e.textContent=text;return e;}
function cancelPending(){clearTimeout(pending);pending=null;pendingKey='';}
function close(){cancelPending();active=null;hover=null;path=[];svg.replaceChildren();$('empty').classList.remove('hidden');$('crumb').textContent='ROOT CONSTELLATION';}
function open(p){close();active={...p};anchor={x:stage.clientWidth/2,y:stage.clientHeight/2};pages.clear();$('empty').classList.add('hidden');stage.focus({preventScroll:true});status(p.mode==='hold'?'Centered · hold the shortcut, then release over a star.':'Centered · click or Space to launch a hovered star.');render();}
function launch(n){if(!n||n.children||n.nav)return;close();status(`✦ Would launch “${n.name}” · simulated launch`);}
function openFolder(depth,n,a){if(path[depth]?.node.id===n.id)return;path=path.slice(0,depth);path.push({node:n,angle:Math.atan2(Math.sin(a),Math.cos(a))});hover=null;render();}
function render(){
 if(!active)return;
 svg.replaceChildren();const group=el('g');svg.append(group);
 let cx=0,cy=0,dir=-Math.PI/2;
 const bounds={minX:-radius,maxX:radius,minY:-radius,maxY:radius};
 const levels=[{node:tree,angle:dir},...path];
 for(let depth=0;depth<levels.length;depth++){
  const level=levels[depth];
  if(depth){dir=level.angle;[cx,cy]=point(cx,cy,radius*.52,dir);}
  const outer=radius*(depth?1.15:1),inner=radius*(depth?.58:.30);
  const sweep=depth?Math.PI*.82:TAU;
  const result=entries(level.node.children,pages.get(level.node.id)||0,depth?9:12);
  const items=result.items, start=depth?dir-sweep/2:-Math.PI/2-Math.PI/Math.max(1,items.length);
  const step=sweep/Math.max(1,items.length);
  // Include actual fan extrema, rather than its full imaginary circle.
  const candidates=[start,start+sweep,...[0,Math.PI/2,Math.PI,Math.PI*1.5,TAU].flatMap(a=>[a-TAU,a,a+TAU]).filter(a=>a>=start&&a<=start+sweep)];
  for(const a of candidates)for(const r of [inner,outer]){const [x,y]=point(cx,cy,r,a);bounds.minX=Math.min(bounds.minX,x);bounds.maxX=Math.max(bounds.maxX,x);bounds.minY=Math.min(bounds.minY,y);bounds.maxY=Math.max(bounds.maxY,y);}
  items.forEach((n,i)=>{
   const a=start+step*(i+.5), from=start+step*i+.009,to=start+step*(i+1)-.009;
   const selected=path[depth]?.node.id===n.id;
   const sector=el('path',{d:wedge(cx,cy,inner,outer,from,to),class:`sector ${n.children?'folder':'star'} ${selected?'selected':''}`,'data-depth':depth,'data-index':i,tabindex:0,role:'button','aria-label':n.children?`Open constellation ${n.name}`:n.nav?`${n.name} page`:`Launch ${n.name}`});
   sector._item={node:n,depth,angle:a,parent:level.node};group.append(sector);
   const [x,y]=point(cx,cy,inner+(outer-inner)*.54,a);
   group.append(el('text',{x,y:y-10,class:`symbol ${n.children?'':'star-symbol'}`},n.nav?(n.nav>0?'›':'‹'):n.children?'✧':'✦'));
   const label=n.name.length>17?n.name.slice(0,16)+'…':n.name;
   group.append(el('text',{x,y:y+11,style:`font-size:${items.length>9?10:11}px`},label));
   if(n.children)group.append(el('text',{...Object.fromEntries(['x','y'].map((k,j)=>[k,point(cx,cy,outer-10,a)[j]])),style:'font-size:10px;fill:#bba9ff'},'·'));
   sector.append(el('title',{},n.name));
  });
  if(result.pages>1){const [x,y]=point(cx,cy,inner-12,dir);group.append(el('text',{x,y,style:'font-size:9px'},`${result.page+1}/${result.pages}`));}
 }
 const hub=el('circle',{cx:0,cy:0,r:radius*.27,class:'hub',role:'button',tabindex:0,'aria-label':'Return to root'});hub.onclick=()=>{path=[];hover=null;render();};hub.onkeydown=e=>{if(e.key==='Enter'){path=[];render();}};group.append(hub,el('text',{x:0,y:-5,class:'symbol'},'✧'),el('text',{x:0,y:16,class:'hub-text'},'ROOT'));
 const f=fit(bounds,stage.clientWidth,stage.clientHeight,anchor,$('edges').checked);group.setAttribute('transform',`translate(${f.x} ${f.y}) scale(${f.scale})`);
 $('crumb').textContent=['Your universe',...path.map(p=>p.node.name)].join(' / ').toUpperCase();
}
function hit(e){return e.target.closest?.('[data-index]')?._item;}
function act(item){if(!active||!item)return;cancelPending();if(item.node.nav){pages.set(item.parent.id,(pages.get(item.parent.id)||0)+item.node.nav);path=path.slice(0,item.depth);hover=null;render();}else if(item.node.children){openFolder(item.depth,item.node,item.angle);}else launch(item.node);}
stage.addEventListener('pointermove',e=>{
 const item=hit(e);hover=item || null;
 svg.querySelectorAll('.sector.selected').forEach(p=>{if(!path.some(x=>x.node.id===p._item?.node.id))p.classList.remove('selected');});
 if(item){e.target.classList.add('selected');}
 if(!active?.hover||!item||item.node.nav){cancelPending();return;}
 const key=`${item.depth}:${item.node.id}`;if(pendingKey===key)return;cancelPending();
 if(item.node.children){if(path[item.depth]?.node.id===item.node.id)return;pendingKey=key;pending=setTimeout(()=>{pendingKey='';openFolder(item.depth,item.node,item.angle);},delay);}
 else if(path.length>item.depth){pendingKey=key;pending=setTimeout(()=>{path=path.slice(0,item.depth);hover=item;render();},Math.max(delay,100));}
});
stage.addEventListener('pointerleave',()=>{hover=null;cancelPending();});
svg.addEventListener('click',e=>{if(e.button===0)act(hit(e));});
svg.addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();act(hit(e));}});
stage.addEventListener('click',e=>{if(active&&e.target===svg){close();status('Dismissed');}});
stage.addEventListener('contextmenu',e=>{if(active){e.preventDefault();close();status('Dismissed');}});
const isEditing=e=>/INPUT|SELECT|TEXTAREA/.test(e.target.tagName)||e.target.isContentEditable;
function matches(b,e){return b.mouse===undefined&&e.code===b.code&&e.ctrlKey===b.ctrl&&e.shiftKey===b.shift&&e.altKey===b.alt&&e.metaKey===b.meta;}
function setBinding(b){const p=profiles.find(x=>x.id===recording);if(!p)return;const label=bindingLabel(b);if(profiles.some(x=>x.id!==p.id&&bindingLabel(x.binding)===label)){status('That shortcut already has a profile. Choose another.');return;}p.binding=b;recording=null;persist();renderProfiles();status(`Shortcut set to ${label}`);}
document.addEventListener('keydown',e=>{
 if(e.key==='Escape'){e.preventDefault();recording=null;renderProfiles();close();status('Dismissed');return;}
 if(recording){e.preventDefault();if(!['Control','Alt','Shift','Meta'].includes(e.key))setBinding({code:e.code,ctrl:e.ctrlKey,shift:e.shiftKey,alt:e.altKey,meta:e.metaKey});return;}
 if(isEditing(e))return;
 const p=profiles.find(x=>matches(x.binding,e));if(p){e.preventDefault();if(e.repeat)return;if(active?.id===p.id){if(p.mode==='click')close();}else open(p);return;}
 if(active&&e.code==='Space'){e.preventDefault();if(!e.repeat)launch(hover?.node);}
 if(active&&e.code==='Backspace'){e.preventDefault();path.pop();hover=null;cancelPending();render();}
});
document.addEventListener('keyup',e=>{
 if(active?.mode!=='hold'||active.binding.mouse!==undefined)return;
 const b=active.binding;
 const released=e.code===b.code||(b.ctrl&&e.key==='Control')||(b.shift&&e.key==='Shift')||(b.alt&&e.key==='Alt')||(b.meta&&e.key==='Meta');
 if(released){const n=hover?.node;if(n&&!n.children&&!n.nav)launch(n);else{close();status('Released without a star · cancelled');}}
});
document.addEventListener('pointerdown',e=>{
 if(recording){e.preventDefault();setBinding({mouse:e.button});return;}
 if(e.button===0||isEditing(e)||e.target.closest('aside,header,.canvas-heading'))return;
 const p=profiles.find(x=>x.binding.mouse===e.button);if(!p)return;e.preventDefault();if(active?.id===p.id&&p.mode==='click')close();else open(p);
});
document.addEventListener('pointerup',e=>{if(active?.mode==='hold'&&active.binding.mouse===e.button){const n=hover?.node;if(n&&!n.children&&!n.nav)launch(n);else{close();status('Released without a star · cancelled');}}});
document.addEventListener('auxclick',e=>{if(profiles.some(p=>p.binding.mouse===e.button))e.preventDefault();});
document.addEventListener('contextmenu',e=>{if(profiles.some(p=>p.binding.mouse===2))e.preventDefault();});
window.addEventListener('blur',()=>{recording=null;renderProfiles();if(active){close();status('Focus lost · cancelled');}});
document.addEventListener('visibilitychange',()=>{if(document.hidden)close();});
new ResizeObserver(()=>{if(active){anchor={x:stage.clientWidth/2,y:stage.clientHeight/2};hover=null;cancelPending();render();}}).observe(stage);
$('open').onclick=()=>open({...profiles[0],id:'preview',mode:'click',hover:profiles[0]?.hover??true});
$('addProfile').onclick=()=>{const p={...defaultProfiles()[0],binding:{code:'KeyF',ctrl:true,shift:true,alt:true,meta:false}};profiles.push(p);close();recording=p.id;renderProfiles();};
$('radius').value=radius;$('delay').value=delay;$('sizeValue').textContent=radius+' px';$('delayValue').textContent=delay+' ms';
$('radius').oninput=e=>{radius=+e.target.value;$('sizeValue').textContent=radius+' px';hover=null;cancelPending();render();persist();};
$('delay').oninput=e=>{delay=+e.target.value;$('delayValue').textContent=delay+' ms';cancelPending();persist();};
$('edges').onchange=()=>{hover=null;cancelPending();render();};
$('addItem').onsubmit=e=>{e.preventDefault();const name=$('itemName').value.trim();if(!name)return;find(tree,$('parent').value).children.push($('kind').value==='folder'?folder(name,[]):star(name));$('itemName').value='';refreshParents();persist();hover=null;render();status(`Added “${name}”`);};
$('stress').onclick=()=>{for(let i=0;i<12;i++)tree.children.push(star(`Extra star ${tree.children.length+1}`));refreshParents();persist();hover=null;render();};
$('reset').onclick=()=>{close();tree=initialTree();profiles=defaultProfiles();radius=144;delay=60;$('radius').value=radius;$('delay').value=delay;$('sizeValue').textContent='144 px';$('delayValue').textContent='60 ms';persist();renderProfiles();refreshParents();status('Experiment reset');};
renderProfiles();refreshParents();
