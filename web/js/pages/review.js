import { getDecisions } from '../api.js';
import { esc, formatTime } from '../ui.js';

const FILTERS = [
  ['all', 'All'],
  ['ok', 'OK'],
  ['reject', 'Reject'],
  ['fix', 'Fix'],
];
const DECISION_TAG = { ok: 'ok', reject: 'danger', fix: 'warn' };

export async function mountReview(root) {
  root.innerHTML = `
    <div class="page-head"><div><h1>Review queue</h1><p>Every decision a person has saved, newest first.</p></div>
      <div class="segmented" id="filters"></div></div>
    <div id="body"><div class="empty">Loading…</div></div>`;
  const body = root.querySelector('#body');
  const filters = root.querySelector('#filters');
  let all = [];
  let current = 'all';

  function paint() {
    const count = (key) => (key === 'all' ? all.length : all.filter((d) => d.decision === key).length);
    filters.innerHTML = FILTERS.map(
      ([key, label]) => `<button data-key="${key}" aria-pressed="${key === current}">${label} <em>${count(key)}</em></button>`,
    ).join('');
    const shown = current === 'all' ? all : all.filter((d) => d.decision === current);
    if (!shown.length) {
      body.innerHTML = `<div class="empty">${all.length ? 'Nothing in this group.' : 'No decisions saved yet. Photos the two checks disagree on show up in the dashboard for review.'}</div>`;
      return;
    }
    body.innerHTML = `<div class="table"><table>
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
    current = button.dataset.key;
    paint();
  });

  try {
    all = await getDecisions();
    paint();
  } catch (err) {
    body.innerHTML = `<div class="empty">Could not load decisions: ${esc(err.message)}</div>`;
  }
  return null;
}
