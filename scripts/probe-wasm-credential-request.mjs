#!/usr/bin/env node
// Exact-artifact, offline investigation of saved-credential request prerequisite.
// Original login handler, settings accessors and credential request functions run.
// UI services and settings-source operation 980 replaced IN MEMORY only. No client artifact,
// saved profile, keychain, network or game process is accessed or changed.
// Setting 95 is a controlled fixture input, NOT a proposed production patch.
// --expect-request deliberately fails on zero-setting fixture behavior.
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const source=readFileSync(new URL('../web/Gw.jspi.wasm', import.meta.url));
assert.equal(createHash('sha256').update(source).digest('hex'),'1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b');
const u=n=>{let a=[];do{let b=n&127;n>>>=7;a.push(b|(n?128:0));}while(n);return Buffer.from(a)};
const r=(b,p)=>{let n=0,s=0,v;do{v=b[p++];n|=(v&127)<<s;s+=7;}while(v&128);return[n>>>0,p]};
const name=s=>Buffer.concat([u(s.length),Buffer.from(s)]);
const sections=[];
for(let p=8;p<source.length;){const id=source[p++];const [size,next]=r(source,p);p=next;sections.push({id,body:source.subarray(p,p+size)});p+=size;}
const types=[]; const tb=sections.find(x=>x.id===1).body;let [cnt,p]=r(tb,0);
for(let i=0;i<cnt;i++){assert.equal(tb[p++],96);let [n,q]=r(tb,p);p=q;const params=[...tb.subarray(p,p+n)];p+=n;[n,q]=r(tb,p);p=q;types.push({params,results:[...tb.subarray(p,p+n)]});p+=n;}
const fb=sections.find(x=>x.id===3).body;[cnt,p]=r(fb,0);const funcs=[];for(let i=0;i<cnt;i++){let[n,q]=r(fb,p);p=q;funcs.push(n);}
const origModule=new WebAssembly.Module(source);const imp=WebAssembly.Module.imports(origModule);assert.ok(imp.every(x=>x.kind==='function'));const imported=imp.length;
const stubs=new Map([5864,6676,6695,6704,6730,6744,6750,6792,6838,11093,11191,10150,980].map(n=>[n,0]));
for(const section of sections){let b=section.body;
 if(section.id===7){const[n,q]=r(b,0);const extras=[['initSettings',10786],['getSetting',10797],['loginUi',11302]];section.body=Buffer.concat([u(n+extras.length),b.subarray(q),...extras.map(([s,f])=>Buffer.concat([name(s),Buffer.from([0]),u(f)]))]);}
 if(section.id===10){let[n,q]=r(b,0);const parts=[u(n)];for(let i=0;i<n;i++){let[size,next]=r(b,q);q=next;let body=b.subarray(q,q+size);q+=size;const index=i+imported;if(stubs.has(index)){const type=types[funcs[i]];assert.ok(type.results.length<=1);assert.ok(type.results.length===0||type.results[0]===127);body=Buffer.from(type.results.length?[0,65,0,11]:[0,11]);}parts.push(u(body.length),body);}section.body=Buffer.concat(parts);}
}
const module=new WebAssembly.Module(Buffer.concat([source.subarray(0,8),...sections.flatMap(s=>[Buffer.from([s.id]),u(s.body.length),s.body])]));
const results=[];
for(const flag of [0,1]){
 let e;let requests=0;const imports={};
 const alloc=bytes=>{const ptr=e.malloc(bytes.length);new Uint8Array(e.memory.buffer,ptr,bytes.length).set(bytes);return ptr;};
 for(const item of imp)(imports[item.module]??={})[item.name]=(...args)=>{
  if(item.name==='emscripten_get_now')return 0;
  if(item.name==='emscripten_asm_const_ptr'&&args[0]===2656343)return alloc(Buffer.from('en\0'));
  if(item.name==='emscripten_asm_const_int'&&args[0]===2666470)return 1;
  if(item.name==='emscripten_asm_const_int'&&args[0]===2666524){requests++;return 0;}
  if(item.name==='emscripten_asm_const_int'&&args[0]===2656136){const vals=new Uint32Array(e.memory.buffer,args[2],3);const cstr=p=>{const h=new Uint8Array(e.memory.buffer);let end=p;while(h[end])end++;return Buffer.from(h.subarray(p,end)).toString()};throw Error('client assertion '+cstr(vals[0])+' '+cstr(vals[1])+':'+vals[2]);}
  throw Error('Blocked import '+item.name+' '+args[0]);
 };
 e=new WebAssembly.Instance(module,imports).exports;e.emscripten_stack_init();e.__wasm_call_ctors();e.initSettings(1458736);
 assert.equal(e.getSetting(95),0);
 const str=p=>{const h=new Uint16Array(e.memory.buffer);let out='';for(let i=p/2;h[i]&&out.length<100;i++)out+=String.fromCharCode(h[i]);return out};
 assert.equal(str(1493662), 'BtnRememberPass');
 // Client's own boolean-setting descriptor and persisted INI section identify
 // this gate; names are not inferred solely from the nearby checkbox label.
 const descriptor95 = new Uint32Array(e.memory.buffer, 1456208 + 95 * 12, 3);
 assert.equal(str(descriptor95[0]), 'SavePassword');
 assert.equal(str(1463970), 'Prefs');
 new Uint32Array(e.memory.buffer)[(5940704+95*4)/4]=flag;
 const state=alloc(Buffer.alloc(16)); const data=alloc(Buffer.alloc(16));const param=alloc(Buffer.alloc(16));const event=alloc(Buffer.alloc(16));
 const set=(ptr,vals)=>new Uint32Array(e.memory.buffer,ptr,vals.length).set(vals);
 set(param,[data]);set(event,[0,9,state]);
 try{e.loginUi(event,param,0);}catch(err){console.error('flag',flag,'requests',requests,err);process.exit(1);}
 results.push({setting95:flag,requests});
 console.log(JSON.stringify({setting95:flag,requests}));
 assert.equal(requests,flag);
}

if (process.argv.includes('--expect-request')) {
 assert.equal(results[0].requests, 1, 'Zero-setting login fixture never asks host for saved credentials');
}
console.log('PASS: saved-credential request gate. UI dependencies mocked; settings-source load forced to fail; no field population or authentication tested.');
