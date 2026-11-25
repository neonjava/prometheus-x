const $ = (id) => document.getElementById(id);

const REPLAY_MS = 220;
const HEAT_RGB = '176, 96, 90';

const els = {
  input: $('image-input'),
  zone: $('drop-zone'),
  form: $('upload-form'),
  button: $('submit-btn'),
  console: $('console'),
  preview: $('preview'),
  heat: $('heat'),
  heatCheck: $('heat-check'),
  error: $('error'),
};

const state = { result: null, fileName: '' };

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// ---- console ----------------------------------------------------------

function clearConsole() {
  els.console.innerHTML = '';
}

function logLine(text, ms, cls) {
  const line = document.createElement('div');
  const stamp = document.createElement('span');
  stamp.className = 't';
  stamp.textContent = ms === null ? '' : `[${String(ms).padStart(4)} ms] `;
  const body = document.createElement('span');
  if (cls) body.className = cls;
  body.textContent = text;
  line.append(stamp, body);
  els.console.append(line);
  els.console.scrollTop = els.console.scrollHeight;
}

function lineClass(message) {
  if (/WARNING|looks odd/.test(message)) return 'warn';
  if (/REJECTED/.test(message)) return 'bad';
  if (/PASS|OK|Step 1 passed/.test(message)) return 'ok';
  return '';
}

const VERDICT_CLASS = { Good: 'ok', Defect: 'bad', Uncertain: 'warn' };

// ---- heatmap ----------------------------------------------------------

function clearHeat() {
  els.heat.getContext('2d').clearRect(0, 0, els.heat.width, els.heat.height);
}

// The server resizes the photo to a square grid, so each cell covers an equal share of the image.
function drawHeat(finding, threshold) {
  els.heat.width = els.preview.clientWidth;
  els.heat.height = els.preview.clientHeight;
  clearHeat();
  if (!finding || !els.heatCheck.checked) return;

  const [rows, cols] = finding.grid;
  const peak = Math.max(...finding.map);
  const cellW = els.heat.width / cols;
  const cellH = els.heat.height / rows;
  const ctx = els.heat.getContext('2d');

  finding.map.forEach((distance, i) => {
    if (distance <= threshold) return;
    const strength = Math.min(1, (distance - threshold) / Math.max(peak - threshold, 1e-6));
    ctx.fillStyle = `rgba(${HEAT_RGB}, ${0.18 + 0.5 * strength})`;
    ctx.fillRect((i % cols) * cellW, Math.floor(i / cols) * cellH, cellW + 1, cellH + 1);
  });
}

// ---- result card ------------------------------------------------------

const BANNERS = {
  Good: ['OK, passes', 'Step 1 found nothing odd.'],
  Defect: ['Rejected', 'Both checks flag damage.'],
  Uncertain: ['Needs a person', 'The two checks disagree or are unsure.'],
};

function row(label, value) {
  return `<div class="row"><span>${label}</span><span>${value}</span></div>`;
}

function describeModel(model) {
  if (!model) return 'skipped (Step 1 passed)';
  return `${model.vote} · ${model.score.toFixed(2)} / ${model.threshold.toFixed(2)}`;
}

function findingRows(finding) {
  if (!finding) return '';
  return (
    row('Where', `${finding.area_pct.toFixed(1)}% of the image`) +
    row('How bad', finding.severity) +
    row('How sure (defect probability)', `${Math.round(finding.defect_probability * 100)}%`)
  );
}

function showShift(shift) {
  const el = $('shift');
  el.style.display = shift.warning ? 'block' : 'none';
  el.textContent =
    `Warning: this photo looks different from the training images ` +
    `(distance ${shift.distance.toFixed(2)} > ${shift.limit.toFixed(2)}). Lighting or camera may have changed.`;
}

