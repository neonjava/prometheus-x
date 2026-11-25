// Camera mode: teach a product by filming it, inspect it live, or watch another device.
import { getLive, getStatus, learnProduct, liveFrameUrl, predictFrame, pushFrame } from '../api.js';
import { drawSpotHeat, liveSpots } from '../heatview.js';
import { esc, toast } from '../ui.js';

const FRAME_PX = 640;
const JPEG_QUALITY = 0.85;
const GUIDE_SHARE = 0.86;
const LEARN_FRAMES = 90;
const LEARN_EVERY_MS = 250;
const PREVIEW_EVERY_MS = 200;
const WATCH_EVERY_MS = 200;
const STALE_MS = 3000;
// A single odd frame (motion blur, a hand) should not raise the alarm.
const VOTES = 3;
const VOTES_NEEDED = 2;
// An added object covers many patches; one or two lit patches are glare or blur.
const MIN_SPOT_CELLS = 3;

const LABELS = { Good: 'Looks normal', Defect: 'Anomaly found', Uncertain: 'Not sure' };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function steadyVerdict(recent) {
  const count = (v) => recent.filter((r) => r === v).length;
  if (count('Defect') >= VOTES_NEEDED) return 'Defect';
  if (count('Good') >= VOTES_NEEDED) return 'Good';
  return 'Uncertain';
}

