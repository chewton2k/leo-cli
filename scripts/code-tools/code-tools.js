import { EditorState } from '@codemirror/state';
import { ensureSyntaxTree, indentRange, indentUnit } from '@codemirror/language';
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

globalThis.leoCode = { language, highlight, indent };
