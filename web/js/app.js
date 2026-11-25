import { getStatus } from './api.js';
import { blockStrayDrops, esc, formatNumber, icon, TAB_LABELS } from './ui.js';
import { mountBatch } from './pages/batch.js';
import { mountCheck } from './pages/check.js';
import { mountHome } from './pages/home.js';
import { mountHow } from './pages/how.js';
import { mountReview } from './pages/review.js';

const ROUTES = {
  '/': mountHome,
  '/check': mountCheck,
  '/batch': mountBatch,
  '/review': mountReview,
  '/how': mountHow,
};
const STATUS_REFRESH_MS = 15000;

const view = document.getElementById('view');
const glide = document.getElementById('glide');
let cleanup = null;

function paintTabs() {
  document.querySelectorAll('.tab').forEach((tab) => {
    const [iconName, label] = TAB_LABELS[tab.dataset.route];
    tab.innerHTML = `${icon(iconName)}<span>${label}</span>`;
    tab.title = label;
  });
}

// The white pill glides under the current tab.
function moveGlide() {
  const active = document.querySelector('.tab[aria-current="page"]');
  if (!active) return;
  glide.style.left = `${active.offsetLeft}px`;
  glide.style.width = `${active.offsetWidth}px`;
}

function markActive(path) {
  document.querySelectorAll('.tab').forEach((tab) => {
    if (tab.dataset.route === path) tab.setAttribute('aria-current', 'page');
    else tab.removeAttribute('aria-current');
  });
  // Wait a frame so the active tab has its label and final width.
  requestAnimationFrame(moveGlide);
}

async function render() {
  const requested = location.hash.replace(/^#/, '') || '/';
  const path = requested in ROUTES ? requested : '/';
  if (cleanup) cleanup();
  view.innerHTML = '';
  const page = document.createElement('section');
  page.className = 'page';
  view.append(page);
  markActive(path);
  window.scrollTo(0, 0);
  cleanup = (await ROUTES[path](page)) ?? null;
}

async function refreshStatus() {
  const pill = document.getElementById('status-pill');
  try {
    const s = await getStatus();
    pill.className = 'status-pill ready';
    pill.innerHTML = `<i class="status-dot"></i><span>Model ready <span class="long">· <b>${formatNumber(s.good_vectors)}</b> good images learned</span></span>`;
  } catch (err) {
    pill.className = 'status-pill down';
    pill.innerHTML = `<i class="status-dot"></i><span>Server offline</span>`;
    console.error(esc(err.message));
  }
}

paintTabs();
blockStrayDrops();
window.addEventListener('hashchange', render);
window.addEventListener('resize', moveGlide);
refreshStatus();
setInterval(refreshStatus, STATUS_REFRESH_MS);
render();
