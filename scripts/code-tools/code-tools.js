import { EditorState, EditorSelection, countColumn } from '@codemirror/state';
import { ensureSyntaxTree, syntaxTree, indentRange, indentUnit, indentOnInput } from '@codemirror/language';
import { insertNewlineAndIndent, indentMore, indentLess, deleteCharBackward } from '@codemirror/commands';
import { closeBrackets, insertBracket, deleteBracketPair } from '@codemirror/autocomplete';
import { cpp } from '@codemirror/lang-cpp';
import { rust } from '@codemirror/lang-rust';
import { java } from '@codemirror/lang-java';
import { python } from '@codemirror/lang-python';
import { sql } from '@codemirror/lang-sql';
import { javascript } from '@codemirror/lang-javascript';
import { go } from '@codemirror/lang-go';
import { highlightTree, tagHighlighter, tags } from '@lezer/highlight';

const MOST_CODE = 32000;
const PARSE_MS = 75;
const MOST_INDENT_CONTEXT = 32;
const modes = { cpp: cpp(), rust: rust(), java: java(), python: python(), sql: sql(), javascript: javascript({ jsx: true }), typescript: javascript({ typescript: true, jsx: true }), go: go() };
const aliases = { 'c++': 'cpp', cxx: 'cpp', cc: 'cpp', rs: 'rust', py: 'python', python3: 'python', 'python 3': 'python', js: 'javascript', jsx: 'javascript', nodejs: 'javascript', 'node.js': 'javascript', ts: 'typescript', tsx: 'typescript', golang: 'go', postgresql: 'sql', postgres: 'sql', mysql: 'sql', sqlite: 'sql' };
const escape = (s) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const highlighter = tagHighlighter([
  { tag: tags.keyword, class: 'code-keyword' },
  { tag: [tags.string, tags.character], class: 'code-string' },
  { tag: tags.comment, class: 'code-comment' },
  { tag: [tags.number, tags.bool, tags.null], class: 'code-number' },
  { tag: [tags.typeName, tags.className], class: 'code-type' },
  { tag: tags.function(tags.variableName), class: 'code-function' },
  { tag: tags.operator, class: 'code-operator' },
]);

function language(name) {
  const key = String(name || '').trim().toLowerCase();
  const id = Object.hasOwn(aliases, key) ? aliases[key] : key;
  return Object.hasOwn(modes, id) ? id : null;
}

function highlight(value, name) {
  const id = language(name);
  if (!id || typeof value !== 'string' || value.length > MOST_CODE) return null;
  const out = [];
  let at = 0;
  highlightTree(modes[id].language.parser.parse(value), highlighter, (from, to, cls) => {
    out.push(escape(value.slice(at, from)), `<span class="${cls}">${escape(value.slice(from, to))}</span>`);
    at = to;
  });
  out.push(escape(value.slice(at)));
  return out.join('');
}

function indent(value, from, to, name) {
  const id = language(name);
  if (!id || typeof value !== 'string' || value.length > MOST_CODE || !Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < from || to > value.length) return null;
  const state = EditorState.create({ doc: value, extensions: [modes[id], indentUnit.of(id === 'go' ? '\t' : '    '), EditorState.tabSize.of(4)] });
  const tree = ensureSyntaxTree(state, value.length, PARSE_MS);
  if (!tree) return null;
  const protectedRanges = [];
  highlightTree(tree, highlighter, (start, end, cls) => {
    if (/code-string|code-comment/.test(cls)) protectedRanges.push([start, end]);
  });
  const changes = [];
  let protectedAt = 0;
  indentRange(state, 0, value.length).iterChanges((start, end, _a, _b, insert) => {
    while (protectedAt < protectedRanges.length && protectedRanges[protectedAt][1] <= start) protectedAt++;
    const range = protectedRanges[protectedAt];
    if (range && range[0] < end && range[1] > start) return;
    changes.push({ from: start, to: end, insert });
  });
  const change = state.changes(changes);
  if (change.newLength > MOST_CODE) return null;
  return { value: change.apply(state.doc).toString(), from: change.mapPos(from), to: change.mapPos(to) };
}

