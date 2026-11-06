let popup = null;

function closePopup(focus = true) {
  if (!popup) return;
  const { box, owner } = popup;
  popup = null;
  box.inert = true;
  box.setAttribute('aria-hidden', 'true');
  box.className = 'popup-menu-gone';
  fold(box, () => box.remove());
  if (owner.hasAttribute('aria-haspopup')) owner.setAttribute('aria-expanded', 'false');
  if (focus && owner.isConnected) owner.focus({ preventScroll: true });
}

function popupRow(row, index) {
  if (row.line) return '<hr>';
  if (row.note) return `<p>${esc(row.note)}</p>`;
  return `<button type="button" role="menuitem" data-row="${index}"${row.danger ? ' class="danger"' : ''}${row.off ? ' disabled' : ''}>${row.icon || ''}<span>${esc(row.text)}</span></button>`;
}

function popupMenu(owner, { label, rows, point = null }) {
  closePopup(false);
  const box = document.createElement('div');
  box.className = 'popup-menu';
  box.setAttribute('role', 'menu');
  box.setAttribute('aria-label', label);
  document.body.appendChild(box);
  let shown = rows;
  const draw = (list) => {
    shown = list;
    box.innerHTML = list.map(popupRow).join('');
    const bounds = owner.getBoundingClientRect();
    box.style.left = `${Math.max(8, Math.min(point?.x ?? bounds.right, innerWidth - box.offsetWidth - 8))}px`;
    box.style.top = `${Math.max(8, Math.min(point?.y ?? bounds.bottom, innerHeight - box.offsetHeight - 8))}px`;
    box.querySelector('button:not(:disabled)')?.focus();
  };
  if (owner.hasAttribute('aria-haspopup')) owner.setAttribute('aria-expanded', 'true');
  draw(rows);
  popup = { box, owner };
  unfold(box, point || owner);
  box.addEventListener('click', (e) => {
    const button = e.target.closest('[data-row]');
    if (!button || button.disabled) return;
    const row = shown[button.dataset.row];
    if (row.next) { draw(row.next); return; }
    closePopup();
    Promise.resolve().then(row.run).catch(fail);
  });
  box.addEventListener('keydown', (e) => {
    const buttons = [...box.querySelectorAll('button:not(:disabled)')];
    const at = buttons.indexOf(document.activeElement);
    let next;
    if (e.key === 'ArrowDown') next = (at + 1) % buttons.length;
    else if (e.key === 'ArrowUp') next = (at + buttons.length - 1) % buttons.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = buttons.length - 1;
    else if (e.key === 'Escape' || e.key === 'Tab') { closePopup(); e.preventDefault(); e.stopPropagation(); return; }
    if (next !== undefined) { e.preventDefault(); buttons[next].focus(); }
  });
}

document.addEventListener('pointerdown', (event) => {
  if (popup && !popup.box.contains(event.target)) closePopup(false);
});
addEventListener('hashchange', () => closePopup(false));
