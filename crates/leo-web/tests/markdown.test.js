const assert = require('node:assert/strict');
const md = require('../src/web/markdown.js');
const { render } = md;

const cases = [];
const test = (name, fn) => cases.push([name, fn]);

const boxes = (html) =>
  [...html.matchAll(/<input type="checkbox" data-box="(\d+)"( checked)?>/g)].map(
    (m) => [Number(m[1]), Boolean(m[2])]
  );

test('text is escaped, so a note cannot inject markup', () => {
  const html = render('<script>alert(1)</script> & "quotes"');
  assert.ok(!html.includes('<script>'), html);
  assert.ok(html.includes('&lt;script&gt;'), html);
  assert.ok(html.includes('&amp;'), html);
});

test('headings start below the note title', () => {
  assert.ok(render('# Graphs').includes('<h2>Graphs</h2>'));
  assert.ok(render('## BFS').includes('<h3>BFS</h3>'));
  assert.ok(render('###### deep').includes('<h6>deep</h6>'));
});

test('bold, italics, strikethrough and inline code', () => {
  const html = render('**bold** *it* _also_ ~~gone~~ `x < y`');
  assert.ok(html.includes('<strong>bold</strong>'), html);
  assert.ok(html.includes('<em>it</em>'), html);
  assert.ok(html.includes('<em>also</em>'), html);
  assert.ok(html.includes('<del>gone</del>'), html);
  assert.ok(html.includes('<code>x &lt; y</code>'), html);
});

test('formatting inside inline code is left alone', () => {
  assert.ok(render('`**not bold**`').includes('<code>**not bold**</code>'));
});

test('snake_case words are not italic', () => {
  assert.ok(!render('call my_long_name here').includes('<em>'));
});

test('links open in a new tab without passing anything along', () => {
  const html = render('[docs](https://example.com/a?b=1)');
  assert.ok(
    html.includes('<a href="https://example.com/a?b=1" target="_blank" rel="noopener noreferrer">docs</a>'),
    html
  );
});

test('only web and mail links become links', () => {
  const html = render('[x](javascript:alert(1)) [y](data:text/html,hi)');
  assert.ok(!html.includes('<a '), html);
});

test('bare addresses become links', () => {
  const html = render('see https://github.com/chewton2k/leo-cli.');
  assert.ok(html.includes('href="https://github.com/chewton2k/leo-cli"'), html);
  assert.ok(html.includes('</a>.'), html);
});

test('fenced code keeps its text and says its language', () => {
  const html = render('```rust\nfn main() { let x = 1 < 2; }\n**no**\n```');
  assert.ok(html.includes('<pre><code class="language-rust">'), html);
  assert.ok(html.includes('1 &lt; 2'), html);
  assert.ok(html.includes('**no**'), html);
});

test('checkbox numbers match the ones leo ticks', () => {
  const html = render('- [ ] a\ntext\n  - [x] b\n- [X] c\n- [ ]\n');
  assert.deepEqual(boxes(html), [
    [1, false],
    [2, true],
    [3, true],
    [4, false],
  ]);
});

test('a checkbox with extra spaces after the dash is a checkbox too', () => {
  assert.deepEqual(boxes(render('-   [ ] wide\n*  [x] also\n- [ ] plain')), [[1, false], [2, true], [3, false]]);
});

test('a checkbox inside a code block still counts, but is not a checkbox', () => {
  const html = render('```\n- [ ] in code\n```\n- [ ] real');
  assert.deepEqual(boxes(html), [[2, false]]);
});

test('a checkbox in a quote does not count, as leo does not tick it', () => {
  const html = render('> - [ ] quoted\n\n- [ ] real');
  assert.deepEqual(boxes(html), [[1, false]]);
});

test('numbered and nested lists', () => {
  const html = render('1. one\n2. two\n   - inner\n3. three');
  assert.ok(html.includes('<ol>'), html);
  assert.ok(html.includes('<li>two<ul><li>inner</li></ul></li>'), html);
});

test('tables', () => {
  const html = render('| a | b |\n|---|:-:|\n| 1 | **2** |');
  assert.ok(html.includes('<table>'), html);
  assert.ok(html.includes('<th>a</th>'), html);
  assert.ok(html.includes('<td><strong>2</strong></td>'), html);
});

test('quotes and rules', () => {
  const html = render('> said\n\n---\n\nafter');
  assert.ok(html.includes('<blockquote><p>said</p></blockquote>'), html);
  assert.ok(html.includes('<hr>'), html);
});

test('lines of a paragraph keep their breaks', () => {
  assert.ok(render('one\ntwo').includes('<p>one<br>two</p>'));
});

test('an answered @leo question reads as a question and answer', () => {
  const html = render('**Q:** what is BFS?\nIt explores level by level.');
  assert.ok(html.includes('<strong>Q:</strong> what is BFS?'), html);
});

