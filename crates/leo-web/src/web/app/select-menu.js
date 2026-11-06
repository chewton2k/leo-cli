const SELECT_MENU_SCOPE = '.select-control > select';
let selectMenuIds = 0, selectMenuRefocus = null;

function selectMenuKeyOf(select) {
  return [...select.attributes].filter((a) => !['data-menu', 'tabindex', 'aria-hidden'].includes(a.name)).map((a) => `${a.name}=${a.value}`).join('|');
}

function selectMenuName(select) {
  if (select.getAttribute('aria-label')) return select.getAttribute('aria-label');
  const row = select.closest('.set-row, label');
  return row?.querySelector('.set-label')?.textContent.trim() || '';
}

function selectMenuLabel(select) {
  return select.selectedOptions[0]?.textContent || '';
}

function syncSelectMenu(select) {
  const text = select.parentElement?.querySelector(':scope > .select-menu-button .select-menu-text');
  if (text) text.textContent = selectMenuLabel(select);
}

function enhanceSelect(select) {
  if (select.dataset.menu) return;
  select.dataset.menu = String(++selectMenuIds);
  const box = select.parentElement;
  const id = `select-menu-${select.dataset.menu}`;
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'select-menu-button';
  button.setAttribute('aria-haspopup', 'listbox');
  button.setAttribute('aria-expanded', 'false');
  button.setAttribute('aria-controls', id);
  const name = selectMenuName(select);
  if (name) button.setAttribute('aria-label', name);
  button.innerHTML = `<span class="select-menu-text"></span>${ICON.chevron}`;
  const list = document.createElement('div');
  list.className = 'select-menu';
  list.id = id;
  list.setAttribute('role', 'listbox');
  if (name) list.setAttribute('aria-label', name);
  list.hidden = true;
  box.classList.add('has-menu');
  select.tabIndex = -1;
  select.setAttribute('aria-hidden', 'true');
  box.append(button, list);
  const own = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value');
  Object.defineProperty(select, 'value', {
    configurable: true,
    get() { return own.get.call(this); },
    set(next) { own.set.call(this, next); syncSelectMenu(this); },
  });
  syncSelectMenu(select);
  const lost = !document.activeElement || document.activeElement === document.body;
  if (selectMenuRefocus && selectMenuRefocus.until > Date.now() && selectMenuRefocus.key === selectMenuKeyOf(select) && lost) {
    selectMenuRefocus = null;
    button.focus({ preventScroll: true });
  }
}

function enhanceSelects(root = document.body) {
  if (matchMedia('(forced-colors: active)').matches) return;
  for (const select of root.querySelectorAll(SELECT_MENU_SCOPE)) if (!select.closest('.scrim-gone')) enhanceSelect(select);
}

function keepSelectMenu(box, next) {
  if (!box.matches('.select-control.has-menu')) return;
  const select = box.querySelector(':scope > select'), desired = next.querySelector(':scope > select');
  const button = box.querySelector(':scope > .select-menu-button'), list = box.querySelector(':scope > .select-menu');
  if (!select || !desired || !button || !list || refreshKey(select) !== refreshKey(desired)) return;
  next.classList.add('has-menu');
  for (const name of ['data-menu', 'tabindex', 'aria-hidden']) desired.setAttribute(name, select.getAttribute(name));
  const keptButton = button.cloneNode(true), keptList = list.cloneNode(true);
  keptButton.querySelector('.select-menu-text').textContent = selectMenuLabel(desired);
  next.append(keptButton, keptList);
}

function selectMenuOptions(list) {
  return [...list.querySelectorAll('.select-menu-option:not([aria-disabled="true"])')];
}

function openSelectMenu(button) {
  const box = button.parentElement, select = box.querySelector(':scope > select'), list = box.querySelector(':scope > .select-menu');
  if (!select || !list || select.disabled) return;
  closeSelectMenus(list);
  list.innerHTML = [...select.options].map((option, index) => {
    const on = option.selected;
    return `<div class="select-menu-option${on ? ' on' : ''}" role="option" id="${list.id}-${index}" data-index="${index}" aria-selected="${on}"${option.disabled ? ' aria-disabled="true"' : ''}><span>${esc(option.textContent)}</span>${on ? ICON.check : ''}</div>`;
  }).join('');
  const bounds = button.closest('.sheet')?.getBoundingClientRect() || { top: 0, bottom: innerHeight };
  const room = bounds.bottom - button.getBoundingClientRect().bottom;
  list.classList.toggle('up', room < Math.min(280, list.scrollHeight + 16) && button.getBoundingClientRect().top - bounds.top > room);
  button.setAttribute('aria-expanded', 'true');
  unfold(list, button);
  cascade(list);
  list.tabIndex = -1;
  list.focus({ preventScroll: true });
  activateSelectOption(list, list.querySelector('.select-menu-option.on') || selectMenuOptions(list)[0]);
}

