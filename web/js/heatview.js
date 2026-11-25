// Photo viewer: smooth heatmap, numbered spots, close-ups and a plain-language reason.
import { esc } from './ui.js';

const MODES = [['both', 'Heat + spots'], ['heat', 'Heat'], ['spots', 'Spots'], ['off', 'Photo']];
const MAX_SPOTS = 3;
// Colour starts a little under the limit so nearly-odd areas show faintly.
const HEAT_START = 0.92;
const HEAT_FULL = 1.25;
const COLOURS = [[255, 214, 64], [245, 134, 40], [208, 52, 46]];
const CROP_PX = 150;
const CROP_PADDING_CELLS = 1.5;

// Kept between photos so stepping through a batch does not reset the view.
const prefs = { mode: 'both', strength: 75 };

/** Groups neighbouring patches that are over the limit into spots, strongest first. */
function findSpots(finding, limit) {
  if (!finding) return [];
  const [rows, cols] = finding.grid;
  const seen = new Set();
  const spots = [];
  finding.map.forEach((value, start) => {
    if (value <= limit || seen.has(start)) return;
    const spot = { x0: cols, y0: rows, x1: 0, y1: 0, cells: 0, peak: 0 };
    const queue = [start];
    seen.add(start);
    while (queue.length) {
      const i = queue.pop();
      const x = i % cols;
      const y = Math.floor(i / cols);
      Object.assign(spot, {
        x0: Math.min(spot.x0, x), y0: Math.min(spot.y0, y),
        x1: Math.max(spot.x1, x + 1), y1: Math.max(spot.y1, y + 1),
        cells: spot.cells + 1, peak: Math.max(spot.peak, finding.map[i]),
      });
      for (let dy = -1; dy <= 1; dy += 1) {
        for (let dx = -1; dx <= 1; dx += 1) {
          const nx = x + dx;
          const ny = y + dy;
          const n = ny * cols + nx;
          if (nx >= 0 && nx < cols && ny >= 0 && ny < rows && !seen.has(n) && finding.map[n] > limit) {
            seen.add(n);
            queue.push(n);
          }
        }
      }
    }
    spot.areaPct = (100 * spot.cells) / (rows * cols);
    spot.overPct = 100 * (spot.peak / limit - 1);
    spot.where = describePlace((spot.x0 + spot.x1) / 2 / cols, (spot.y0 + spot.y1) / 2 / rows);
    spots.push(spot);
  });
  return spots.sort((a, b) => b.peak - a.peak).slice(0, MAX_SPOTS);
}

function describePlace(x, y) {
  const third = (v, names) => names[Math.min(2, Math.floor(v * 3))];
  const vertical = third(y, ['top', 'middle', 'bottom']);
  const horizontal = third(x, ['left', 'centre', 'right']);
  if (vertical === 'middle' && horizontal === 'centre') return 'centre';
  return `${vertical} ${horizontal}`;
}

const over = (model) => Math.round(100 * (model.score / model.threshold - 1));

function checkReason(label, model, what) {
  if (!model) return null;
  if (model.vote === 'Uncertain') {
    const side = model.score > model.threshold ? 'over' : 'under';
    return `<b>${label}:</b> only ${Math.abs(over(model))}% ${side} the limit, too close to be sure.`;
  }
  if (model.score > model.threshold) return `<b>${label}:</b> ${what} is ${over(model)}% past the limit.`;
  return `<b>${label}:</b> within the limit (${model.score.toFixed(2)} of ${model.threshold.toFixed(2)}).`;
}

function reasons(result, spots) {
  const list = [
    checkReason('Whole photo', result.global, 'its distance from the closest good photo'),
    checkReason('Small areas', result.patch, 'the least familiar patch'),
  ];
  if (result.verdict === 'Good') list.push('Nothing looks unlike the good photos, so it passes.');
  if (result.verdict === 'Uncertain') list.push('A score is close to its limit or the two checks disagree, so a person should look.');
  if (result.verdict !== 'Good' && result.patch && !spots.length) {
    list.push('No single spot is over the limit. The photo differs as a whole: check position, lighting and focus.');
  }
  if (result.shift.warning) list.push('Lighting or camera looks different from training, which may be part of the cause.');
  return list.filter(Boolean);
}

const HEADINGS = { Good: 'Why it passed', Defect: 'Why it was rejected', Uncertain: 'Why it needs a person' };

function whyHtml(result, spots) {
  if (result.verdict === 'Unknown') {
    return `<div class="why"><h3>Why it was not judged</h3><p>The closest learned product is <b>${esc(result.product)}</b>, but this photo is ${(result.global.score / result.global.threshold).toFixed(1)} times its limit away. Pick the product by hand if you know it is one of them.</p></div>`;
  }
  const spotCards = spots
    .map(
      (s, i) => `<figure class="spot"><canvas width="${CROP_PX}" height="${CROP_PX}" data-spot="${i}"></canvas>
        <figcaption><b>Spot ${i + 1} · ${s.where}</b><span>${s.areaPct.toFixed(1)}% of the photo</span>
        <span>${Math.round(s.overPct)}% past the limit</span></figcaption></figure>`,
    )
    .join('');
  const note = result.verdict === 'Good'
    ? ''
    : '<p class="why-note">Only good products were learned, so the system cannot name the defect type. It shows where it looks wrong and by how much.</p>';
  return `<div class="why"><h3>${esc(HEADINGS[result.verdict])}</h3>
    <ul>${reasons(result, spots).map((r) => `<li>${r}</li>`).join('')}</ul>
    ${spots.length ? `<div class="spots">${spotCards}</div>` : ''}${note}</div>`;
}

