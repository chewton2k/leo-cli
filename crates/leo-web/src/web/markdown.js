(function (root) {
  'use strict';

  const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const escape = (s) => s.replace(/[&<>"']/g, (c) => ESCAPES[c]);

  const safeUrl = (url) => (/^(https?:|mailto:)/i.test(url.trim()) ? url.trim() : null);

  const link = (url, label) =>
    `<a href="${escape(url)}" target="_blank" rel="noopener noreferrer">${label}</a>`;

  function emphasis(s) {
    return s
      .replace(/\*\*(?=\S)([\s\S]*?\S)\*\*/g, '<strong>$1</strong>')
      .replace(/__(?=\S)([\s\S]*?\S)__(?![A-Za-z0-9])/g, '<strong>$1</strong>')
      .replace(/~~(?=\S)([\s\S]*?\S)~~/g, '<del>$1</del>')
      .replace(/(^|[^*\w])\*(?=\S)([^*]*?\S)\*(?!\*)/g, '$1<em>$2</em>')
      .replace(/(^|[^_\w])_(?=\S)([^_]*?\S)_(?![_\w])/g, '$1<em>$2</em>');
  }

  let pictureDir = '';

  function picture(target, alt) {
    const src = String(target || '').trim().replace(/^<(.*)>$/, '$1');
    if (!src || /^[a-z][a-z0-9+.-]*:/i.test(src) || src.startsWith('//')) return null;
    const url = `/api/image?path=${encodeURIComponent(src)}&from=${encodeURIComponent(pictureDir)}`;
    return `<img class="note-img" src="${escape(url)}" alt="${escape(alt || '')}" loading="lazy">`;
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
    s = s.replace(/!\[\[([^\]|]+)(?:\|[^\]]*)?\]\]/g, (whole, name) => {
      const img = picture(name.trim(), name.trim());
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
  const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
  const TABLE_RULE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
  const BOX = /^(?:[-*+]|\d+[.)]) {1,4}\[( |x|X)\](?: |$)/;

  const isBox = (line) => BOX.test(line.trimStart());

  const cells = (row) =>
    row
      .trim()
      .replace(/^\|/, '')
      .replace(/\|$/, '')
      .split('|')
      .map((c) => c.trim());

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
      return { indent: m[1].replace(/\t/g, '    ').length, ordered: /\d/.test(m[2]), raw: line, text: m[3] };
    });

    function build(from, indent) {
      const ordered = items[from].ordered;
      const first = ordered ? parseInt(items[from].raw.trim(), 10) : 1;
      let html = ordered ? (first > 1 ? `<ol start="${first}">` : '<ol>') : '<ul>';
      let i = from;
      while (i < items.length && items[i].indent >= indent) {
        const item = items[i];
        if (item.indent > indent) {
          const [inner, next] = build(i, item.indent);
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
          html += `<li>${inline(item.text)}</li>`;
        }
        i++;
      }
      return [html + (ordered ? '</ol>' : '</ul>'), i];
    }

    let html = '';
    let i = 0;
    while (i < items.length) {
      const [block, next] = build(i, items[i].indent);
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
        i += 2;
        let rows = '';
        while (i < lines.length && lines[i].includes('|') && lines[i].trim()) {
          rows += `<tr>${cells(lines[i]).map((c) => `<td>${inline(c)}</td>`).join('')}</tr>`;
          i++;
        }
        html += `<div class="table"><table><thead><tr>${head
          .map((c) => `<th>${inline(c)}</th>`)
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
        interactive: true,
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
      .replace(/^\s*(#{1,6}|>|[-*+]|\d+[.)])\s+/gm, '')
      .replace(/\[( |x|X)\]\s+/g, '')
      .replace(/!\[\[[^\]]*\]\]/g, ' ')
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, ' ')
      .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/[*_~`]/g, '')
      .replace(/\s+/g, ' ')
      .trim();
  }

  const api = { render, plain, escape };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.leoMarkdown = api;
})(typeof window !== 'undefined' ? window : globalThis);