function closeSelectMenus(except) {
  for (const list of document.querySelectorAll('.select-menu:not([hidden])')) {
    if (list === except) continue;
    list.parentElement.querySelector(':scope > .select-menu-button')?.setAttribute('aria-expanded', 'false');
    fold(list);
  }
}

function closeSelectMenu(list, refocus) {
  const button = list.parentElement.querySelector(':scope > .select-menu-button');
  button?.setAttribute('aria-expanded', 'false');
  fold(list);
  if (refocus) button?.focus({ preventScroll: true });
}

function activateSelectOption(list, option) {
  for (const other of list.querySelectorAll('.select-menu-option.active')) other.classList.remove('active');
  if (!option) return;
  option.classList.add('active');
  list.setAttribute('aria-activedescendant', option.id);
  option.scrollIntoView({ block: 'nearest' });
}

function pickSelectOption(list, option) {
  const select = list.parentElement.querySelector(':scope > select');
  if (!select || !option || option.getAttribute('aria-disabled') === 'true') return;
  const index = Number(option.dataset.index);
  closeSelectMenu(list, true);
  if (select.selectedIndex === index) return;
  select.selectedIndex = index;
  syncSelectMenu(select);
  selectMenuRefocus = { key: selectMenuKeyOf(select), until: Date.now() + 2000 };
  select.dispatchEvent(new Event('input', { bubbles: true }));
  select.dispatchEvent(new Event('change', { bubbles: true }));
}

let selectMenuTyped = '', selectMenuTypedAt = 0;
function selectMenuKey(list, e) {
  const options = selectMenuOptions(list);
  const at = options.indexOf(list.querySelector('.select-menu-option.active'));
  const move = (to) => { e.preventDefault(); activateSelectOption(list, options[Math.max(0, Math.min(options.length - 1, to))]); };
  if (e.key === 'ArrowDown') move(at + 1);
  else if (e.key === 'ArrowUp') move(at - 1);
  else if (e.key === 'Home') move(0);
  else if (e.key === 'End') move(options.length - 1);
  else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pickSelectOption(list, options[at]); }
  else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeSelectMenu(list, true); }
  else if (e.key === 'Tab') closeSelectMenu(list, false);
  else if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
    selectMenuTyped = (Date.now() - selectMenuTypedAt < 700 ? selectMenuTyped : '') + e.key.toLowerCase();
    selectMenuTypedAt = Date.now();
    const found = options.find((option) => option.textContent.trim().toLowerCase().startsWith(selectMenuTyped));
    if (found) { e.preventDefault(); activateSelectOption(list, found); }
  }
}

document.addEventListener('click', (e) => {
  const button = e.target.closest?.('.select-menu-button');
  if (button) {
    const list = button.parentElement.querySelector(':scope > .select-menu');
    if (button.getAttribute('aria-expanded') === 'true') closeSelectMenu(list, true); else openSelectMenu(button);
    return;
  }
  const option = e.target.closest?.('.select-menu-option');
  if (option) { pickSelectOption(option.closest('.select-menu'), option); return; }
  closeSelectMenus();
});
document.addEventListener('mousedown', (e) => {
  if (e.target.closest?.('.select-menu-button, .select-menu')) e.preventDefault();
});
document.addEventListener('mousemove', (e) => {
  const option = e.target.closest?.('.select-menu-option');
  if (option && !option.classList.contains('active')) activateSelectOption(option.closest('.select-menu'), option);
});
document.addEventListener('keydown', (e) => {
  const button = e.target.closest?.('.select-menu-button');
  if (button && ['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(e.key)) { e.preventDefault(); e.stopPropagation(); openSelectMenu(button); return; }
  const list = e.target.closest?.('.select-menu');
  if (list) selectMenuKey(list, e);
}, true);
document.addEventListener('focusout', (e) => {
  const list = e.target.closest?.('.select-menu');
  if (list && !list.parentElement.contains(e.relatedTarget)) closeSelectMenu(list, false);
});
document.addEventListener('change', (e) => {
  if (e.target.dataset?.menu) syncSelectMenu(e.target);
});
new MutationObserver(() => enhanceSelects()).observe(document.body, { childList: true, subtree: true });