function difference(before, after) {
  let from = 0, to = before.length, end = after.length;
  while (from < to && from < end && before[from] === after[from]) from++;
  const split = (s, at) => at > 0 && /[\uD800-\uDBFF]/.test(s[at - 1]) && /[\uDC00-\uDFFF]/.test(s[at] || '');
  if (split(before, from) || split(after, from)) from--;
  while (to > from && end > from && before[to - 1] === after[end - 1]) { to--; end--; }
  if (split(before, to) || split(after, end)) { to++; end++; }
  return { from, to, insert: after.slice(from, end) };
}

function keepIndent(state, transaction) {
  const at = state.selection.main.from, line = state.doc.lineAt(at);
  const leading = /^[\t ]*/.exec(line.text)[0];
  const selection = transaction.state.selection.main, next = transaction.newDoc.lineAt(selection.from);
  if (countColumn(transaction.newDoc.sliceString(next.from, selection.from), 4) >= countColumn(leading, 4)) return transaction;
  const pairs = { '(': ')', '[': ']', '{': '}' };
  const tree = syntaxTree(state);
  const tail = tree.resolveInner(at, -1);
  const end = /Comment$/.test(tail.name) && tail.from >= line.from ? tail.from : at;
  const before = state.sliceDoc(line.from, end).replace(/[\t ;]*$/, '');
  const closingAt = /[)\]}]$/.test(before) ? line.from + before.length : at;
  let node = tree.resolveInner(closingAt, -1);
  for (let depth = 0; node && depth < MOST_INDENT_CONTEXT; depth++, node = node.parent) {
    const first = node.firstChild, last = node.lastChild;
    if (!first || !Object.hasOwn(pairs, first.name)) continue;
    const closed = last?.name === pairs[first.name];
    if (first.to <= at && (!closed || last.to > at) && (first.name !== '{' || first.from >= line.from)) return transaction;
    if (closed && last.to === closingAt && first.from < line.from) return transaction;
  }
  return transaction.state.update({ changes: { from: next.from, to: selection.from, insert: leading }, selection: EditorSelection.cursor(next.from + leading.length) });
}

function editor(name) {
  const id = language(name);
  let state = EditorState.create({ extensions: [id ? modes[id] : [], indentUnit.of('    '), EditorState.tabSize.of(4), indentOnInput(), closeBrackets()] });
  return {
    edit(value, from, to, key, shift = false, direction = 'forward') {
      if (typeof value !== 'string' || value.length > MOST_CODE || !Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < from || to > value.length) return null;
      if (id === 'rust' && key === "'" && from === to) return null;
      const before = state.doc.toString();
      state = state.update({ ...(before !== value ? { changes: difference(before, value) } : {}), selection: EditorSelection.range(direction === 'backward' ? to : from, direction === 'backward' ? from : to) }).state;
      if (key !== 'Tab') ensureSyntaxTree(state, to, PARSE_MS);
      let transaction = null;
      const target = { state, dispatch: tr => { transaction = tr; } };
      if (key === 'Enter') insertNewlineAndIndent(target);
      else if (key === 'Tab' && shift) indentLess(target);
      else if (key === 'Tab' && from !== to) indentMore(target);
      else if (key === 'Tab') {
        const column = countColumn(state.sliceDoc(state.doc.lineAt(from).from, from), 4);
        transaction = state.update(state.replaceSelection(' '.repeat(4 - column % 4)), { userEvent: 'input.type' });
      } else if (key === 'Backspace') {
        if (!deleteBracketPair(target) && from === to && from > state.doc.lineAt(from).from && !/\S/.test(state.sliceDoc(state.doc.lineAt(from).from, from))) deleteCharBackward(target);
      } else if ('()[]{}\'"`'.includes(key) && key.length === 1) {
        transaction = insertBracket(state, key);
        if (!transaction && /^[\t ]*$/.test(state.sliceDoc(state.doc.lineAt(from).from, from)) && ')]}'.includes(key)) transaction = state.update(state.replaceSelection(key), { userEvent: 'input.type' });
      }
      if (!transaction || transaction.newDoc.length > MOST_CODE) return null;
      if (key === 'Enter') transaction = keepIndent(state, transaction);
      if (transaction.newDoc.length > MOST_CODE) return null;
      state = transaction.state;
      const result = state.doc.toString(), selection = state.selection.main;
      return { value: result, from: selection.from, to: selection.to, direction: selection.anchor > selection.head ? 'backward' : 'forward', change: difference(value, result) };
    },
  };
}

globalThis.leoCode = { language, highlight, indent, editor, difference };
