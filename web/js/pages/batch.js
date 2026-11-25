import {
  csvUrl, getBatch, getItem, imageUrl, saveDecision, startBatch, thumbUrl,
} from '../api.js';
import {
  bindDropzone, category, CATEGORY_LABEL, CATEGORY_TAG, Console, drawHeat, esc, formatBytes,
  formatDuration, icon, resultHtml, reviewBox, SEVERITY_TAG, toast,
} from '../ui.js';

const POLL_MS = 1000;
const CHUNK = 120;
const MIN_FOR_BANNERS = 10;
const DEFECT_BANNER_RATE = 0.3;
const SHIFT_BANNER_RATE = 0.2;
const SAVE_KEY = 'px-last-batch';
const SAVE_PARALLEL = 8;
const SEVERITY_RANK = { High: 3, Medium: 2, Low: 1 };
const RANK_DEFECT = 4000;
const RANK_REVIEW = 2000;
const RANK_ERROR = 1000;

const TILES = [
  ['all', 'Total', ''],
  ['ok', 'OK', 'ok'],
  ['review', 'Needs review', 'review'],
  ['defect', 'Defect', 'defect'],
  ['shift', 'Shift warnings', 'shift'],
  ['error', 'Errors', ''],
];

const store = {
  get() {
    try { return localStorage.getItem(SAVE_KEY); } catch { return null; }
  },
  set(id) {
    try { localStorage.setItem(SAVE_KEY, id); } catch { /* storage can be blocked; resuming is optional */ }
  },
};

// Higher means look at it sooner.
function urgency(item) {
  const cat = category(item);
  if (cat === 'defect') return RANK_DEFECT + (SEVERITY_RANK[item.severity] ?? 0) * 100 + (item.defect_probability ?? 0) * 10;
  if (cat === 'review') return RANK_REVIEW + (item.patch_score ?? item.global_score ?? 0);
  if (cat === 'error') return RANK_ERROR;
  return item.patch_score ?? item.global_score ?? 0;
}

const SORTS = {
  worst: (a, b) => urgency(b) - urgency(a),
  score: (a, b) => (b.patch_score ?? b.global_score ?? -1) - (a.patch_score ?? a.global_score ?? -1),
  name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }),
};

function matchesFilter(item, filter) {
  if (filter === 'all') return true;
  if (filter === 'shift') return item.shift_warning;
  return category(item) === filter;
}

