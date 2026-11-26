// Demo-only extras: sample photos on the upload pages and a note on the camera page.
const view = document.getElementById('view');
const samples = fetch('demo-data/data.json').then((r) => r.json()).then((d) => d.samples.map((s) => s.name));
const label = (name) => (name.includes('-good-') ? 'good board' : 'damaged board');

async function choose(input, names) {
  const files = await Promise.all(names.map(async (name) => {
    const blob = await (await fetch(`demo-data/images/${name}`)).blob();
    return new File([blob], name, { type: 'image/jpeg' });
  }));
  const transfer = new DataTransfer();
  files.forEach((file) => transfer.items.add(file));
  input.files = transfer.files;
  input.dispatchEvent(new Event('change'));
}

function panel(html) {
  const node = document.createElement('div');
  node.className = 'demo-samples';
  node.innerHTML = html;
  return node;
}

async function addSamples(zone) {
  const input = zone.parentElement.querySelector('input[type="file"]');
  const names = await samples;
  const box = input.multiple
    ? panel(`<button type="button" class="btn" data-all>Use the ${names.length} sample photos</button>`)
    : panel(`<p class="small muted">Try a sample photo, then press "Check image":</p><div class="demo-strip">${names
      .map((n) => `<button type="button" data-name="${n}" title="${label(n)}"><img loading="lazy" src="demo-data/thumbs/${n}" alt="Sample ${label(n)}"></button>`)
      .join('')}</div>`);
  box.addEventListener('click', async (e) => {
    const button = e.target.closest('button');
    if (!button) return;
    const all = 'all' in button.dataset;
    const text = button.textContent;
    button.disabled = true;
    if (all) button.textContent = 'Loading photos…';
    try {
      await choose(input, all ? names : [button.dataset.name]);
    } finally {
      button.disabled = false;
      if (all) button.textContent = text;
    }
  });
  // Below the check button, so the button stays in view.
  (zone.parentElement.querySelector('#go') ?? input).after(box);
}

new MutationObserver(() => {
  const zone = view.querySelector('#zone:not([data-demo])');
  if (zone) {
    zone.dataset.demo = '1';
    addSamples(zone);
  }
  const stage = view.querySelector('#stage:not([data-demo])');
  if (stage) {
    stage.dataset.demo = '1';
    stage.before(panel('<p class="notice">Demo: the camera preview runs only in your browser. Teaching and inspecting need the Rust server.</p>'));
  }
}).observe(view, { childList: true, subtree: true });
