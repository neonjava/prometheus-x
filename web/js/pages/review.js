import { getDecisions, getPendingReviews, saveDecision } from '../api.js';
import { esc, formatTime, toast } from '../ui.js';

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
          (p) => `<tr>
            <td style="width:52px">
              <img src="${esc(p.thumb_url)}" alt="${esc(p.name)}"
                   style="width:40px;height:40px;border-radius:6px;object-fit:cover;background:#eee;display:block"
                   onerror="this.style.opacity=.2">
            </td>
            <td>
              <a href="#/batch" data-open-batch="${esc(p.batch_id)}" style="font-weight:600;color:var(--text)">
                ${esc(p.batch_name)}
              </a>
            </td>
            <td><b>${esc(p.name)}</b></td>
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

  waitingBody.addEventListener('click', async (e) => {
    const link = e.target.closest('[data-open-batch]');
    if (link) {
      try { localStorage.setItem('px-last-batch', link.dataset.openBatch); } catch { /* storage can be blocked */ }
      return;
    }
    const button = e.target.closest('[data-action]');
    if (!button) return;

    const { action, image, verdict } = button.dataset;
    button.disabled = true;
    try {
      await saveDecision(image, verdict, action);
      const at = pending.findIndex((p) => p.image === image);
      if (at >= 0) {
        const [item] = pending.splice(at, 1);
        decided.unshift({
          time_unix: Math.floor(Date.now() / 1000),
          image: item.image,
          verdict: item.verdict,
          decision: action,
        });
      }
      toast(`Marked ${action.toUpperCase()}`);
      paintWaiting();
      paintDecided();
    } catch (err) {
      button.disabled = false;
      toast(`Could not save: ${err.message}`, 'error');
    }
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

  return null;
}
