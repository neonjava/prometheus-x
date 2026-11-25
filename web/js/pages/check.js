import { getStatus, predictImage, saveDecision } from '../api.js';
import {
  bindDropzone, Console, esc, fillProducts, icon, resultHtml, reviewBox, toast, VERDICT_CLASS,
} from '../ui.js';
import { mountHeatView } from '../heatview.js';

export function mountCheck(root) {
  root.innerHTML = `
    <div class="page-head"><div><h1>Check one image</h1><p>Drop a photo and watch each step of the check.</p></div></div>
    <div class="grid-2">
      <div style="display:grid;gap:1.6rem">
        <div class="card">
          <div class="section-label">Add image</div>
          <select class="field product-pick" id="product" aria-label="Product"><option value="auto">Auto-detect product</option></select>
          <div class="dropzone" id="zone">${icon('upload')}<b id="zone-text">Click or drop a product photo here</b><small>JPG, PNG, BMP or WebP</small></div>
          <input class="hidden-input" type="file" id="input" accept="image/*">
          <button class="btn primary block" id="go" style="margin-top:.9rem" disabled>Check image</button>
          <div id="viewer"></div>
          <div class="notice danger" id="error" hidden></div>
        </div>
        <div class="card" id="result" hidden></div>
      </div>
      <div class="card"><div class="section-label">Console</div><div class="console" id="console"><span class="t">Waiting for an image…</span></div></div>
    </div>`;

  const $ = (sel) => root.querySelector(sel);
  const out = new Console($('#console'));
  const state = { file: null, result: null };

  let release = () => {};

  function selectFile(files) {
    state.file = files[0];
    state.url = URL.createObjectURL(state.file);
    release();
    $('#viewer').innerHTML = `<div class="preview-wrap"><img alt="Uploaded product" src="${state.url}"></div>`;
    $('#zone-text').textContent = state.file.name;
    $('#go').disabled = false;
    $('#result').hidden = true;
    $('#error').hidden = true;
    out.clear();
    out.line(`Selected ${state.file.name}. Press "Check image".`);
  }

  bindDropzone($('#zone'), $('#input'), selectFile);
  getStatus().then((s) => fillProducts($('#product'), s.products)).catch(() => {});

  function showResult(r) {
    state.result = r;
    const box = $('#result');
    box.hidden = false;
    box.innerHTML = `<div class="section-label">Result</div>${resultHtml(r)}${r.needs_review ? reviewBox() : ''}`;
    release();
    release = mountHeatView($('#viewer'), state.url, r);
  }

  $('#go').addEventListener('click', async () => {
    $('#error').hidden = true;
    $('#result').hidden = true;
    out.clear();
    out.line(`Sending ${state.file.name} to the server…`);
    $('#go').disabled = true;
    $('#go').textContent = 'Checking…';
    try {
      const result = await predictImage(state.file, $('#product').value);
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
  return () => release();
}