test('pictures in a note are shown from leo, and outside ones stay links', () => {
  const html = md.render('![Heap diagram](attachments/heap.png)\n\n![](<attachments/a b.png>)\n\n![[board.png|300]]\n\n![x](https://example.com/x.png)', { dir: 'cs130' });
  assert.ok(html.includes('<img class="note-img" src="/api/image?path=attachments%2Fheap.png&amp;from=cs130" alt="Heap diagram" loading="lazy">'), html);
  assert.ok(html.includes('src="/api/image?path=attachments%2Fa%20b.png&amp;from=cs130" alt=""'), html);
  assert.ok(html.includes('src="/api/image?path=board.png&amp;from=cs130" alt="board.png"'), html);
  assert.ok(html.includes('<a href="https://example.com/x.png"'), html);
  assert.ok(!/<img[^>]*example\.com/.test(html), 'no picture is loaded from elsewhere');
  assert.ok(!md.render('![a](javascript:alert(1))').includes('<img'));
  assert.ok(!md.render('![a](data:image/png;base64,AAAA)').includes('<img'));
  assert.ok(md.render('![a"><script>](x.png)').includes('alt="a&quot;&gt;&lt;script&gt;"'));
  assert.equal(md.plain('Look ![Heap](attachments/h.png) and ![[b.png]] then [link](https://x.y)'), 'Look and then link');
  assert.equal(md.plain('Sizes:\n\n| a | b |\n|---|:--:|\n| 1 | 2 |'), 'Sizes: a b 1 2');
});


let failed = 0;
test('a mermaid block becomes a diagram slot that keeps its source, escaped', () => {
  const html = render('```mermaid\nflowchart LR\n  A["<b>x</b>"] --> B\n```\n- [ ] after');
  assert.match(html, /<figure class="diagram"><pre class="diagram-src"><code>flowchart LR\n  A\[&quot;&lt;b&gt;x&lt;\/b&gt;&quot;\] --&gt; B<\/code><\/pre><\/figure>/);
  assert.deepEqual(boxes(html), [[1, false]]);
  assert.doesNotMatch(render('```js\nx\n```'), /diagram/);
});

test('a [[Title]] link to another note opens it, with an optional label, and stays escaped', () => {
  assert.equal(
    render('See [[Graph traversals]] and [[Heaps|heaps]].'),
    '<p>See <a class="wiki-link" href="#/search/Graph%20traversals" data-action="open-title" data-title="Graph traversals">Graph traversals</a> and <a class="wiki-link" href="#/search/Heaps" data-action="open-title" data-title="Heaps">heaps</a>.</p>'
  );
  assert.match(render('[[A <b> & "c"]]'), /data-title="A &lt;b&gt; &amp; &quot;c&quot;">A &lt;b&gt; &amp; &quot;c&quot;<\/a>/);
  assert.match(render('[[Lecture#Part 2]]'), /data-title="Lecture">Lecture<\/a>/);
  assert.doesNotMatch(render('![[pic.png]]'), /wiki-link/);
});

test('math in LaTeX becomes slots the page draws, and prices stay prices', () => {
  assert.equal(render('Energy $E = mc^2$ here.'), '<p>Energy <span class="math" data-tex="E = mc^2">E = mc^2</span> here.</p>');
  assert.equal(render('It costs $5 and $10.'), '<p>It costs $5 and $10.</p>');
  assert.doesNotMatch(render('Keep \\$x$ literal'), /class="math"/);
  assert.doesNotMatch(render('`$a$` is code'), /class="math"/);
  assert.equal(render('$$\n\\frac{a}{b} < c\n$$'), '<div class="math math-block" data-tex="\\frac{a}{b} &lt; c">\\frac{a}{b} &lt; c</div>');
  assert.equal(render('$$ x^2 $$'), '<div class="math math-block" data-tex="x^2">x^2</div>');
  assert.match(render('$$\nx\n$$\n- [ ] after'), /data-box="1"/);
});

test('Obsidian callouts become boxes, and a - or + makes them fold', () => {
  assert.equal(
    render('> [!example]- How the loop works\n> It walks the **queue**.'),
    '<details class="callout callout-example"><summary>How the loop works</summary><div class="callout-body"><p>It walks the <strong>queue</strong>.</p></div></details>'
  );
  assert.match(render('> [!tip]+ Open by default\n> x'), /<details class="callout callout-tip" open><summary>Open by default<\/summary>/);
  assert.equal(render('> [!warning]\n> Careful'), '<div class="callout callout-warning"><div class="callout-title">Warning</div><div class="callout-body"><p>Careful</p></div></div>');
  assert.match(render('> [!note"><img src=x>] t\n> x'), /^<blockquote>/);
  assert.match(render('> plain quote'), /^<blockquote>/);
});

for (const [name, fn] of cases) {
  try {
    fn();
    console.log(`ok   ${name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${name}\n     ${e.message.split('\n').join('\n     ')}`);
  }
}
console.log(`${cases.length - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