function mix(t) {
  const scaled = t * (COLOURS.length - 1);
  const i = Math.min(COLOURS.length - 2, Math.floor(scaled));
  const f = scaled - i;
  return COLOURS[i].map((c, k) => Math.round(c + (COLOURS[i + 1][k] - c) * f));
}

// One pixel per patch; scaling it up with smoothing turns the blocks into soft blobs.
function heatImage(finding, limit) {
  const [rows, cols] = finding.grid;
  const small = document.createElement('canvas');
  small.width = cols;
  small.height = rows;
  const ctx = small.getContext('2d');
  const pixels = ctx.createImageData(cols, rows);
  const low = limit * HEAT_START;
  const high = Math.max(limit * HEAT_FULL, ...finding.map);
  finding.map.forEach((value, i) => {
    if (value <= low) return;
    const t = Math.min(1, (value - low) / (high - low));
    pixels.data.set([...mix(t), Math.round(255 * (0.35 + 0.65 * t))], i * 4);
  });
  ctx.putImageData(pixels, 0, 0);
  return small;
}

function drawSpots(ctx, spots, cellW, cellH) {
  spots.forEach((s, i) => {
    const x = s.x0 * cellW - 3;
    const y = s.y0 * cellH - 3;
    const w = (s.x1 - s.x0) * cellW + 6;
    const h = (s.y1 - s.y0) * cellH + 6;
    for (const [colour, width] of [['rgba(255,255,255,.95)', 4], ['rgb(208,52,46)', 2]]) {
      ctx.strokeStyle = colour;
      ctx.lineWidth = width;
      ctx.beginPath();
      ctx.roundRect(x, y, w, h, 8);
      ctx.stroke();
    }
    ctx.fillStyle = 'rgb(208,52,46)';
    ctx.beginPath();
    ctx.arc(x, y, 10, 0, 2 * Math.PI);
    ctx.fill();
    ctx.fillStyle = '#fff';
    ctx.font = '600 12px system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(String(i + 1), x, y);
  });
}

function drawCrop(canvas, img, spot, [rows, cols]) {
  const cellW = img.naturalWidth / cols;
  const cellH = img.naturalHeight / rows;
  const wanted = Math.max((spot.x1 - spot.x0 + 2 * CROP_PADDING_CELLS) * cellW,
    (spot.y1 - spot.y0 + 2 * CROP_PADDING_CELLS) * cellH);
  const side = Math.min(wanted, img.naturalWidth, img.naturalHeight);
  const clamp = (v, max) => Math.max(0, Math.min(v, max));
  const x = clamp(((spot.x0 + spot.x1) / 2) * cellW - side / 2, img.naturalWidth - side);
  const y = clamp(((spot.y0 + spot.y1) / 2) * cellH - side / 2, img.naturalHeight - side);
  canvas.getContext('2d').drawImage(img, x, y, side, side, 0, 0, canvas.width, canvas.height);
}

/** Fills `host` with the viewer for one result. Returns a function that releases it. */
export function mountHeatView(host, src, result) {
  const { finding } = result;
  const limit = result.patch ? result.patch.threshold : 0;
  const spots = findSpots(finding, limit);
  host.innerHTML = `<div class="preview-wrap"><img alt="Checked photo"><canvas></canvas></div>
    ${finding ? `<div class="heat-controls">
      <div class="segmented">${MODES.map(([key, label]) => `<button type="button" data-mode="${key}" aria-pressed="${prefs.mode === key}">${label}</button>`).join('')}</div>
      <label>Strength <input type="range" min="20" max="100" value="${prefs.strength}"></label>
    </div>` : ''}
    ${whyHtml(result, spots)}`;

  const img = host.querySelector('.preview-wrap img');
  const canvas = host.querySelector('.preview-wrap canvas');
  const heat = finding ? heatImage(finding, limit) : null;

  function paint() {
    canvas.width = img.clientWidth;
    canvas.height = img.clientHeight;
    if (!finding || !canvas.width) return;
    const ctx = canvas.getContext('2d');
    if (prefs.mode === 'both' || prefs.mode === 'heat') {
      ctx.imageSmoothingQuality = 'high';
      ctx.globalAlpha = prefs.strength / 100;
      ctx.drawImage(heat, 0, 0, canvas.width, canvas.height);
      ctx.globalAlpha = 1;
    }
    if (prefs.mode === 'both' || prefs.mode === 'spots') {
      drawSpots(ctx, spots, canvas.width / finding.grid[1], canvas.height / finding.grid[0]);
    }
  }

  img.addEventListener('load', () => {
    paint();
    host.querySelectorAll('[data-spot]').forEach((c) => drawCrop(c, img, spots[c.dataset.spot], finding.grid));
  });
  img.src = src;
  const observer = new ResizeObserver(paint);
  observer.observe(img);

  host.querySelector('.heat-controls')?.addEventListener('click', (e) => {
    const button = e.target.closest('[data-mode]');
    if (!button) return;
    prefs.mode = button.dataset.mode;
    host.querySelectorAll('[data-mode]').forEach((b) => b.setAttribute('aria-pressed', b === button));
    paint();
  });
  host.querySelector('.heat-controls input')?.addEventListener('input', (e) => {
    prefs.strength = Number(e.target.value);
    paint();
  });
  return () => observer.disconnect();
}