export function mountBatch(root) {
  root.innerHTML = `
    <div class="page-head"><div><h1>Batch dashboard</h1><p>Upload a zip or many photos and see which ones need attention.</p></div>
      <a class="btn" id="export" href="#" hidden>${icon('download')} Export CSV</a></div>

    <div class="card" id="intake">
      <div class="section-label">Add images</div>
      <div class="dropzone" id="zone">${icon('upload')}<b>Drop a zip or many photos here</b><small id="picked">or click to choose files</small></div>
      <input class="hidden-input" type="file" id="input" accept=".zip,image/*" multiple>
      <div class="actions" style="margin-top:.9rem">
        <button class="btn primary" id="start" disabled>Start batch</button>
        <button class="btn" id="resume" hidden>Resume last batch</button>
      </div>
      <div class="notice danger" id="error" hidden></div>
    </div>

    <div class="card" id="progress-card" hidden style="margin-top:1.4rem">
      <div class="section-label" id="progress-title">Checking</div>
      <div class="progress"><div class="bar"><i id="bar"></i></div><div class="meta"><span id="progress-left"></span><span id="progress-right"></span></div></div>
    </div>

    <div id="alerts"></div>
    <div class="tiles" id="tiles" hidden></div>
    <div class="toolbar" id="toolbar" hidden>
      <label class="search small">${icon('search')}<input id="query" placeholder="Search file name" autocomplete="off"></label>
      <select class="field" id="sort" aria-label="Sort">
        <option value="worst">Worst first</option><option value="score">Highest score</option><option value="name">Name</option>
      </select>
      <select class="field" id="severity" aria-label="Severity">
        <option value="any">Any severity</option><option value="High">High</option><option value="Medium">Medium</option><option value="Low">Low</option>
      </select>
      <span class="spacer"></span>
      <button class="btn sm" id="select-review">Select all needing review</button>
    </div>
    <div class="cards" id="cards"></div>
    <button class="btn more" id="more" hidden>Show more</button>
    <div id="bulk"></div>`;

  const $ = (sel) => root.querySelector(sel);
  const state = {
    id: null, files: [], items: [], status: '', total: 0, done: 0, filter: 'all', query: '', sort: 'worst',
    severity: 'any', selected: new Set(), decided: new Map(), shown: CHUNK, startedAt: 0, timer: null,
    drawer: null, finishedToast: false,
  };

  // ---- intake -------------------------------------------------------------

  function setFiles(fileList) {
    state.files = [...fileList];
    const bytes = state.files.reduce((sum, f) => sum + f.size, 0);
    const zips = state.files.filter((f) => /\.zip$/i.test(f.name)).length;
    const parts = [`${state.files.length} file${state.files.length === 1 ? '' : 's'}`, formatBytes(bytes)];
    if (zips) parts.push(`${zips} zip`);
    $('#picked').textContent = parts.join(' · ');
    $('#start').disabled = false;
  }
  bindDropzone($('#zone'), $('#input'), setFiles);

  const savedId = store.get();
  if (savedId) $('#resume').hidden = false;

  function showError(message) {
    $('#error').textContent = message;
    $('#error').hidden = !message;
  }

  $('#start').addEventListener('click', async () => {
    showError('');
    $('#start').disabled = true;
    $('#progress-card').hidden = false;
    $('#progress-title').textContent = 'Uploading';
    try {
      const { batch_id: id } = await startBatch(state.files, (fraction) => {
        $('#bar').style.width = `${Math.round(fraction * 100)}%`;
        $('#progress-left').textContent = `Uploading ${Math.round(fraction * 100)}%`;
        $('#progress-right').textContent = '';
      });
      follow(id);
    } catch (err) {
      showError(err.message);
      $('#progress-card').hidden = true;
      $('#start').disabled = false;
    }
  });

  $('#resume').addEventListener('click', () => follow(savedId));

  // ---- following a batch ----------------------------------------------------

  function follow(id) {
    state.id = id;
    state.items = [];
    state.selected.clear();
    state.finishedToast = false;
    state.startedAt = Date.now();
    store.set(id);
    $('#intake').hidden = true;
    $('#progress-card').hidden = false;
    $('#export').hidden = false;
    $('#export').href = csvUrl(id);
    $('#tiles').hidden = false;
    $('#toolbar').hidden = false;
    clearInterval(state.timer);
    poll();
    state.timer = setInterval(poll, POLL_MS);
  }

  async function poll() {
    try {
      const data = await getBatch(state.id);
      Object.assign(state, { items: data.items, status: data.status, total: data.total, done: data.done });
      paintProgress(data.error);
      paintAll();
      if (data.status === 'done' || data.status === 'failed') {
        clearInterval(state.timer);
        if (!state.finishedToast && data.status === 'done') toast(`Batch finished: ${data.items.length} images checked`);
        state.finishedToast = true;
      }
    } catch (err) {
      clearInterval(state.timer);
      $('#intake').hidden = false;
      $('#tiles').hidden = true;
      $('#toolbar').hidden = true;
      showError(`Could not load the batch: ${err.message}`);
    }
  }

  function paintProgress(error) {
    const { status, total, done } = state;
    const fraction = total ? done / total : 0;
    $('#bar').style.width = `${Math.round(fraction * 100)}%`;
    if (status === 'failed') {
      $('#progress-title').textContent = 'Batch failed';
      $('#progress-left').textContent = error ?? 'Something went wrong';
      $('#progress-right').textContent = '';
      return;
    }
    if (status === 'preparing') {
      $('#progress-title').textContent = 'Unpacking';
      $('#progress-left').textContent = 'Reading the upload…';
      return;
    }
    const seconds = (Date.now() - state.startedAt) / 1000;
    const rate = done && seconds ? done / seconds : 0;
    if (status === 'done') {
      $('#progress-title').textContent = 'Finished';
      $('#progress-left').textContent = `${done} of ${total} checked`;
      $('#progress-right').textContent = `${formatDuration(seconds)}`;
    } else {
      $('#progress-title').textContent = 'Checking';
      $('#progress-left').textContent = `${done} of ${total} checked`;
      $('#progress-right').textContent = rate
        ? `${rate.toFixed(1)} images/s · about ${formatDuration((total - done) / rate)} left`
        : '';
    }
  }

  // ---- list, tiles, alerts ----------------------------------------------------

  function visibleItems() {
    const query = state.query.toLowerCase();
    return state.items
      .filter((item) => matchesFilter(item, state.filter))
      .filter((item) => state.severity === 'any' || item.severity === state.severity)
      .filter((item) => !query || item.name.toLowerCase().includes(query))
      .sort(SORTS[state.sort]);
  }

  function counts() {
    const c = { all: state.total || state.items.length, ok: 0, review: 0, defect: 0, shift: 0, error: 0 };
    for (const item of state.items) {
      c[category(item)] += 1;
      if (item.shift_warning) c.shift += 1;
    }
    return c;
  }

  function paintTiles(c) {
    $('#tiles').innerHTML = TILES.map(
      ([key, label, tone]) => `<button class="tile ${tone}" data-filter="${key}" aria-pressed="${state.filter === key}">
        <div class="label"><i></i>${label}</div><div class="value">${c[key]}</div></button>`,
    ).join('');
  }

  function paintAlerts(c) {
    const judged = c.ok + c.review + c.defect;
    let html = '';
    if (judged >= MIN_FOR_BANNERS && c.defect / judged >= DEFECT_BANNER_RATE) {
      html += `<div class="notice danger">High defect rate: ${Math.round((100 * c.defect) / judged)}% of this batch is defective. Check the line before continuing.</div>`;
    }
    if (judged >= MIN_FOR_BANNERS && c.shift / judged >= SHIFT_BANNER_RATE) {
      html += `<div class="notice">Many photos look different from the training images (${c.shift} of ${judged}). Lighting or camera may have changed for this batch.</div>`;
    }
    $('#alerts').innerHTML = html;
  }

  function cardHtml(item) {
    const cat = category(item);
    const decided = state.decided.get(item.name);
    const score = item.patch_score ?? item.global_score;
    const right = decided
      ? `<span class="tag ok">Reviewed: ${esc(decided)}</span>`
      : item.severity
        ? `<span class="tag ${SEVERITY_TAG[item.severity]}">${esc(item.severity)}</span>`
        : '';
    const picture = cat === 'error'
      ? `<div class="err-text">${esc(item.error ?? 'Could not check this image')}</div>`
      : `<img loading="lazy" alt="" src="${thumbUrl(state.id, item.name)}">`;
    return `<div class="thumb-card ${cat} ${state.selected.has(item.name) ? 'selected' : ''}" role="button" tabindex="0" data-name="${esc(item.name)}">
      <div class="img">${picture}
        <span class="badge tag ${CATEGORY_TAG[cat]}">${CATEGORY_LABEL[cat]}</span>
        ${item.shift_warning ? '<span class="shift-dot" title="Lighting or camera may have changed"></span>' : ''}
        <span class="pick" data-pick title="Select">${icon('check')}</span>
      </div>
      <div class="name" title="${esc(item.name)}">${esc(item.name)}</div>
      <div class="meta"><span>${score == null ? '' : `score ${score.toFixed(2)}`}</span>${right}</div>
    </div>`;
  }

  function paintCards() {
    const list = visibleItems();
    if (!list.length) {
      $('#cards').innerHTML = `<div class="empty" style="grid-column:1/-1">${state.items.length ? 'Nothing matches these filters.' : 'Waiting for the first results…'}</div>`;
      $('#more').hidden = true;
      return;
    }
    $('#cards').innerHTML = list.slice(0, state.shown).map(cardHtml).join('');
    $('#more').hidden = list.length <= state.shown;
    $('#more').textContent = `Show more (${list.length - state.shown} left)`;
  }

  function paintBulk() {
    const n = state.selected.size;
    $('#bulk').innerHTML = n
      ? `<div class="bulkbar"><b>${n} selected</b>
          <button class="btn sm" data-bulk="ok">OK</button><button class="btn sm" data-bulk="reject">Reject</button>
          <button class="btn sm" data-bulk="fix">Fix</button><button class="btn sm" data-bulk="clear">${icon('x')} Clear</button></div>`
      : '';
  }

  // Results arrive while the batch runs; repaint at most once per frame.
  let queued = false;
  function paintAll() {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      const c = counts();
      paintTiles(c);
      paintAlerts(c);
      paintCards();
      paintBulk();
    });
  }

  $('#tiles').addEventListener('click', (e) => {
    const tile = e.target.closest('[data-filter]');
    if (!tile) return;
    state.filter = state.filter === tile.dataset.filter ? 'all' : tile.dataset.filter;
    state.shown = CHUNK;
    paintAll();
  });
  $('#query').addEventListener('input', (e) => { state.query = e.target.value; state.shown = CHUNK; paintAll(); });
  $('#sort').addEventListener('change', (e) => { state.sort = e.target.value; paintAll(); });
  $('#severity').addEventListener('change', (e) => { state.severity = e.target.value; state.shown = CHUNK; paintAll(); });
  $('#more').addEventListener('click', () => { state.shown += CHUNK; paintAll(); });
  $('#select-review').addEventListener('click', () => {
    state.items.filter((i) => category(i) === 'review').forEach((i) => state.selected.add(i.name));
    paintAll();
  });

  function toggleSelect(name) {
    if (state.selected.has(name)) state.selected.delete(name);
    else state.selected.add(name);
    paintAll();
  }

  $('#cards').addEventListener('click', (e) => {
    const card = e.target.closest('.thumb-card');
    if (!card) return;
    if (e.target.closest('[data-pick]')) toggleSelect(card.dataset.name);
    else openDrawer(card.dataset.name);
  });
  $('#cards').addEventListener('keydown', (e) => {
    const card = e.target.closest('.thumb-card');
    if (card && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault();
      openDrawer(card.dataset.name);
    }
  });

  // ---- decisions ---------------------------------------------------------------

  async function decide(names, decision) {
    const verdictOf = (name) => state.items.find((i) => i.name === name)?.verdict ?? '';
    let failed = 0;
    for (let i = 0; i < names.length; i += SAVE_PARALLEL) {
      const results = await Promise.allSettled(
        names.slice(i, i + SAVE_PARALLEL).map((name) =>
          saveDecision(`${state.id}/${name}`, verdictOf(name), decision).then(() => state.decided.set(name, decision))),
      );
      failed += results.filter((r) => r.status === 'rejected').length;
    }
    if (failed) toast(`${failed} decision${failed === 1 ? '' : 's'} could not be saved`, 'error');
    else toast(`Saved ${decision.toUpperCase()} for ${names.length} image${names.length === 1 ? '' : 's'}`);
    paintAll();
    return failed === 0;
  }

  $('#bulk').addEventListener('click', async (e) => {
    const button = e.target.closest('[data-bulk]');
    if (!button) return;
    if (button.dataset.bulk === 'clear') {
      state.selected.clear();
      paintAll();
      return;
    }
    const names = [...state.selected];
    if (await decide(names, button.dataset.bulk)) state.selected.clear();
    paintAll();
  });

  // ---- detail drawer -------------------------------------------------------------

  const itemCache = new Map();

  function closeDrawer() {
    if (!state.drawer) return;
    document.removeEventListener('keydown', state.drawer.onKey);
    window.removeEventListener('resize', state.drawer.onResize);
    state.drawer.el.remove();
    state.drawer = null;
  }

  function openDrawer(name) {
    closeDrawer();
    const el = document.createElement('div');
    el.className = 'backdrop';
    document.body.append(el);
    state.drawer = { el, name, heatOn: true, onKey: null, onResize: null };
    state.drawer.onKey = (e) => {
      if (e.key === 'Escape') closeDrawer();
      if (e.key === 'ArrowRight') step(1);
      if (e.key === 'ArrowLeft') step(-1);
    };
    document.addEventListener('keydown', state.drawer.onKey);
    el.addEventListener('click', (e) => e.target === el && closeDrawer());
    fillDrawer();
  }

  function step(direction) {
    const list = visibleItems();
    const at = list.findIndex((i) => i.name === state.drawer.name);
    const next = list[at + direction];
    if (next) {
      state.drawer.name = next.name;
      fillDrawer();
    }
  }

  async function fillDrawer() {
    const drawer = state.drawer;
    if (!drawer) return;
    const { name } = drawer;
    const list = visibleItems();
    const at = list.findIndex((i) => i.name === name);
    drawer.el.innerHTML = `<div class="drawer" role="dialog" aria-label="${esc(name)}">
      <div class="drawer-head">
        <button class="btn round" data-prev ${at <= 0 ? 'disabled' : ''} aria-label="Previous">${icon('left')}</button>
        <h2>${esc(name)} <span class="muted small">${at >= 0 ? `${at + 1} of ${list.length}` : ''}</span></h2>
        <button class="btn round" data-next ${at < 0 || at >= list.length - 1 ? 'disabled' : ''} aria-label="Next"><span style="display:inline-flex;transform:scaleX(-1)">${icon('left')}</span></button>
        <button class="btn round" data-close aria-label="Close">${icon('x')}</button>
      </div>
      <div class="drawer-body"><div id="d-left"><div class="empty">Loading…</div></div><div id="d-right"></div></div></div>`;
    drawer.el.querySelector('[data-close]').onclick = closeDrawer;
    drawer.el.querySelector('[data-prev]').onclick = () => step(-1);
    drawer.el.querySelector('[data-next]').onclick = () => step(1);

    let item = itemCache.get(`${state.id}/${name}`);
    try {
      if (!item) {
        item = await getItem(state.id, name);
        itemCache.set(`${state.id}/${name}`, item);
      }
    } catch (err) {
      drawer.el.querySelector('#d-left').innerHTML = `<div class="notice danger">${esc(err.message)}</div>`;
      return;
    }
    if (state.drawer !== drawer || drawer.name !== name) return;
    paintDrawerItem(drawer, item);
  }

  function paintDrawerItem(drawer, item) {
    const left = drawer.el.querySelector('#d-left');
    const right = drawer.el.querySelector('#d-right');
    if (!item.response) {
      left.innerHTML = `<div class="notice danger">${esc(item.error ?? 'No result for this image')}</div>`;
      return;
    }
    const r = item.response;
    left.innerHTML = `<div class="preview-wrap"><img id="d-img" alt="${esc(item.name)}" src="${imageUrl(state.id, item.name)}"><canvas id="d-heat"></canvas></div>
      ${r.finding ? `<label class="check-row"><input type="checkbox" id="d-heat-on" checked> show where (heatmap)</label>` : ''}`;
    right.innerHTML = `${resultHtml(r)}${r.needs_review ? reviewBox() : ''}
      <div class="section-label" style="margin-top:1rem">Steps</div><div class="console short" id="d-console"></div>`;

    const paint = () => drawHeat(left.querySelector('#d-heat'), left.querySelector('#d-img'), r.finding,
      r.patch ? r.patch.threshold : 0, drawer.heatOn);
    left.querySelector('#d-img').addEventListener('load', paint);
    drawer.onResize = paint;
    window.addEventListener('resize', paint);
    const toggle = left.querySelector('#d-heat-on');
    if (toggle) toggle.addEventListener('change', () => { drawer.heatOn = toggle.checked; paint(); });

    new Console(right.querySelector('#d-console')).replay(r, false);
    right.addEventListener('click', async (e) => {
      const button = e.target.closest('[data-decision]');
      if (button) await decide([item.name], button.dataset.decision);
    });
  }

  return () => {
    clearInterval(state.timer);
    closeDrawer();
  };
}
