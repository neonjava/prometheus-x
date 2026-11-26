import { getDecisions, getItem, getPendingReviews, imageUrl, saveDecision } from '../api.js';
import { mountHeatView } from '../heatview.js';
import { esc, formatTime, resultHtml, reviewBox, toast } from '../ui.js';

const FILTERS = [
  ['all', 'All'],
  ['ok', 'OK'],
  ['reject', 'Reject'],
  ['fix', 'Fix'],
];
const DECISION_TAG = { ok: 'ok', reject: 'danger', fix: 'warn' };

export async function mountReview(root) {
  root.innerHTML = `
    <div class="page-head">
      <div>
        <h1>Review queue</h1>
        <p>Review uncertain photos across batches and inspect past decisions.</p>
      </div>
    </div>

    <div class="card" style="margin-bottom:1.5rem">
      <div class="section-label" id="waiting-label">Waiting for review</div>
      <div id="waiting-body"><div class="empty">Loading pending photos…</div></div>
    </div>

    <div class="card">
      <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:1rem;flex-wrap:wrap;gap:.5rem">
        <div class="section-label" style="margin:0">Decided</div>
        <div class="segmented" id="filters"></div>
      </div>
      <div id="decided-body"><div class="empty">Loading decisions…</div></div>
    </div>`;

  const waitingBody = root.querySelector('#waiting-body');
  const decidedBody = root.querySelector('#decided-body');
  const filters = root.querySelector('#filters');
  const waitingLabel = root.querySelector('#waiting-label');

  let pending = [];
  let decided = [];
  let currentFilter = 'all';

  function paintWaiting() {
    waitingLabel.textContent = `Waiting for review (${pending.length})`;
    if (!pending.length) {
      waitingBody.innerHTML = `<div class="empty">No photos waiting for review. All batches are clear!</div>`;
      return;
    }
    waitingBody.innerHTML = `<div class="table"><table>
      <thead>
        <tr>
          <th>Thumb</th>
          <th>Batch</th>
          <th>Photo</th>
          <th>Verdict</th>
          <th style="text-align:right">Action</th>
        </tr>
      </thead>
      <tbody>${pending
        .map(
          (p) => `<tr class="clickable" data-peek="${esc(p.image)}">
            <td style="width:52px">
              <img src="${esc(p.thumb_url)}" alt="${esc(p.name)}"
                   style="width:40px;height:40px;border-radius:6px;object-fit:cover;background:#eee;display:block"
                   onerror="this.style.opacity=.2">
            </td>
            <td>
              ${esc(p.batch_name)}
            </td>
            <td><button type="button" class="link-button">${esc(p.name)}</button></td>
            <td><span class="tag warn">${esc(p.verdict)}</span></td>
            <td style="text-align:right;white-space:nowrap">
              <button class="btn sm ok" data-action="ok" data-image="${esc(p.image)}" data-verdict="${esc(p.verdict)}">OK</button>
              <button class="btn sm danger" data-action="reject" data-image="${esc(p.image)}" data-verdict="${esc(p.verdict)}">Reject</button>
              <button class="btn sm warn" data-action="fix" data-image="${esc(p.image)}" data-verdict="${esc(p.verdict)}">Fix</button>
            </td>
          </tr>`,
        )
        .join('')}</tbody></table></div>`;
  }

  function paintDecided() {
    const count = (key) => (key === 'all' ? decided.length : decided.filter((d) => d.decision === key).length);
    filters.innerHTML = FILTERS.map(
      ([key, label]) => `<button data-key="${key}" aria-pressed="${key === currentFilter}">${label} <em>${count(key)}</em></button>`,
    ).join('');
    const shown = currentFilter === 'all' ? decided : decided.filter((d) => d.decision === currentFilter);
    if (!shown.length) {
      decidedBody.innerHTML = `<div class="empty">${decided.length ? 'Nothing in this group.' : 'No decisions saved yet.'}</div>`;
      return;
    }
    decidedBody.innerHTML = `<div class="table"><table>
      <thead><tr><th>When</th><th>Image</th><th>Verdict</th><th>Decision</th></tr></thead>
      <tbody>${shown
        .map(
          (d) => `<tr><td class="num">${esc(formatTime(d.time_unix))}</td><td>${esc(d.image)}</td>
            <td>${esc(d.verdict)}</td><td><span class="tag ${DECISION_TAG[d.decision] ?? ''}">${esc(d.decision)}</span></td></tr>`,
        )
        .join('')}</tbody></table></div>`;
  }

  filters.addEventListener('click', (e) => {
    const button = e.target.closest('button');
    if (!button) return;
    currentFilter = button.dataset.key;
    paintDecided();
  });

  async function decide(item, action) {
    try {
      await saveDecision(item.image, item.verdict, action);
    } catch (err) {
      toast(`Could not save: ${err.message}`, 'error');
      return false;
    }
    pending = pending.filter((p) => p.image !== item.image);
    decided.unshift({
      time_unix: Math.floor(Date.now() / 1000),
      image: item.image,
      verdict: item.verdict,
      decision: action,
    });
    toast(`Marked ${action.toUpperCase()}`);
    paintWaiting();
    paintDecided();
    return true;
  }

  // ---- popup: look at one photo without leaving the queue -----------------

  let popup = null;

  function closePopup() {
    if (!popup) return;
    popup.release?.();
    document.removeEventListener('keydown', popup.onKey);
    popup.el.remove();
    popup = null;
  }

  async function openPopup(item) {
    closePopup();
    const el = document.createElement('div');
    el.className = 'backdrop';
    el.innerHTML = `<div class="drawer peek" role="dialog" aria-label="${esc(item.name)}">
      <div class="drawer-head"><h2>${esc(item.name)} <span class="muted small">batch ${esc(item.batch_name)}</span></h2>
        <button class="btn sm" data-close>Close</button></div>
      <div id="peek-result"><div class="empty">Loading…</div></div><div id="peek-view"></div></div>`;
    const onKey = (e) => e.key === 'Escape' && closePopup();
    popup = { el, onKey, release: null };
    document.addEventListener('keydown', onKey);
    document.body.append(el);
    el.addEventListener('click', async (e) => {
      if (e.target === el || e.target.closest('[data-close]')) {
        closePopup();
        return;
      }
      const button = e.target.closest('[data-decision]');
      if (button && (await decide(item, button.dataset.decision))) closePopup();
    });

    const mine = popup;
    try {
      const { response, error } = await getItem(item.batch_id, item.name);
      if (popup !== mine) return;
      if (!response) throw new Error(error ?? 'No result for this photo');
      el.querySelector('#peek-result').innerHTML = `${resultHtml(response)}${reviewBox()}`;
      mine.release = mountHeatView(el.querySelector('#peek-view'), imageUrl(item.batch_id, item.name), response);
    } catch (err) {
      el.querySelector('#peek-result').innerHTML = `<div class="notice danger">${esc(err.message)}</div>`;
    }
  }

  waitingBody.addEventListener('click', async (e) => {
    const row = e.target.closest('[data-peek]');
    const action = e.target.closest('[data-action]');
    if (action) {
      const item = pending.find((p) => p.image === action.dataset.image);
      action.disabled = true;
      if (item && !(await decide(item, action.dataset.action))) action.disabled = false;
      return;
    }
    const item = row && pending.find((p) => p.image === row.dataset.peek);
    if (item) openPopup(item);
  });

  try {
    const [p, d] = await Promise.all([getPendingReviews(), getDecisions()]);
    pending = p;
    decided = d;
    paintWaiting();
    paintDecided();
  } catch (err) {
    waitingBody.innerHTML = `<div class="empty">Could not load review queue: ${esc(err.message)}</div>`;
    decidedBody.innerHTML = '';
  }

  return closePopup;
}
