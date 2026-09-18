#!/usr/bin/env node
// Exact-artifact, offline investigation of saved-credential request prerequisite.
// Original login handler, settings accessors and credential request functions run.
// UI services and settings-source operation 980 replaced IN MEMORY only. No client artifact,
// saved profile, keychain, network or game process is accessed or changed.
// Exercises production-derived artifact with real getter and login handler.
// Credential presence uses production export. Separate regression fixtures write
// synthetic persisted settings only to check the disabled bridge preserves them.
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [runtime, artifactPath] = process.argv.slice(2);
assert.ok(['jspi', 'asyncify'].includes(runtime) && artifactPath,
  'Usage: node scripts/probe-wasm-launcher-prefill.mjs <jspi|asyncify> <derived.wasm>');
const original = readFileSync(new URL(runtime === 'jspi' ? '../web/Gw.jspi.wasm' : '../web/Gw.wasm', import.meta.url));
const originalHash = runtime === 'jspi'
  ? '1eb07332632e2fca8aabf5baa14fa1a1e6a2a59ec7134dfb8f6231d924c9fd7b'
  : '373c65abcfe6c26161ffd59cb8ea298eb3d1814f4778aeaf24d8978c1fdfa8ef';
assert.equal(createHash('sha256').update(original).digest('hex'), originalHash,
  'Client changed: recertify fixture function identities');
