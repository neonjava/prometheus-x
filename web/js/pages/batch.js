import {
  csvUrl, getBatch, getBatches, getItem, imageUrl, saveDecision, startBatch, thumbUrl,
} from '../api.js';
import {
  bindDropzone, category, CATEGORY_LABEL, CATEGORY_TAG, Console, drawHeat, esc, formatBytes,
  formatDuration, formatTime, icon, resultHtml, reviewBox, SEVERITY_TAG, toast,
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

// Module-level persistent state so navigating to Review and back loses nothing.
const activeBatch = {
  id: null, files: [], items: [], status: '', total: 0, done: 0, filter: 'all', query: '', sort: 'worst',
  severity: 'any', selected: new Set(), decided: new Map(), shown: CHUNK, startedAt: 0, timer: null,
  finishedToast: false,
};

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
    <div class="page-head">
      <div>
        <h1>Batch dashboard</h1>
        <p>Upload a zip or many photos and see which ones need attention.</p>
      </div>
      <div style="display:flex;gap:.6rem;align-items:center">
        <button class="btn" id="new-batch" hidden>${icon('upload')} New batch</button>
        <a class="btn" id="export" href="#" hidden>${icon('download')} Export CSV</a>
      </div>
    </div>

    <div class="card" id="intake">
      <div class="section-label">Add images</div>
      <div class="dropzone" id="zone">${icon('upload')}<b>Drop a zip or many photos here</b><small id="picked">or click to choose files</small></div>
      <input class="hidden-input" type="file" id="input" accept=".zip,image/*" multiple>
      <div class="actions" style="margin-top:.9rem">
        <button class="btn primary" id="start" disabled>Start batch</button>
      </div>
      <div class="notice danger" id="error" hidden></div>
    </div>

    <div class="card" id="recent-card" style="margin-top:1.4rem">
      <div class="section-label">Recent batches</div>
      <div id="recent-list"><div class="empty">Loading recent batches…</div></div>
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
  let drawer = null;

  function setFiles(fileList) {
    activeBatch.files = [...fileList];
    const bytes = activeBatch.files.reduce((sum, f) => sum + f.size, 0);
    const zips = activeBatch.files.filter((f) => /\.zip$/i.test(f.name)).length;
    const parts = [`${activeBatch.files.length} file${activeBatch.files.length === 1 ? '' : 's'}`, formatBytes(bytes)];
    if (zips) parts.push(`${zips} zip`);
    $('#picked').textContent = parts.join(' · ');
    $('#start').disabled = false;
  }
  bindDropzone($('#zone'), $('#input'), setFiles);

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
      const { batch_id: id } = await startBatch(activeBatch.files, (fraction) => {
        $('#bar').style.width = `${Math.round(fraction * 100)}%`;
        $('#progress-left').textContent = `Uploading ${Math.round(fraction * 100)}%`;
        $('#progress-right').textContent = '';
      });
      follow(id, true);
    } catch (err) {
      showError(err.message);
      $('#progress-card').hidden = true;
      $('#start').disabled = false;
    }
  });

  $('#new-batch').addEventListener('click', () => {
    activeBatch.id = null;
    activeBatch.items = [];
    activeBatch.total = 0;
    activeBatch.done = 0;
    activeBatch.selected.clear();
    clearInterval(activeBatch.timer);
    activeBatch.timer = null;
    store.set('');
    $('#intake').hidden = false;
    $('#new-batch').hidden = true;
    $('#progress-card').hidden = true;
    $('#tiles').hidden = true;
    $('#toolbar').hidden = true;
    $('#cards').innerHTML = '';
    $('#alerts').innerHTML = '';
    $('#export').hidden = true;
    $('#bulk').innerHTML = '';
    loadRecent();
  });

  async function loadRecent() {
    const listEl = $('#recent-list');
    if (!listEl) return;
    try {
      const batches = await getBatches();
      if (!batches.length) {
        listEl.innerHTML = '<div class="empty">No batches yet.</div>';
        return;
      }
      listEl.innerHTML = `<div class="table"><table>
        <thead><tr><th>Time</th><th>Batch</th><th>Status</th><th>Counts</th><th style="text-align:right">Action</th></tr></thead>
        <tbody>${batches.slice(0, 10).map((b) => {
          const isCurrent = b.id === activeBatch.id;
          const statusTag = b.status === 'done' ? 'ok' : b.status === 'failed' ? 'danger' : 'warn';
          return `<tr>
            <td class="num">${esc(formatTime(b.time))}</td>
            <td><b>${esc(b.id)}</b></td>
            <td><span class="tag ${statusTag}">${esc(b.status)}</span></td>
            <td>${b.counts.total} items · ${b.counts.ok} OK · ${b.counts.review} review · ${b.counts.defect} defect</td>
            <td style="text-align:right">
              <button class="btn sm ${isCurrent ? 'primary' : ''}" data-open-b="${esc(b.id)}">
                ${isCurrent ? 'Viewing' : 'Open'}
              </button>
            </td>
          </tr>`;
        }).join('')}</tbody></table></div>`;
    } catch (err) {
      listEl.innerHTML = `<div class="empty">Could not load recent batches: ${esc(err.message)}</div>`;
    }
  }

  $('#recent-card').addEventListener('click', (e) => {
    const btn = e.target.closest('[data-open-b]');
    if (btn) follow(btn.dataset.openB, false);
  });

  function follow(id, isNew = false) {
    if (isNew || activeBatch.id !== id) {
      activeBatch.id = id;
      activeBatch.items = [];
      activeBatch.selected.clear();
      activeBatch.finishedToast = false;
      activeBatch.startedAt = Date.now();
    }
    store.set(id);
    $('#intake').hidden = true;
    $('#new-batch').hidden = false;
    $('#progress-card').hidden = false;
    $('#export').hidden = false;
    $('#export').href = csvUrl(id);
    $('#tiles').hidden = false;
    $('#toolbar').hidden = false;
    $('#sort').value = activeBatch.sort;
    $('#severity').value = activeBatch.severity;
    $('#query').value = activeBatch.query;

    clearInterval(activeBatch.timer);
    poll();
    activeBatch.timer = setInterval(poll, POLL_MS);
    loadRecent();
  }

  async function poll() {
    if (!activeBatch.id) return;
    try {
      const data = await getBatch(activeBatch.id);
      Object.assign(activeBatch, { items: data.items, status: data.status, total: data.total, done: data.done });
      paintProgress(data.error);
      paintAll();
      if (data.status === 'done' || data.status === 'failed') {
        clearInterval(activeBatch.timer);
        activeBatch.timer = null;
        if (!activeBatch.finishedToast && data.status === 'done') toast(`Batch finished: ${data.items.length} images checked`);
        activeBatch.finishedToast = true;
        loadRecent();
      }
    } catch (err) {
      clearInterval(activeBatch.timer);
      activeBatch.timer = null;
      showError(`Could not load batch: ${err.message}`);
    }
  }

  function paintProgress(error) {
    const { status, total, done } = activeBatch;
    const fraction = total ? done / total : 0;
    const bar = $('#bar');
    if (bar) bar.style.width = `${Math.round(fraction * 100)}%`;
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
    const seconds = (Date.now() - activeBatch.startedAt) / 1000;
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

  function visibleItems() {
    const query = activeBatch.query.toLowerCase();
    return activeBatch.items
      .filter((i) => matchesFilter(i, activeBatch.filter))
      .filter((i) => !query || i.name.toLowerCase().includes(query))
      .filter((i) => activeBatch.severity === 'any' || i.severity === activeBatch.severity)
      .sort(SORTS[activeBatch.sort]);
  }

  function counts() {
    const c = { all: activeBatch.total || activeBatch.items.length, ok: 0, review: 0, defect: 0, shift: 0, error: 0 };
    for (const item of activeBatch.items) {
      c[category(item)] += 1;
      if (item.shift_warning) c.shift += 1;
    }
    return c;
  }

  function paintTiles(c) {
    $('#tiles').innerHTML = TILES.map(
      ([key, label, tone]) => `<button class="tile ${tone}" data-filter="${key}" aria-pressed="${activeBatch.filter === key}">
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
    const decided = activeBatch.decided.get(decisionKey(item.name));
    const score = item.patch_score ?? item.global_score;
    const right = decided
      ? `<span class="tag ok">Reviewed: ${esc(decided)}</span>`
      : item.severity
        ? `<span class="tag ${SEVERITY_TAG[item.severity]}">${esc(item.severity)}</span>`
        : '';
    const picture = cat === 'error'
      ? `<div class="err-text">${esc(item.error ?? 'Could not check this image')}</div>`
      : `<img loading="lazy" alt="" src="${thumbUrl(activeBatch.id, item.name)}">`;
    return `<div class="thumb-card ${cat} ${activeBatch.selected.has(item.name) ? 'selected' : ''}" role="button" tabindex="0" data-name="${esc(item.name)}">
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
      $('#cards').innerHTML = `<div class="empty" style="grid-column:1/-1">${activeBatch.items.length ? 'Nothing matches these filters.' : 'Waiting for the first results…'}</div>`;
      $('#more').hidden = true;
      return;
    }
    $('#cards').innerHTML = list.slice(0, activeBatch.shown).map(cardHtml).join('');
    $('#more').hidden = list.length <= activeBatch.shown;
    $('#more').textContent = `Show more (${list.length - activeBatch.shown} left)`;
  }

  function paintBulk() {
    const n = activeBatch.selected.size;
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
    activeBatch.filter = activeBatch.filter === tile.dataset.filter ? 'all' : tile.dataset.filter;
    activeBatch.shown = CHUNK;
    paintAll();
  });
  $('#query').addEventListener('input', (e) => {
    activeBatch.query = e.target.value;
    activeBatch.shown = CHUNK;
    paintCards();
  });

  $('#sort').addEventListener('change', (e) => {
    activeBatch.sort = e.target.value;
    paintCards();
  });

  $('#severity').addEventListener('change', (e) => {
    activeBatch.severity = e.target.value;
    activeBatch.shown = CHUNK;
    paintCards();
  });

  $('#more').addEventListener('click', () => {
    activeBatch.shown += CHUNK;
    paintCards();
  });

  $('#select-review').addEventListener('click', () => {
    for (const item of activeBatch.items) {
      if (category(item) === 'review') activeBatch.selected.add(item.name);
    }
    paintAll();
  });

  function toggleSelect(name) {
    if (activeBatch.selected.has(name)) activeBatch.selected.delete(name);
    else activeBatch.selected.add(name);
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

  // Two batches can hold files with the same name, so decisions are keyed by batch too.
  function decisionKey(name) {
    return `${activeBatch.id}/${name}`;
  }

  async function decide(names, decision) {
    let failed = 0;
    const chunks = [];
    for (let i = 0; i < names.length; i += SAVE_PARALLEL) chunks.push(names.slice(i, i + SAVE_PARALLEL));
    for (const chunk of chunks) {
      await Promise.all(chunk.map(async (name) => {
        const item = activeBatch.items.find((i) => i.name === name);
        try {
          await saveDecision(decisionKey(name), item?.verdict ?? 'Uncertain', decision);
          activeBatch.decided.set(decisionKey(name), decision);
        } catch {
          failed += 1;
        }
      }));
    }
    if (failed) toast(`Saved ${decision.toUpperCase()}, but ${failed} failed`, 'error');
    else toast(`Marked ${decision.toUpperCase()} for ${names.length} image${names.length === 1 ? '' : 's'}`);
    paintAll();
    return failed === 0;
  }

  $('#bulk').addEventListener('click', async (e) => {
    const button = e.target.closest('[data-bulk]');
    if (!button) return;
    if (button.dataset.bulk === 'clear') {
      activeBatch.selected.clear();
      paintAll();
      return;
    }
    const names = [...activeBatch.selected];
    if (await decide(names, button.dataset.bulk)) activeBatch.selected.clear();
    paintAll();
  });

  // ---- drawer -----------------------------------------------------------------
  const itemCache = new Map();

  function closeDrawer() {
    if (!drawer) return;
    document.removeEventListener('keydown', drawer.onKey);
    window.removeEventListener('resize', drawer.onResize);
    drawer.el.remove();
    drawer = null;
  }

  function openDrawer(name) {
    closeDrawer();
    const el = document.createElement('div');
    el.className = 'backdrop';
    document.body.append(el);
    drawer = { el, name, heatOn: true, onKey: null, onResize: null };
    drawer.onKey = (e) => {
      if (e.key === 'Escape') closeDrawer();
      if (e.key === 'ArrowRight') step(1);
      if (e.key === 'ArrowLeft') step(-1);
    };
    document.addEventListener('keydown', drawer.onKey);
    el.addEventListener('click', (e) => e.target === el && closeDrawer());
    fillDrawer();
  }

  function step(direction) {
    const list = visibleItems();
    const at = list.findIndex((i) => i.name === drawer.name);
    const next = list[at + direction];
    if (next) {
      drawer.name = next.name;
      fillDrawer();
    }
  }

  async function fillDrawer() {
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

    let item = itemCache.get(`${activeBatch.id}/${name}`);
    try {
      if (!item) {
        item = await getItem(activeBatch.id, name);
        itemCache.set(`${activeBatch.id}/${name}`, item);
      }
    } catch (err) {
      drawer.el.querySelector('#d-left').innerHTML = `<div class="notice danger">${esc(err.message)}</div>`;
      return;
    }
    if (!drawer || drawer.name !== name) return;
    paintDrawerItem(drawer, item);
  }

  function paintDrawerItem(d, item) {
    const left = d.el.querySelector('#d-left');
    const right = d.el.querySelector('#d-right');
    if (!item.response) {
      left.innerHTML = `<div class="notice danger">${esc(item.error ?? 'No result for this image')}</div>`;
      return;
    }
    const r = item.response;
    left.innerHTML = `<div class="preview-wrap"><img id="d-img" alt="${esc(item.name)}" src="${imageUrl(activeBatch.id, item.name)}"><canvas id="d-heat"></canvas></div>
      ${r.finding ? `<label class="check-row"><input type="checkbox" id="d-heat-on" checked> show where (heatmap)</label>` : ''}`;
    right.innerHTML = `${resultHtml(r)}${r.needs_review ? reviewBox() : ''}
      <div class="section-label" style="margin-top:1rem">Steps</div><div class="console short" id="d-console"></div>`;

    const paint = () => drawHeat(left.querySelector('#d-heat'), left.querySelector('#d-img'), r.finding,
      r.patch ? r.patch.threshold : 0, d.heatOn);
    left.querySelector('#d-img').addEventListener('load', paint);
    window.removeEventListener('resize', d.onResize);
    d.onResize = paint;
    window.addEventListener('resize', paint);
    const toggle = left.querySelector('#d-heat-on');
    if (toggle) toggle.addEventListener('change', () => { d.heatOn = toggle.checked; paint(); });

    new Console(right.querySelector('#d-console')).replay(r, false);
    right.addEventListener('click', async (e) => {
      const button = e.target.closest('[data-decision]');
      if (button) await decide([item.name], button.dataset.decision);
    });
  }

  // Auto-resume last active batch if present.
  const resumeId = store.get() || activeBatch.id;
  if (resumeId) {
    follow(resumeId, false);
  } else {
    loadRecent();
  }

  return () => {
    closeDrawer();
  };
}
