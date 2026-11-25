// Small helpers shared by every page.

const ICONS = {
  home: '<path d="M3 11l9-8 9 8"/><path d="M5 10v10h14V10"/>',
  scan: '<path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3"/><circle cx="12" cy="12" r="3"/>',
  grid: '<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>',
  review: '<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.7 2.7L16 9.5"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5"/><circle cx="12" cy="7.8" r=".6"/>',
  chevron: '<path d="M9 6l6 6-6 6"/>',
  left: '<path d="M15 6l-6 6 6 6"/>',
  upload: '<path d="M12 16V4"/><path d="M7 9l5-5 5 5"/><path d="M5 20h14"/>',
  download: '<path d="M12 4v12"/><path d="M7 11l5 5 5-5"/><path d="M5 20h14"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>',
  search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4 4"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
};

export const TAB_LABELS = {
  '/': ['home', 'Overview'],
  '/check': ['scan', 'Check image'],
  '/batch': ['grid', 'Batch'],
  '/review': ['review', 'Review'],
  '/how': ['info', 'How it works'],
};

export function icon(name) {
  return `<svg class="icon" viewBox="0 0 24 24" aria-hidden="true">${ICONS[name]}</svg>`;
}

const HTML_ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export const esc = (text) => String(text).replace(/[&<>"']/g, (c) => HTML_ESCAPES[c]);

const TOAST_MS = 4500;
const MAX_TOASTS = 3;

export function toast(message, kind = 'ok') {
  const box = document.getElementById('toasts');
  const node = document.createElement('div');
  node.className = `toast ${kind}`;
  node.innerHTML = `<i></i><span>${esc(message)}</span>`;
  node.addEventListener('click', () => node.remove());
  box.append(node);
  while (box.children.length > MAX_TOASTS) box.firstChild.remove();
  // Success hides itself; errors stay until clicked.
  if (kind !== 'error') setTimeout(() => node.remove(), TOAST_MS);
}

const BYTE_UNITS = ['B', 'KB', 'MB', 'GB'];
export function formatBytes(bytes) {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${BYTE_UNITS[unit]}`;
}

export function formatDuration(seconds) {
  if (!Number.isFinite(seconds)) return '…';
  const s = Math.round(seconds);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
}

export const formatNumber = (n) => Number(n).toLocaleString();

export function formatTime(unixSeconds) {
  return new Date(unixSeconds * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
}

// ---- verdicts -----------------------------------------------------------

export function category(item) {
  if (item.status === 'error') return 'error';
  return { Good: 'ok', Defect: 'defect', Uncertain: 'review', Unknown: 'unknown' }[item.verdict] ?? 'error';
}

export const CATEGORY_LABEL = { ok: 'OK', review: 'Needs review', defect: 'Defect', unknown: 'Unknown product', error: 'Error' };
export const CATEGORY_TAG = { ok: 'ok', review: 'warn', defect: 'danger', unknown: 'grey', error: 'grey' };
export const SEVERITY_TAG = { Low: 'warn', Medium: 'danger', High: 'danger' };

export const BANNERS = {
  Good: ['OK, passes', 'Step 1 found nothing odd.'],
  Defect: ['Rejected', 'Both checks flag damage.'],
  Uncertain: ['Needs a person', 'The two checks disagree or are unsure.'],
  Unknown: ['Unknown product', 'This does not look like any product the system has learned, so it was not judged.'],
};

// ---- console ------------------------------------------------------------

export function lineClass(message) {
  if (/WARNING|looks odd|unknown product/.test(message)) return 'warn';
  if (/REJECTED/.test(message)) return 'bad';
  if (/PASS|OK|Step 1 passed/.test(message)) return 'ok';
  return '';
}

export const VERDICT_CLASS = { Good: 'ok', Defect: 'bad', Uncertain: 'warn', Unknown: 'warn' };

const REPLAY_MS = 200;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export class Console {
  constructor(el) {
    this.el = el;
  }

  clear() {
    this.el.innerHTML = '';
  }

  line(text, ms = null, cls = '') {
    const row = document.createElement('div');
    const stamp = document.createElement('span');
    stamp.className = 't';
    stamp.textContent = ms === null ? '' : `[${String(ms).padStart(4)} ms] `;
    const body = document.createElement('span');
    if (cls) body.className = cls;
    body.textContent = text;
    row.append(stamp, body);
    this.el.append(row);
    this.el.scrollTop = this.el.scrollHeight;
  }

  // The server sends the real steps and timings at once; printing them one by one shows the flow.
  async replay(result, animate = true) {
    for (const entry of result.log) {
      this.line(entry.message, entry.ms, lineClass(entry.message));
      if (animate) await sleep(REPLAY_MS);
    }
    this.line(`Final verdict: ${result.verdict.toUpperCase()}`, null, VERDICT_CLASS[result.verdict]);
  }
}

// ---- result card pieces -------------------------------------------------

export function fact(label, value) {
  return `<div class="fact"><span>${label}</span><span>${value}</span></div>`;
}

export function describeModel(model) {
  if (!model) return 'skipped (Step 1 passed)';
  return `${esc(model.vote)} · ${model.score.toFixed(2)} / ${model.threshold.toFixed(2)}`;
}

export function resultHtml(result) {
  const [title, subtitle] = BANNERS[result.verdict];
  if (result.verdict === 'Unknown') {
    return `<div class="banner Unknown">${title}<small>${subtitle}</small></div>
      <div class="facts">${fact('Closest product', esc(result.product))}</div>`;
  }
  const f = result.finding;
  let facts =
    (result.product ? fact('Product', esc(result.product)) : '') +
    fact('Step 1 · global check', describeModel(result.global)) +
    fact('Step 2 · patch check', describeModel(result.patch));
  if (f) {
    facts +=
      fact('Where', `${f.area_pct.toFixed(1)}% of the image`) +
      fact('How bad', esc(f.severity)) +
      fact('How sure (defect probability)', `${Math.round(f.defect_probability * 100)}%`);
  }
  const shift = result.shift.warning
    ? `<div class="notice">Warning: this photo looks different from the training images (distance ${result.shift.distance.toFixed(2)} > ${result.shift.limit.toFixed(2)}). Lighting or camera may have changed.</div>`
    : '';
  return `<div class="banner ${result.verdict}">${title}<small>${subtitle}</small></div>${shift}<div class="facts">${facts}</div>`;
}

export function reviewBox() {
  return `<div class="review-box">
    <p>Step 3: the two checks disagree. A person decides:</p>
    <div class="actions">
      <button class="btn" data-decision="ok">OK</button>
      <button class="btn" data-decision="reject">Reject</button>
      <button class="btn" data-decision="fix">Fix</button>
    </div>
  </div>`;
}

// ---- dropzone -----------------------------------------------------------

// Makes `zone` clickable and droppable; `onFiles` gets the chosen FileList.
export function bindDropzone(zone, input, onFiles) {
  zone.addEventListener('click', () => input.click());
  input.addEventListener('change', () => input.files.length && onFiles(input.files));
  ['dragenter', 'dragover'].forEach((type) =>
    zone.addEventListener(type, (e) => { e.preventDefault(); zone.classList.add('over'); }));
  ['dragleave', 'drop'].forEach((type) =>
    zone.addEventListener(type, (e) => { e.preventDefault(); zone.classList.remove('over'); }));
  zone.addEventListener('drop', (e) => e.dataTransfer.files.length && onFiles(e.dataTransfer.files));
}

// Stops the browser from opening a file dropped outside a zone.
export function blockStrayDrops() {
  ['dragover', 'drop'].forEach((type) => window.addEventListener(type, (e) => e.preventDefault()));
}

// ---- confirm sheet ------------------------------------------------------

/** Asks before something that cannot be undone. Resolves to true only on the action button. */
export function confirmSheet(message, actionLabel) {
  return new Promise((resolve) => {
    const el = document.createElement('div');
    el.className = 'backdrop';
    el.innerHTML = `<div class="sheet" role="alertdialog"><p>${esc(message)}</p>
      <div class="actions"><button class="btn" data-answer="no">Cancel</button>
      <button class="btn danger solid" data-answer="yes">${esc(actionLabel)}</button></div></div>`;
    const finish = (answer) => {
      document.removeEventListener('keydown', onKey);
      el.remove();
      resolve(answer);
    };
    const onKey = (e) => e.key === 'Escape' && finish(false);
    document.addEventListener('keydown', onKey);
    el.addEventListener('click', (e) => {
      if (e.target === el) finish(false);
      const button = e.target.closest('[data-answer]');
      if (button) finish(button.dataset.answer === 'yes');
    });
    document.body.append(el);
  });
}
