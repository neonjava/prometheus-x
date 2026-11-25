import { getStatus } from '../api.js';
import { esc, formatNumber, icon } from '../ui.js';

const GREETINGS = [
  [5, 'Good night,'],
  [12, 'Good morning,'],
  [18, 'Good afternoon,'],
  [24, 'Good evening,'],
];

// The greeting follows the visitor's own clock, not the server's.
function greeting() {
  const hour = new Date().getHours();
  return GREETINGS.find(([until]) => hour < until)[1];
}

const LINKS = {
  inspect: [
    ['#/check', 'scan', 'Check one image', 'Drop a photo and watch each step'],
    ['#/batch', 'grid', 'Check a batch', 'Upload a zip or many photos at once'],
  ],
  followUp: [
    ['#/review', 'review', 'Review queue', 'Decisions people have saved'],
    ['#/how', 'info', 'How it works', 'The three steps in plain words'],
  ],
};

function rows(list) {
  return list
    .map(
      ([href, iconName, title, note]) => `<a class="row-link" href="${href}">
        ${icon(iconName)}<div><b>${esc(title)}</b><span class="note">${esc(note)}</span></div>
        <span class="chev">${icon('chevron')}</span></a>`,
    )
    .join('');
}

function figure(label, id) {
  return `<div class="figure"><div class="label">${label}</div><div class="value placeholder" id="${id}">–</div></div>`;
}

export async function mountHome(root) {
  root.innerHTML = `
    <div class="hero">
      <div>
        <small class="greet">${greeting()}</small>
        <h1 class="big">Inspection desk</h1>
        <p class="sub">Trained on good images only, so it can spot defects it has never seen.</p>
        <div class="figures">
          ${figure('Good images learned', 'f-good')}
          ${figure('Patch memory', 'f-patch')}
          ${figure('Waiting for review', 'f-waiting')}
          ${figure('Reviews saved', 'f-reviews')}
        </div>
      </div>
      <div class="card how-card">
        <h3>How a photo is checked <span>3 steps</span></h3>
        <div class="headline">Normal photos pass in one step.</div>
        <div class="track"></div>
        <div class="dots">
          <div><i></i><b>Step 1</b>Quick check</div>
          <div><i></i><b>Step 2</b>Closer look</div>
          <div><i></i><b>Step 3</b>A person decides</div>
        </div>
      </div>
    </div>

    <label class="search">${icon('search')}<input id="find" placeholder="Find a page, like batch or review" autocomplete="off"></label>

    <div class="grid-2">
      <div class="panel rows" data-group><h2>Inspect</h2>${rows(LINKS.inspect)}</div>
      <div class="panel rows" data-group><h2>Follow up</h2>${rows(LINKS.followUp)}</div>
    </div>`;

  const find = root.querySelector('#find');
  find.addEventListener('input', () => {
    const query = find.value.trim().toLowerCase();
    root.querySelectorAll('.row-link').forEach((row) => {
      row.hidden = query !== '' && !row.textContent.toLowerCase().includes(query);
    });
  });

  try {
    const s = await getStatus();
    const set = (id, value) => {
      const el = root.querySelector(`#${id}`);
      el.textContent = formatNumber(value);
      el.classList.remove('placeholder');
    };
    set('f-good', s.good_vectors);
    set('f-patch', s.patch_rows);
    set('f-waiting', s.waiting_for_review ?? 0);
    set('f-reviews', s.reviews);
  } catch {
    root.querySelector('.sub').textContent = 'The server is not answering. Start it and refresh this page.';
  }
  return null;
}