const source = readFileSync(artifactPath);
const u=n=>{let a=[];do{let b=n&127;n>>>=7;a.push(b|(n?128:0));}while(n);return Buffer.from(a)};
const r=(b,p)=>{let n=0,s=0,v;do{v=b[p++];n|=(v&127)<<s;s+=7;}while(v&128);return[n>>>0,p]};
const name=s=>Buffer.concat([u(s.length),Buffer.from(s)]);
const sections=[];
for(let p=8;p<source.length;){const id=source[p++];const [size,next]=r(source,p);p=next;sections.push({id,body:source.subarray(p,p+size)});p+=size;}
const types=[]; const tb=sections.find(x=>x.id===1).body;let [cnt,p]=r(tb,0);
for(let i=0;i<cnt;i++){assert.equal(tb[p++],96);let [n,q]=r(tb,p);p=q;const params=[...tb.subarray(p,p+n)];p+=n;[n,q]=r(tb,p);p=q;types.push({params,results:[...tb.subarray(p,p+n)]});p+=n;}
const fb=sections.find(x=>x.id===3).body;[cnt,p]=r(fb,0);const funcs=[];for(let i=0;i<cnt;i++){let[n,q]=r(fb,p);p=q;funcs.push(n);}
const origModule=new WebAssembly.Module(source);const imp=WebAssembly.Module.imports(origModule);assert.ok(imp.every(x=>x.kind==='function'));const imported=imp.length;
// Fixture preserves settings and credential-request functions. Production may
// rewrite the reviewed login initializer to seed a managed email address.
const bodies = bytes => {
 let code;
 for (let p=8;p<bytes.length;) {
  const id=bytes[p++]; const [size,next]=r(bytes,p);p=next;
  if(id===10)code=bytes.subarray(p,p+size);
  p+=size;
 }
 const [count,start]=r(code,0);let p=start;const result=[];
 for(let i=0;i<count;i++) {const [size,next]=r(code,p);p=next;result.push(code.subarray(p,p+size));p+=size;}
 return result;
};
assert.deepEqual(WebAssembly.Module.imports(origModule), WebAssembly.Module.imports(new WebAssembly.Module(original)));
const originalBodies=bodies(original), derivedBodies=bodies(source);
for(const index of [354,358,10786,10845,10153,10154,9975]) {
 assert.deepEqual(derivedBodies[index-imported],originalBodies[index-imported],`Unexpected production change to client function ${index}`);
}
const stubs=new Map([5864,5865,6676,6695,6704,6730,6744,6750,6792,6838,6839,10491,11093,11191,11192,10150,980,6842,6843,11303].map(n=>[n,0]));
for(const section of sections){let b=section.body;
 if(section.id===7){const[n,q]=r(b,0);const extras=[['initSettings',10786],['getSetting',10797],['loginUi',11302]];section.body=Buffer.concat([u(n+extras.length),b.subarray(q),...extras.map(([s,f])=>Buffer.concat([name(s),Buffer.from([0]),u(f)]))]);}
 if(section.id===10){let[n,q]=r(b,0);const parts=[u(n)];for(let i=0;i<n;i++){let[size,next]=r(b,q);q=next;let body=b.subarray(q,q+size);q+=size;const index=i+imported;if(index===11195){assert.equal(types[funcs[i]].results.length,0);body=Buffer.from([0,65,251,0,32,0,32,1,16,1,26,11]);}else if(index===6796){const type=types[funcs[i]];assert.deepEqual(type,{params:[127,127],results:[127]});body=Buffer.from([0,65,252,0,32,0,32,1,16,1,11]);}else if(index===11188){assert.deepEqual(types[funcs[i]],{params:[127,127,127],results:[]});body=Buffer.from([0,65,253,0,32,2,65,0,16,1,26,11]);}else if(stubs.has(index)){const type=types[funcs[i]];assert.ok(type.results.length<=1);assert.ok(type.results.length===0||type.results[0]===127);body=Buffer.from(type.results.length?[0,65,0,11]:[0,11]);}parts.push(u(body.length),body);}section.body=Buffer.concat(parts);}
}
const module=new WebAssembly.Module(Buffer.concat([source.subarray(0,8),...sections.flatMap(s=>[Buffer.from([s.id]),u(s.body.length),s.body])]));
const results=[];
for(const flag of [0,1,0]){
 let e;let requests=0;let assignedName;let assignmentArgs;let fixturePtr;let currentName='';let widgetReads=0;const imports={};
 const alloc=bytes=>{const ptr=e.malloc(bytes.length);new Uint8Array(e.memory.buffer,ptr,bytes.length).set(bytes);return ptr;};
 for(const item of imp)(imports[item.module]??={})[item.name]=(...args)=>{
  if(item.name==='emscripten_get_now')return 0;
  if(item.name==='emscripten_asm_const_ptr'&&args[0]===2656343)return alloc(Buffer.from('en\0'));
  if(item.name==='emscripten_asm_const_int'&&args[0]===2666470)return 1;
  if(item.name==='emscripten_asm_const_int'&&args[0]===123){assignmentArgs=args.map(Number);assignedName=args[2];return 0;}
  if(item.name==='emscripten_asm_const_int'&&args[0]===124)return fixturePtr;
  if(item.name==='emscripten_asm_const_int'&&args[0]===125){widgetReads++;const out=args[1];new Uint8Array(e.memory.buffer).set(Buffer.from(`${currentName}\0`,'utf16le'),out);return 0;}
  if(item.name==='emscripten_asm_const_int'&&args[0]===2666524){requests++;return 0;}
  if(item.name==='emscripten_asm_const_int'&&args[0]===2656136){const vals=new Uint32Array(e.memory.buffer,args[2],3);const cstr=p=>{const h=new Uint8Array(e.memory.buffer);let end=p;while(h[end])end++;return Buffer.from(h.subarray(p,end)).toString()};throw Error('client assertion '+cstr(vals[0])+' '+cstr(vals[1])+':'+vals[2]);}
  throw Error('Blocked import '+item.name+' '+args[0]);
 };
 e=new WebAssembly.Instance(module,imports).exports;
 assert.equal(typeof e.GwnativeSetLauncherCredentialsAvailable, 'function', 'Production prefill bridge missing');
 // Real harness enables this before constructors/main; the setting loader
 // must not clear it. This request-path check starts with untouched settings.
 e.GwnativeSetLauncherCredentialsAvailable(flag);
 if (flag) {
  assert.equal(typeof e.GwnativeSetLauncherAccountName, 'function', 'Managed name bridge missing');
  const name=alloc(Buffer.from('launcher-fixture@example.invalid\0','utf16le'));
  e.GwnativeSetLauncherAccountName(name);
 }
 e.emscripten_stack_init();
 e.__wasm_call_ctors();
 e.initSettings(1458736);
 const utf16=pointer=>{let value='';const words=new Uint16Array(e.memory.buffer);for(let i=pointer/2;words[i];i++)value+=String.fromCharCode(words[i]);return value;};
 assert.equal(e.getSetting(95),flag);
 assert.equal(new Uint32Array(e.memory.buffer)[(5940704+95*4)/4], 0, 'Persisted setting was changed');
 const str=p=>{const h=new Uint16Array(e.memory.buffer);let out='';for(let i=p/2;h[i]&&out.length<100;i++)out+=String.fromCharCode(h[i]);return out};
 assert.equal(str(1493662), 'BtnRememberPass');
 // Client's own boolean-setting descriptor and persisted INI section identify
 // this gate; names are not inferred solely from the nearby checkbox label.
 const descriptor95 = new Uint32Array(e.memory.buffer, 1456208 + 95 * 12, 3);
 assert.equal(str(descriptor95[0]), 'SavePassword');
 assert.equal(str(1463970), 'Prefs');
 // Toggling is per-instance and reversible, even after settings initialize.
 e.GwnativeSetLauncherCredentialsAvailable(0);
 assert.equal(e.getSetting(95),0);
 // Disabled bridge must preserve an existing client-side remembered-password
 // preference rather than masking it with its zero default.
 new Uint32Array(e.memory.buffer)[(5940704+95*4)/4]=1;
 assert.equal(e.getSetting(95),1);
 new Uint32Array(e.memory.buffer)[(5940704+95*4)/4]=0;
 const settings=new Uint32Array(e.memory.buffer);
 settings[(5940704+94*4)/4]=1; settings[(5940704+96*4)/4]=1;
 assert.equal(e.getSetting(94),1); assert.equal(e.getSetting(96),1);
 settings[(5940704+94*4)/4]=0; settings[(5940704+96*4)/4]=0;
 e.GwnativeSetLauncherCredentialsAvailable(flag);
 const state=alloc(Buffer.alloc(16)); const data=alloc(Buffer.alloc(16));const param=alloc(Buffer.alloc(16));const event=alloc(Buffer.alloc(16));
 const set=(ptr,vals)=>new Uint32Array(e.memory.buffer,ptr,vals.length).set(vals);
 set(param,[data]);set(event,[0,9,state]);
 try{e.loginUi(event,param,0);}catch(err){console.error('flag',flag,'requests',requests,err);process.exit(1);}
 if(flag) assert.equal(utf16(assignedName),'launcher-fixture@example.invalid',`Managed name did not reach initial email assignment: ${assignmentArgs}`);
 else assert.equal(assignedName,undefined,'Unmanaged initialization unexpectedly assigned an email');
 // Actual 11302's event-79/event-210 branch: unchanged 354 compares email
 // and unchanged 358 writes password only after a match. UI plumbing is
 // replaced in-memory with marker-routed fixtures; no credentials leave this process.
 fixturePtr=alloc(Buffer.alloc(16));
 const password='synthetic-password-only';
 const consumer=(email,expectPassword)=>{
  currentName=email;
  const readsBefore=widgetReads;
  const context=alloc(Buffer.alloc(256)); const wrapper=alloc(Buffer.alloc(4));
  const rootframe=alloc(Buffer.alloc(16)); const outer=alloc(Buffer.alloc(12));
  const nested=alloc(Buffer.alloc(16)); const payload=alloc(Buffer.alloc(336));
  set(wrapper,[context]); set(outer,[rootframe,79,wrapper]); set(nested,[210,0,0,payload]);
  new Uint8Array(e.memory.buffer).set(Buffer.from('fixture@example.invalid\0','utf16le'),payload+4);
  new Uint8Array(e.memory.buffer).set(Buffer.from(`${password}\0`,'utf16le'),payload+132);
  e.loginUi(outer,nested,0);
  assert.equal(widgetReads,readsBefore+1,'Password consumer did not read fixture email');
  assert.equal(utf16(context+4),expectPassword?password:'',`Password consumer result for ${email||'empty'} current email`);
 };
 consumer('',false);
 consumer('other@example.invalid',false);
 consumer('fixture@example.invalid',true);
 results.push({setting95:flag,requests});
 console.log(JSON.stringify({setting95:flag,requests}));
 assert.equal(requests,flag);
}

console.log(`PASS ${runtime}: managed name reaches login initializer and bridge causes credential request without changing persisted setting. Authentication not tested.`);
