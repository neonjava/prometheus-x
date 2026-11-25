import { predictImage, saveDecision } from '../api.js';
import {
  bindDropzone, Console, drawHeat, esc, icon, resultHtml, reviewBox, toast, VERDICT_CLASS,
} from '../ui.js';

export function mountCheck(root) {
  root.innerHTML = `
    <div class="page-head"><div><h1>Check one image</h1><p>Drop a photo and watch each step of the check.</p></div></div>
    <div class="grid-2">
      <div style="display:grid;gap:1.6rem">
        <div class="card">
          <div class="section-label">Add image</div>
          <div class="dropzone" id="zone">${icon('upload')}<b id="zone-text">Click or drop a product photo here</b><small>JPG, PNG, BMP or WebP</small></div>
          <input class="hidden-input" type="file" id="input" accept="image/*">
          <button class="btn primary block" id="go" style="margin-top:.9rem" disabled>Check image</button>
          <div class="preview-wrap" id="preview-wrap" hidden><img id="preview" alt="Uploaded product"><canvas id="heat"></canvas></div>
          <label class="check-row" id="heat-row" hidden><input type="checkbox" id="heat-on" checked> show where (heatmap)</label>
          <div class="notice danger" id="error" hidden></div>
        </div>
        <div class="card" id="result" hidden></div>
      </div>
      <div class="card"><div class="section-label">Console</div><div class="console" id="console"><span class="t">Waiting for an image…</span></div></div>
    </div>`;

  const $ = (sel) => root.querySelector(sel);
  const out = new Console($('#console'));
  const state = { file: null, result: null };

  function selectFile(files) {
    state.file = files[0];
    $('#preview').src = URL.createObjectURL(state.file);
    $('#preview-wrap').hidden = false;
    $('#zone-text').textContent = state.file.name;
    $('#go').disabled = false;
    $('#result').hidden = true;
    $('#heat-row').hidden = true;
    $('#error').hidden = true;
    $('#heat').getContext('2d').clearRect(0, 0, $('#heat').width, $('#heat').height);
    out.clear();
    out.line(`Selected ${state.file.name}. Press "Check image".`);
  }

  bindDropzone($('#zone'), $('#input'), selectFile);

  function paintHeat() {
    const r = state.result;
    drawHeat($('#heat'), $('#preview'), r.finding, r.patch ? r.patch.threshold : 0, $('#heat-on').checked);
  }
  $('#heat-on').addEventListener('change', () => state.result && paintHeat());

  function showResult(r) {
    state.result = r;
    const box = $('#result');
    box.hidden = false;
    box.innerHTML = `<div class="section-label">Result</div>${resultHtml(r)}${r.needs_review ? reviewBox() : ''}`;
    $('#heat-row').hidden = !r.finding;
    paintHeat();
  }

  $('#go').addEventListener('click', async () => {
    $('#error').hidden = true;
    $('#result').hidden = true;
    out.clear();
    out.line(`Sending ${state.file.name} to the server…`);
    $('#go').disabled = true;
    $('#go').textContent = 'Checking…';
    try {
      const result = await predictImage(state.file);
      await out.replay(result);
      showResult(result);
    } catch (err) {
      $('#error').textContent = err.message;
      $('#error').hidden = false;
      out.line(`ERROR: ${err.message}`, null, 'bad');
    } finally {
      $('#go').disabled = false;
      $('#go').textContent = 'Check image';
    }
  });

  $('#result').addEventListener('click', async (e) => {
    const button = e.target.closest('[data-decision]');
    if (!button) return;
    try {
      await saveDecision(state.file.name, state.result.verdict, button.dataset.decision);
      const message = `Decision saved: ${button.dataset.decision.toUpperCase()}`;
      toast(message);
      out.line(message, null, VERDICT_CLASS.Good);
    } catch (err) {
      toast(`Could not save: ${esc(err.message)}`, 'error');
    }
  });
  return null;
}
