const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const assert = require('node:assert/strict');
(async () => {
 for (const file of ['Gw.js', 'Gw.jspi.js']) {
  const source = fs.readFileSync(path.resolve(__dirname, '../../../web', file), 'utf8');
  const line = source.split('\n').find(l => l.includes('const readPromise = Module.image.readAsync'));
  assert.ok(line, 'actual generated read callback exists');
  const fn = line.slice(line.indexOf(':') + 1).trim().replace(/,\s*$/, '');
  for (const reject of [false, true]) {
   let fatal = 0, complete = 0;
   const Module = {image: {readAsync: () => reject ? Promise.reject(new Error('synthetic read failure')) : Promise.resolve()}};
   const read = vm.runInNewContext(`(${fn})`, {Module, _EmscriptenExeFileFatalImageReadError: () => fatal++, _EmscriptenExeFileOnImageAsyncReadComplete: () => complete++});
   for (let i=0;i<100;i++) read(1, 0, 0, 16, 1, 0);
   await new Promise(setImmediate);
   assert.equal(Module.imageReads.size, reject ? 100 : 0);
   console.log(JSON.stringify({file, reject, reads:100, retained:Module.imageReads.size, fatal, complete}));
  }
 }
})();
