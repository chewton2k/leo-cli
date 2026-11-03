const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function fixture() {
  const listeners = {};
  const context = {folderLabel: value => value, felix:{NOTE_DRAG:'application/x-leo-note'},clearTimeout(){},document:{
    addEventListener(type, fn) { listeners[type] = fn; }, querySelectorAll:() => [],
    body:{classList:{remove(){},add(){},toggle(){}}}
  }};
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '../src/web/app/drag.js'), 'utf8'), context);
  return listeners;
}

test('only intentional note and folder sources can start a native drag', () => {
  const listeners = fixture();
  for (const kind of ['note', 'folder', 'link', 'image', 'selection', 'control']) {
    let prevented = false;
    const data = new Map();
    const source = {dataset:{id:'note-id',title:'My note',from:'',dir:'A'}, addEventListener(){}};
    listeners.dragstart({target:{closest(selector) {
      return kind === 'note' && selector.startsWith('.card') || kind === 'folder' && selector.startsWith('[data-action') ? source : null;
    }},preventDefault(){prevented = true;},dataTransfer:{setData:(key,value) => data.set(key,value)}});
    assert.equal(prevented, !['note','folder'].includes(kind), kind);
    assert.equal(data.has('application/x-leo-note'), kind === 'note', kind);
    assert.equal(data.has('application/x-leo-folder'), kind === 'folder', kind);
  }
});