function showResult(result) {
  state.result = result;
  $('result').style.display = 'block';

  const [title, subtitle] = BANNERS[result.verdict];
  const banner = $('banner');
  banner.className = `banner ${result.verdict}`;
  banner.innerHTML = `${title}<small>${subtitle}</small>`;

  $('rows').innerHTML =
    row('Step 1 · global check', describeModel(result.global)) +
    row('Step 2 · patch check', describeModel(result.patch)) +
    findingRows(result.finding);

  showShift(result.shift);
  $('review').style.display = result.needs_review ? 'block' : 'none';
  $('saved').textContent = '';
  $('heat-toggle').style.display = result.finding ? 'block' : 'none';
  drawHeat(result.finding, result.patch ? result.patch.threshold : 0);
}

// ---- file selection ---------------------------------------------------

function selectFile(file) {
  state.fileName = file.name;
  els.preview.src = URL.createObjectURL(file);
  $('preview-wrap').style.display = 'block';
  els.zone.textContent = file.name;
  els.error.style.display = 'none';
  $('result').style.display = 'none';
  $('heat-toggle').style.display = 'none';
  clearHeat();
  clearConsole();
  logLine(`Selected ${file.name}. Press "Check image".`, null);
}

els.input.addEventListener('change', () => {
  if (els.input.files[0]) selectFile(els.input.files[0]);
});
els.zone.addEventListener('click', () => els.input.click());
['dragenter', 'dragover'].forEach((type) =>
  els.zone.addEventListener(type, (e) => { e.preventDefault(); els.zone.classList.add('over'); }));
['dragleave', 'drop'].forEach((type) =>
  els.zone.addEventListener(type, (e) => { e.preventDefault(); els.zone.classList.remove('over'); }));
els.zone.addEventListener('drop', (e) => {
  if (!e.dataTransfer.files.length) return;
  els.input.files = e.dataTransfer.files;
  selectFile(els.input.files[0]);
});
// Stop the browser from opening a file dropped outside the zone.
['dragover', 'drop'].forEach((type) => window.addEventListener(type, (e) => e.preventDefault()));
els.heatCheck.addEventListener('change', () => {
  if (state.result) drawHeat(state.result.finding, state.result.patch.threshold);
});

// ---- talking to the server --------------------------------------------

async function requestPrediction(file) {
  const data = new FormData();
  data.append('image', file);
  const res = await fetch('/predict', { method: 'POST', body: data });
  if (!res.ok) throw new Error(`Server returned ${res.status}: ${await res.text()}`);
  return res.json();
}

async function replayLog(result) {
  for (const line of result.log) {
    logLine(line.message, line.ms, lineClass(line.message));
    await sleep(REPLAY_MS);
  }
  logLine(`Final verdict: ${result.verdict.toUpperCase()}`, null, VERDICT_CLASS[result.verdict]);
}

function showError(err) {
  els.error.textContent = err.message;
  els.error.style.display = 'block';
  logLine(`ERROR: ${err.message}`, null, 'bad');
}

els.form.addEventListener('submit', async (e) => {
  e.preventDefault();
  const file = els.input.files[0];
  if (!file) return;

  els.error.style.display = 'none';
  $('result').style.display = 'none';
  clearConsole();
  logLine(`Sending ${file.name} to the server…`, null);
  els.button.disabled = true;
  els.button.textContent = 'Checking…';
  try {
    const result = await requestPrediction(file);
    await replayLog(result);
    showResult(result);
  } catch (err) {
    showError(err);
  } finally {
    els.button.disabled = false;
    els.button.textContent = 'Check image';
  }
});

document.querySelectorAll('[data-decision]').forEach((button) =>
  button.addEventListener('click', async () => {
    const body = JSON.stringify({
      image: state.fileName,
      verdict: state.result.verdict,
      decision: button.dataset.decision,
    });
    const res = await fetch('/decision', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
    });
    const ok = res.ok;
    const message = ok
      ? `Decision saved: ${button.dataset.decision.toUpperCase()}`
      : `Could not save: ${await res.text()}`;
    $('saved').textContent = message;
    logLine(message, null, ok ? 'ok' : 'bad');
  }));
