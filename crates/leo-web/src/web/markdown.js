(function (root) {
  'use strict';

  const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const escape = (s) => s.replace(/[&<>"']/g, (c) => ESCAPES[c]);

  const safeUrl = (url) => (/^(https?:|mailto:)/i.test(url.trim()) ? url.trim() : null);

  const link = (url, label) =>
    `<a href="${escape(url)}" target="_blank" rel="noopener noreferrer">${label}</a>`;

  const SPAN_TAGS = { '*1': 'em', '*2': 'strong', '_1': 'em', '_2': 'strong', '~2': 'del' };
  const wordy = (c) => /[\p{L}\p{N}]/u.test(c);

  function spans(s) {
    const bits = s.split(/(\*+|_+|~~)/);
    const runs = [];
    const open = [];
    const beside = (i, step) => {
      for (let k = i + step; k >= 0 && k < bits.length; k += step) if (bits[k]) return step > 0 ? bits[k][0] : bits[k].at(-1);
      return '';
    };
    for (let i = 1; i < bits.length; i += 2) {
      const run = { at: i, mark: bits[i][0], left: bits[i].length, opens: '', shuts: '' };
      runs.push(run);
      const before = beside(i, -1);
      const after = beside(i, 1);
      let canOpen = /\S/.test(after);
      let canClose = /\S/.test(before);
      if (run.mark === '_') {
        canOpen = canOpen && !wordy(before);
        canClose = canClose && !wordy(after);
      }
      if (run.mark === '*' && run.left === 1 && wordy(before)) canOpen = false;
      while (canClose && run.left) {
        let k = open.length - 1;
        while (k >= 0 && open[k].mark !== run.mark) k--;
        if (k < 0) break;
        const first = open[k];
        open.length = k + 1;
        const size = Math.min(first.left, run.left) >= 2 ? 2 : 1;
        const tag = SPAN_TAGS[run.mark + size];
        first.left -= size;
        run.left -= size;
        first.opens = `<${tag}>${first.opens}`;
        run.shuts += `</${tag}>`;
        if (!first.left) open.pop();
      }
      if (canOpen && run.left) open.push(run);
    }
    for (const run of runs) bits[run.at] = run.shuts + run.mark.repeat(run.left) + run.opens;
    return bits.join('');
  }

  const emphasis = (s) => spans(s).replace(/&lt;u&gt;(.+?)&lt;\/u&gt;/g, '<u>$1</u>');

  let pictureDir = '';

  function sized(alt) {
    const found = /^(.*?)\|\s*(\d{1,4})(?:\s*x\s*\d{1,4})?\s*$/.exec(String(alt || ''));
    if (!found) return { alt: String(alt || ''), width: 0 };
    const width = Number(found[2]);
    return { alt: found[1], width: width >= 16 && width <= 4000 ? width : 0 };
  }

  function picture(target, label) {
    const { alt, width } = sized(label);
    const size = width ? ` width="${width}"` : '';
    const src = String(target || '').trim().replace(/^<(.*)>$/, '$1');
    if (/^data:image\/svg\+xml[;,][^\s"'<>]*$/i.test(src)) return `<img class="note-img drawing-img" src="${escape(src)}" alt="${escape(alt)}"${size}>`;
    if (!src || /^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//')) return null;
    const url = `/api/image?path=${encodeURIComponent(src)}&from=${encodeURIComponent(pictureDir)}`;
    return `<img class="note-img" src="${escape(url)}" alt="${escape(alt)}" loading="lazy"${size}>`;
  }

  const MOST_DRAWING = 200000;

  function drawing(source) {
    const svg = source.trim();
    if (!/^(<\?xml[^>]*>\s*)?<svg[\s>]/i.test(svg) || svg.length > MOST_DRAWING) return null;
    if (!/<\/svg>$/i.test(svg)) return '<figure class="drawing pending"><span>Drawing…</span></figure>';
    const named = svg.match(/<title[^>]*>([^<]{1,200})<\/title>/i);
    const alt = named ? named[1].trim() : 'Drawing';
    const open = svg.match(/<svg[^>]*>/i);
    const ready = open && /\sxmlns=/.test(open[0]) ? svg : svg.replace(/<svg/i, '<svg xmlns="http://www.w3.org/2000/svg"');
    return `<figure class="drawing"><img src="data:image/svg+xml;charset=utf-8,${encodeURIComponent(ready)}" alt="${escape(alt)}"></figure>`;
  }

  function inline(text) {
    const slots = [];
    const keep = (html) => `\u0000${slots.push(html) - 1}\u0000`;
    let s = text;
    s = s.replace(/`([^`]+)`/g, (_, code) => keep(`<code>${escape(code)}</code>`));
    const shown = (tex, block) => keep(`<span class="math${block ? ' math-block' : ''}" data-tex="${escape(tex.trim())}">${escape(tex.trim())}</span>`);
    s = s.replace(/\\\[([\s\S]+?)\\\]/g, (_, tex) => shown(tex, true));
    s = s.replace(/\\\(([\s\S]+?)\\\)/g, (_, tex) => shown(tex, false));
    s = s.replace(/(^|[^\\])\$\$([^$\n]+?)\$\$/g, (_, lead, tex) => lead + shown(tex, true));
    s = s.replace(/(^|[^\\$])\$(?!\s)([^$\n]+?)(?<!\s)\$(?!\d)/g, (_, lead, tex) => lead + keep(`<span class="math" data-tex="${escape(tex)}">${escape(tex)}</span>`));
    s = s.replace(/!\[\[([^\]|]+)(?:\|([^\]]*))?\]\]/g, (whole, name, option) => {
      const img = picture(name.trim(), option && /^\s*\d/.test(option) ? `${name.trim()}|${option}` : name.trim());
      return img ? keep(img) : whole;
    });
    s = s.replace(/(?<!!)\[\[([^\[\]|#]+)(?:#[^\[\]|]*)?(?:\|([^\[\]]+))?\]\]/g, (_, title, label) => {
      const name = title.trim();
      return keep(`<a class="wiki-link" href="#/search/${encodeURIComponent(name)}" data-action="open-title" data-title="${escape(name)}">${escape((label || name).trim())}</a>`);
    });
    s = s.replace(/!\[([^\]]*)\]\((<[^>]+>|[^)\s]+)(?:\s+"[^"]*")?\)/g, (whole, alt, target) => {
      const img = picture(target, alt);
      return img ? keep(img) : whole;
    });
    s = s.replace(/!?\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g, (whole, label, url) => {
      const safe = safeUrl(url);
      return safe ? keep(link(safe, emphasis(escape(label || safe)))) : whole;
    });
    s = s.replace(/\bhttps?:\/\/[^\s<>()]+[^\s<>().,;:!?'"\]]/g, (url) => keep(link(url, escape(url))));
    s = emphasis(escape(s));
    return s.replace(/\u0000(\d+)\u0000/g, (_, i) => slots[Number(i)]);
  }

  const FENCE = /^\s*(```|~~~)\s*([\w+-]*)/;
  const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
  const RULE = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
  const MARKER = String.raw`[-*+]|\d+[.)]|(?<low>[a-z])\k<low>?[.)]|(?<up>[A-Z])\k<up>?(?:\)|\.(?=[^\S\n]{2}|\t))`;
  const ITEM = new RegExp(String.raw`^(\s*)(${MARKER})\s+(?<text>.*)$`);
  const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
  const BOX = new RegExp(String.raw`^(?:${MARKER}) {1,4}\[( |x|X)\](?: |$)`);

  const isBox = (line) => BOX.test(line.trimStart());

  const cells = (row) =>
    row
      .trim()
      .replace(/^\|/, '')
      .replace(/(?<!\\)\|$/, '')
      .split(/(?<!\\)\|/)
      .map((c) => c.trim().replace(/\\\|/g, '|'));

  const aligned = (rule) => cells(rule).map((c) => (c.startsWith(':') ? (c.endsWith(':') ? 'center' : 'left') : c.endsWith(':') ? 'right' : ''));
  const cell = (tag, text, align) => `<${tag}${align ? ` class="align-${align}"` : ''}>${inline(text)}</${tag}>`;

  function letterValue(marker) {
    const run = marker.slice(0, -1).toLowerCase();
    const place = run.charCodeAt(0) - 96;
    return run.length === 2 ? place * 27 : place;
  }

  const listKind = (marker) => (/^[-*+]$/.test(marker) ? 'ul' : marker.replace(/\d+/, '1').replace(/[a-z]+/, 'a').replace(/[A-Z]+/, 'A'));

  function startsBlock(line, next) {
    return (
      FENCE.test(line) ||
      HEADING.test(line) ||
      RULE.test(line) ||
      ITEM.test(line) ||
      /^\s*>/.test(line) ||
      /^\s*(\$\$|\\\[\s*$|\\begin\{(?:equation|align|aligned|gather|gathered|multline|alignat|flalign|split|cases|matrix|pmatrix|bmatrix|vmatrix|array)\*?\})/.test(line) ||
      (line.includes('|') && next !== undefined && TABLE_RULE.test(next) && next.includes('-'))
    );
  }

  function list(lines, state) {
    const items = lines.map((line) => {
      const m = line.match(ITEM);
      return { indent: m[1].replace(/\t/g, '    ').length, kind: listKind(m[2]), marker: m[2], raw: line, text: m.groups.text };
    });

    function build(from, indent, depth) {
      const { kind } = items[from];
      const ordered = kind !== 'ul';
      const lettered = ordered && /[a-z]/i.test(kind);
      const paren = kind.endsWith(')') ? '-paren' : '';
      const first = lettered || !ordered ? 1 : parseInt(items[from].marker, 10);
      const style = lettered ? `list-${kind[0] === 'a' ? 'lower' : 'upper'}${paren}` : depth > 1 ? `list-${(depth - 1) % 3 + 1}` : paren && 'list-paren';
      const look = style ? ` class="${style}"` : '';
      let html = ordered ? (first > 1 ? `<ol${look} start="${first}">` : `<ol${look}>`) : `<ul${look}>`;
      let i = from;
      while (i < items.length && items[i].indent >= indent) {
        const item = items[i];
        if (item.indent === indent && item.kind !== kind) break;
        if (item.indent > indent) {
          const [inner, next] = build(i, item.indent, depth + 1);
          html = html.replace(/<\/li>$/, `${inner}</li>`);
          i = next;
          continue;
        }
        let body;
        const task = item.text.match(/^\[( |x|X)\](?:\s+(.*))?$/);
        if (task && isBox(item.raw) && state.interactive) {
          state.boxes++;
          const done = task[1] !== ' ';
          body = `<label class="task${done ? ' done' : ''}"><input type="checkbox" data-box="${state.boxes}"${done ? ' checked' : ''}><span>${inline(task[2] || '')}</span></label>`;
          html += `<li class="task-item">${body}</li>`;
        } else if (task) {
          const done = task[1] !== ' ';
          html += `<li class="task-item"><label class="task${done ? ' done' : ''}"><input type="checkbox" disabled${done ? ' checked' : ''}><span>${inline(task[2] || '')}</span></label></li>`;
        } else {
          html += `<li${lettered ? ` value="${letterValue(item.marker)}"` : ''}>${inline(item.text)}</li>`;
        }
        i++;
      }
      return [html + (ordered ? '</ol>' : '</ul>'), i];
    }

    let html = '';
    let i = 0;
    while (i < items.length) {
      const [block, next] = build(i, items[i].indent, state.depth);
      html += block;
      i = next;
    }
    return html;
  }

  function blocks(lines, state) {
    let html = '';
    let i = 0;
    while (i < lines.length) {
      const line = lines[i];

      if (!line.trim()) {
        i++;
        continue;
      }

      const math = line.trim();
      const env = math.match(/^\\begin\{((?:equation|align|aligned|gather|gathered|multline|alignat|flalign|split|cases|matrix|pmatrix|bmatrix|vmatrix|array)\*?)\}/);
      if (env || (math.startsWith('\\[') && !math.slice(2).includes('\\]') && !math.slice(2).trim())) {
        const end = env ? `\\end{${env[1]}}` : '\\]';
        const body = [env ? math : math.slice(2)];
        let closed = body[0].includes(end) && env;
        i++;
        while (!closed && i < lines.length) {
          const next = lines[i];
          i++;
          if (next.trim().endsWith(end) || next.includes(end)) {
            body.push(env ? next : next.slice(0, next.lastIndexOf(end)));
            closed = true;
          } else body.push(next);
        }
        const tex = body.join('\n').trim();
        html += `<div class="math math-block" data-tex="${escape(tex)}">${escape(tex)}</div>`;
        continue;
      }
      if (math.startsWith('$$')) {
        let tex;
        if (math.length > 4 && math.endsWith('$$')) {
          tex = math.slice(2, -2);
          i++;
        } else {
          const body = [math.slice(2)];
          i++;
          while (i < lines.length && !lines[i].trim().endsWith('$$')) {
            body.push(lines[i]);
            i++;
          }
          if (i < lines.length) body.push(lines[i].trim().slice(0, -2));
          i++;
          tex = body.join('\n');
        }
        html += `<div class="math math-block" data-tex="${escape(tex.trim())}">${escape(tex.trim())}</div>`;
        continue;
      }
      const fence = line.match(FENCE);
      if (fence) {
        const code = [];
        i++;
        while (i < lines.length && !new RegExp('^\\s*' + fence[1]).test(lines[i])) {
          if (state.interactive && isBox(lines[i])) state.boxes++;
          code.push(lines[i]);
          i++;
        }
        i++;
        if (fence[2].toLowerCase() === 'mermaid') {
          html += `<figure class="diagram"><pre class="diagram-src"><code>${escape(code.join('\n'))}</code></pre></figure>`;
          continue;
        }
        if (fence[2].toLowerCase() === 'svg') {
          const drawn = drawing(code.join('\n'));
          if (drawn) {
            html += drawn;
            continue;
          }
        }
        const lang = fence[2] ? ` class="language-${escape(fence[2])}"` : '';
        html += `<pre><code${lang}>${escape(code.join('\n'))}</code></pre>`;
        continue;
      }

      const heading = line.match(HEADING);
      if (heading) {
        const level = Math.min(heading[1].length + 1, 6);
        html += `<h${level}>${inline(heading[2])}</h${level}>`;
        i++;
        continue;
      }

      if (RULE.test(line)) {
        html += '<hr>';
        i++;
        continue;
      }

      if (/^\s*>/.test(line)) {
        const quoted = [];
        while (i < lines.length && /^\s*>/.test(lines[i])) {
          quoted.push(lines[i].replace(/^\s*> ?/, ''));
          i++;
        }
        const callout = quoted[0].match(/^\s*\[!([A-Za-z-]+)\]([+-]?)\s*(.*)$/);
        if (callout) {
          const kind = callout[1].toLowerCase().replace(/[^a-z-]/g, '');
          const title = callout[3].trim() || kind.charAt(0).toUpperCase() + kind.slice(1);
          const inner = blocks(quoted.slice(1), { boxes: 0, interactive: false });
          html += callout[2]
            ? `<details class="callout callout-${kind}"${callout[2] === '+' ? ' open' : ''}><summary>${inline(title)}</summary><div class="callout-body">${inner}</div></details>`
            : `<div class="callout callout-${kind}"><div class="callout-title">${inline(title)}</div><div class="callout-body">${inner}</div></div>`;
          continue;
        }
        const inner = blocks(quoted, { boxes: 0, interactive: false });
        html += `<blockquote>${inner}</blockquote>`;
        continue;
      }

      if (line.includes('|') && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]) && lines[i + 1].includes('-')) {
        const head = cells(line);
        const aligns = aligned(lines[i + 1]);
        i += 2;
        let rows = '';
        while (i < lines.length && lines[i].includes('|') && lines[i].trim()) {
          const row = cells(lines[i]);
          rows += `<tr>${head.map((_, n) => cell('td', row[n] || '', aligns[n])).join('')}</tr>`;
          i++;
        }
        html += `<div class="table"><table><thead><tr>${head
          .map((c, n) => cell('th', c, aligns[n]))
          .join('')}</tr></thead><tbody>${rows}</tbody></table></div>`;
        continue;
      }

      if (ITEM.test(line)) {
        const items = [];
        while (i < lines.length && ITEM.test(lines[i])) {
          items.push(lines[i]);
          i++;
        }
        html += list(items, state);
        continue;
      }

      const para = [];
      while (i < lines.length && lines[i].trim() && (para.length === 0 || !startsBlock(lines[i], lines[i + 1]))) {
        para.push(lines[i].trim());
        i++;
      }
      html += `<p>${para.map(inline).join('<br>')}</p>`;
    }
    return html;
  }

  function render(markdown, options = {}) {
    pictureDir = options.dir || '';
    try {
      return blocks(String(markdown || '').replace(/\r\n?/g, '\n').split('\n'), {
        boxes: options.boxOffset || 0,
        interactive: options.interactive !== false,
        depth: options.depth || 1,
      });
    } finally {
      pictureDir = '';
    }
  }

  function plain(markdown) {
    return String(markdown || '')
      .replace(/```[\s\S]*?```/g, ' ')
      .replace(/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/gm, ' ')
      .replace(/^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/gm, ' ')
      .replace(/\s*\|\s*/g, ' ')
      .replace(new RegExp(String.raw`^\s*(?:#{1,6}|>|${MARKER})\s+`, 'gm'), '')
      .replace(/\[( |x|X)\]\s+/g, '')
      .replace(/!\[\[[^\]]*\]\]/g, ' ')
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, ' ')
      .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/<\/?u>/g, '')
      .replace(/[*_~`]/g, '')
      .replace(/\s+/g, ' ')
      .trim();
  }

  const api = { render, plain, escape, MARKER };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoMarkdown = api;
})(typeof window !== 'undefined' ? window : globalThis);