export function mountCamera(root) {
  root.innerHTML = `
    <div class="page-head"><div><h1>Camera check</h1><p>Teach it your object by filming it, then show it a scratch or a sticker.</p></div></div>
    <div class="grid-2">
      <div class="card">
        <div class="stage" id="stage">
          <video id="video" playsinline muted></video><img id="remote" alt="Live view from the other device" hidden>
          <canvas id="overlay"></canvas><div class="guide" id="guide"></div>
          <div class="stage-note" id="note">Start the camera here, or watch a phone that is filming.</div>
        </div>
        <div class="banner" id="verdict" hidden></div>
        <div class="actions" style="margin-top:.8rem">
          <button class="btn primary" id="start">Start camera</button>
          <button class="btn" id="watch">Watch other device</button>
        </div>
        <div class="notice danger" id="error" hidden></div>
      </div>
      <div style="display:grid;gap:1.6rem">
        <div class="card">
          <div class="section-label">1. Teach a good object</div>
          <p class="small muted">Remove anything extra from the object, then fill the square with it on a plain background. While it records, slowly move, turn and tilt it into every position you will show later. Keep the same background and light afterwards.</p>
          <input class="field" id="name" value="charger" aria-label="Object name" style="width:100%;margin:.7rem 0">
          <button class="btn primary block" id="learn" disabled>Record and learn (${Math.round((LEARN_FRAMES * LEARN_EVERY_MS) / 1000)} s)</button>
          <div class="progress" id="progress" hidden><div class="bar"><i id="bar"></i></div><div class="meta"><span id="progress-text"></span></div></div>
        </div>
        <div class="card">
          <div class="section-label">2. Inspect live</div>
          <select class="field" id="product" style="width:100%;margin-bottom:.7rem"></select>
          <button class="btn primary block" id="inspect" disabled>Start inspecting</button>
          <div class="facts" id="facts"></div>
        </div>
      </div>
    </div>`;

  const $ = (sel) => root.querySelector(sel);
  const video = $('#video');
  const grabber = document.createElement('canvas');
  grabber.width = FRAME_PX;
  grabber.height = FRAME_PX;
  const state = { stream: null, watching: false, inspecting: false, learning: false, gone: false, recent: [] };

  function showError(message) {
    $('#error').textContent = message;
    $('#error').hidden = !message;
  }

  async function loadProducts(selected) {
    const products = (await getStatus()).products.filter((p) => p.camera);
    $('#product').innerHTML = products.length
      ? products.map((p) => `<option value="${esc(p.name)}">${esc(p.name)}</option>`).join('')
      : '<option value="">Nothing taught yet</option>';
    if (selected) $('#product').value = selected;
    $('#inspect').disabled = !state.stream || !products.length;
  }

  // Only the guide square is sent, so the background around it matters less.
  function grab() {
    const side = Math.min(video.videoWidth, video.videoHeight) * GUIDE_SHARE;
    grabber.getContext('2d').drawImage(video, (video.videoWidth - side) / 2, (video.videoHeight - side) / 2,
      side, side, 0, 0, FRAME_PX, FRAME_PX);
    return new Promise((resolve) => grabber.toBlob(resolve, 'image/jpeg', JPEG_QUALITY));
  }

  // ---- showing a verdict ----------------------------------------------------

  function clearVerdict() {
    $('#verdict').hidden = true;
    $('#facts').innerHTML = '';
    const canvas = $('#overlay');
    canvas.getContext('2d').clearRect(0, 0, canvas.width, canvas.height);
  }

  function showVerdict(result) {
    const spots = liveSpots(result, MIN_SPOT_CELLS);
    const frame = result.verdict === 'Defect' && !spots.length ? 'Good' : result.verdict;
    state.recent = [...state.recent, frame].slice(-VOTES);
    const verdict = steadyVerdict(state.recent);
    const banner = $('#verdict');
    banner.hidden = false;
    banner.className = `banner ${verdict}`;
    banner.textContent = LABELS[verdict];
    const score = result.patch ?? result.global;
    $('#facts').innerHTML = `<div class="fact"><span>This frame</span><span>${esc(frame)}</span></div>
      <div class="fact"><span>Worst spot / limit</span><span>${score.score.toFixed(2)} / ${score.threshold.toFixed(2)}</span></div>`;
    // The frame is the guide square, so the heatmap is drawn over exactly that square.
    const guide = $('#guide');
    const canvas = $('#overlay');
    Object.assign(canvas.style, { left: `${guide.offsetLeft}px`, top: `${guide.offsetTop}px`,
      width: `${guide.offsetWidth}px`, height: `${guide.offsetHeight}px` });
    canvas.width = guide.offsetWidth;
    canvas.height = guide.offsetHeight;
    // Heat appears only for a confirmed anomaly, so a normal object stays clean.
    drawSpotHeat(canvas, result, verdict === 'Defect' ? spots : []);
  }

  // ---- this device's camera ---------------------------------------------------

  async function previewLoop() {
    while (state.stream && !state.gone) {
      if (video.videoWidth) {
        try { await pushFrame(await grab()); } catch { /* watching screens are optional */ }
      }
      await sleep(PREVIEW_EVERY_MS);
    }
  }

  async function startCamera() {
    showError('');
    if (!navigator.mediaDevices) {
      showError('The browser only allows the camera on localhost or https.');
      return;
    }
    try {
      state.stream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: 'environment', width: { ideal: 1280 }, height: { ideal: 720 } },
      });
    } catch (err) {
      showError(`Could not open the camera: ${err.message}`);
      return;
    }
    state.watching = false;
    $('#remote').hidden = true;
    video.hidden = false;
    video.srcObject = state.stream;
    await video.play();
    $('#note').hidden = true;
    $('#start').textContent = 'Camera on';
    $('#start').disabled = true;
    $('#learn').disabled = false;
    loadProducts();
    previewLoop();
  }

  async function learn() {
    const name = $('#name').value.trim().toLowerCase().replace(/\s+/g, '-');
    state.inspecting = false;
    state.learning = true;
    clearVerdict();
    $('#learn').disabled = true;
    $('#inspect').disabled = true;
    $('#progress').hidden = false;
    const frames = [];
    try {
      while (frames.length < LEARN_FRAMES && !state.gone) {
        frames.push(await grab());
        $('#bar').style.width = `${(100 * frames.length) / LEARN_FRAMES}%`;
        $('#progress-text').textContent = `Recording ${frames.length} of ${LEARN_FRAMES}: move it slightly`;
        await sleep(LEARN_EVERY_MS);
      }
      $('#progress-text').textContent = 'Learning… this takes about half a minute';
      await learnProduct(name, frames);
      $('#progress-text').textContent = `Learned "${name}". Now press Start inspecting.`;
      toast(`Learned ${name}`);
      await loadProducts(name);
    } catch (err) {
      $('#progress-text').textContent = '';
      showError(err.message);
    } finally {
      state.learning = false;
      $('#learn').disabled = false;
      $('#inspect').disabled = !$('#product').value;
    }
  }

  async function inspectLoop() {
    const product = $('#product').value;
    state.recent = [];
    while (state.inspecting && !state.gone) {
      try {
        const result = await predictFrame(await grab(), product);
        if (state.inspecting) showVerdict(result);
        showError('');
      } catch (err) {
        showError(err.message);
        await sleep(PREVIEW_EVERY_MS);
      }
    }
  }

  // ---- watching another device ------------------------------------------------

  async function watchLoop() {
    let frameSeq = -1;
    let resultSeq = -1;
    while (state.watching && !state.gone) {
      try {
        const live = await getLive();
        const fresh = live.age_ms !== null && live.age_ms < STALE_MS;
        $('#note').hidden = fresh;
        $('#note').textContent = 'Waiting for the other device to start its camera…';
        if (fresh && live.frame_seq !== frameSeq) {
          frameSeq = live.frame_seq;
          $('#remote').src = liveFrameUrl(frameSeq);
        }
        if (!fresh) clearVerdict();
        else if (live.result && live.result_seq !== resultSeq) {
          // The first answer may be old, so only later ones are shown.
          if (resultSeq >= 0) showVerdict(live.result);
          resultSeq = live.result_seq;
        } else if (resultSeq < 0) resultSeq = live.result_seq;
      } catch (err) {
        showError(err.message);
      }
      await sleep(WATCH_EVERY_MS);
    }
  }

  $('#start').addEventListener('click', startCamera);
  $('#learn').addEventListener('click', learn);
  $('#inspect').addEventListener('click', () => {
    state.inspecting = !state.inspecting;
    $('#inspect').textContent = state.inspecting ? 'Stop inspecting' : 'Start inspecting';
    if (state.inspecting) inspectLoop();
    else clearVerdict();
  });
  $('#watch').addEventListener('click', () => {
    if (state.stream) {
      showError('This device is filming. Reload the page to watch another one.');
      return;
    }
    state.watching = true;
    video.hidden = true;
    $('#remote').hidden = false;
    $('#guide').classList.add('full');
    $('#watch').disabled = true;
    $('#start').disabled = true;
    watchLoop();
  });
  loadProducts().catch((err) => showError(err.message));

  return () => {
    state.gone = true;
    state.stream?.getTracks().forEach((track) => track.stop());
  };
}
