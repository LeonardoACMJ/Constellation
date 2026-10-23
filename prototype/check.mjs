import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {webcrypto} from 'node:crypto';
import {point,wedge,entries,fit,TAU} from './geometry.mjs';

const many=Array.from({length:43},(_,i)=>({id:String(i)}));
const visited=[];
for(let p=0;p<entries(many).pages;p++){
 const result=entries(many,p);assert(result.items.length<=9);
 visited.push(...result.items.filter(x=>!x.nav).map(x=>x.id));
}
assert.deepEqual(visited,many.map(x=>x.id));
assert.equal(entries(many,-1).page,entries(many).pages-1);
for(const width of [280,600,1200])for(const height of [440,620])for(const anchor of [{x:0,y:0},{x:width,y:height}]){
 const b={minX:-144,maxX:720,minY:-260,maxY:340},f=fit(b,width,height,anchor);
 assert(f.x+b.minX*f.scale>=37.99);assert(f.x+b.maxX*f.scale<=width-37.99);
 assert(f.y+b.minY*f.scale>=37.99);assert(f.y+b.maxY*f.scale<=height-37.99);
}
assert(!wedge(0,0,40,144,-TAU,0).includes('NaN'));

// Exercise real app handlers in a minimal DOM. This does not verify visual layout.
class Element {
 constructor(tag='div'){this.tagName=tag.toUpperCase();this.children=[];this.attrs={};this.listeners={};this.value='';this.checked=true;this.clientWidth=900;this.clientHeight=620;this.classList={add(){},remove(){}};}
 setAttribute(k,v){this.attrs[k]=v;}
 append(...nodes){this.children.push(...nodes);}
 replaceChildren(...nodes){this.children=nodes;}
 add(o){this.children.push(o);if(this.children.length===1)this.value=o.value;}
 get options(){return this.children;}
 addEventListener(name,fn){(this.listeners[name]??=[]).push(fn);}
 fire(name,event={}){event.target??=this;event.preventDefault??=()=>{};for(const fn of this.listeners[name]||[])fn(event);}
 getBoundingClientRect(){return {left:0,top:0,width:900,height:620};}
 focus(){}
 closest(selector){if(selector==='[data-index]'&&this.attrs['data-index']!==undefined)return this;return null;}
 querySelectorAll(){return [];}
}
const ids=new Map(), document=new Element('document');
document.getElementById=id=>{if(!ids.has(id))ids.set(id,new Element());return ids.get(id);};
document.createElement=tag=>new Element(tag);document.createElementNS=(_,tag)=>new Element(tag);
const window=new Element('window');
const context=vm.createContext({document,window,Option:class{constructor(text,value){this.textContent=text;this.value=value;}},ResizeObserver:class{observe(){}},localStorage:{getItem:()=>null,setItem(){}},crypto:webcrypto,setTimeout,clearTimeout,Math,console,point,wedge,entries,fit,TAU});
const source=(await readFile(new URL('./app.js',import.meta.url),'utf8')).replace(/^import[^\n]+\n/,'');
vm.runInContext(source,context);
const get=id=>ids.get(id), state=code=>vm.runInContext(code,context);
const key=(code,extra={})=>({code,key:code.replace('Key',''),ctrlKey:true,shiftKey:true,altKey:false,metaKey:false,target:new Element(),...extra});
get('open').onclick();assert.equal(state('active.mode'),'click');assert(get('radial').children.length>0);
function sectors(){return get('radial').children[0].children.filter(x=>x._item);}
const create=sectors().find(x=>x._item.node.name==='Create');
get('radial').fire('click',{target:create,button:0});assert.equal(state('path.length'),1);
const writing=sectors().find(x=>x._item.node.name==='Writing');
get('radial').fire('click',{target:writing,button:0});assert.equal(state('path.length'),2);
const notes=sectors().find(x=>x._item.node.name==='Notes');
get('stage').fire('pointermove',{target:notes,clientX:500,clientY:300});
document.fire('keydown',key('Space',{key:' ',ctrlKey:false,shiftKey:false}));
assert.equal(state('active'),null);assert.match(get('status').textContent,/Would launch “Notes”/);
document.fire('keydown',key('KeyG'));assert.equal(state('active.mode'),'hold');
const firefox=sectors().find(x=>x._item.node.name==='Firefox');
get('stage').fire('pointermove',{target:firefox,clientX:500,clientY:300});
document.fire('keyup',key('KeyG'));assert.equal(state('active'),null);assert.match(get('status').textContent,/Firefox/);
document.fire('keydown',key('KeyG'));document.fire('keyup',key('ControlLeft',{key:'Control',ctrlKey:false}));assert.equal(state('active'),null);assert.match(get('status').textContent,/cancelled/);
document.fire('keydown',key('KeyF'));document.fire('keydown',key('KeyF'));assert.equal(state('active'),null);
document.fire('keydown',key('KeyF'));window.fire('blur');assert.equal(state('active'),null);
get('open').onclick();
get('stage').fire('pointermove',{target:sectors().find(x=>x._item.node.name==='Create'),clientX:500,clientY:300});
await new Promise(r=>setTimeout(r,90));assert.equal(state('path.length'),1);
document.fire('keydown',key('Escape',{key:'Escape'}));assert.equal(state('active'),null);
get('stress').onclick();get('stress').onclick();assert.equal(state('tree.children.length'),30);
get('open').onclick();assert(sectors().some(x=>x._item.node.nav));
get('radial').fire('click',{target:sectors().find(x=>x._item.node.nav===1),button:0});assert.equal(state('pages.get(tree.id)'),1);
state('close()');
console.log('PASS: paging coverage, viewport bounds, nested navigation, hover expansion, Space launch, toggle, hold release, modifier release, blur, Escape, density paging.');
