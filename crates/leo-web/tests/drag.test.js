const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function fixture({document = {}, ...extra} = {}) {
  const listeners = {};
  const context = {folderLabel: value => value, felix:{NOTE_DRAG:'application/x-leo-note'},clearTimeout(){},document:{
    addEventListener(type, fn) { listeners[type] = fn; }, querySelectorAll:() => [],
    body:{classList:{remove(){},add(){},toggle(){}}}, ...document
  }, ...extra};
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

test('holding a note drag on another note row arms combine, and leaving the row disarms it', () => {
  const timers = [];
  const classes = new Set();
  const row = {dataset:{id:'b', title:'Graphs', from:'', dropDir:''}, style:{setProperty(){}},
    classList:{add:(...names) => names.forEach(n => classes.add(n)), remove:(...names) => names.forEach(n => classes.delete(n)), toggle:(name, on) => on ? classes.add(name) : classes.delete(name)},
    closest:(selector) => selector === '.side-tree' ? tree : (selector.includes('side-note') || selector.includes('[data-drop-dir]') && !selector.includes('side-folder')) ? row : null};
  const tree = {getBoundingClientRect:() => ({top:-100, bottom:100})};
  const listeners = fixture({setTimeout: (fn) => { timers.push(fn); return timers.length; }, document:{querySelectorAll:(selector) => selector.includes('combine') && classes.size ? [row] : []}});
  const source = {dataset:{id:'a', title:'BFS', from:''}, addEventListener(){}};
  listeners.dragstart({target:{closest:(selector) => selector.startsWith('.card') ? source : null}, dataTransfer:{setData(){}}});
  const over = (target) => {
    let allowed = false;
    listeners.dragover({target, clientY:0, defaultPrevented:false, preventDefault(){allowed = true;}, dataTransfer:{types:['application/x-leo-note']}});
    return allowed;
  };
  assert.equal(over(row), false, 'a quick pass over a note in the same folder is not a drop');
  assert.ok(classes.has('combine-wait'));
  assert.equal(timers.length, 1);
  over(row);
  assert.equal(timers.length, 1, 'staying on the row keeps the one timer');
  timers[0]();
  assert.ok(classes.has('combine-armed') && !classes.has('combine-wait'));
  assert.equal(over(row), true, 'an armed row takes the drop');
  over({closest:() => null});
  assert.equal(classes.size, 0);
  assert.equal(over(row), false, 'coming back starts the hold again');
  assert.equal(timers.length, 2);
});
