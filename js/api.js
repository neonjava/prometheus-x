// Demo stand-in for the server API: replays responses recorded from the real Rust server.
const ROOT = 'demo-data';
const STEP_MS = 120;
const NO_MODEL = 'This online demo has no model. Use the sample photos here, or run the project on your computer to check your own.';
const CAMERA_OFF = 'The camera preview works here, but learning and live inspection need the Rust server on your computer.';

const data = fetch(`${ROOT}/data.json`).then((res) => {
  if (!res.ok) throw new Error('Could not load the demo data');
  return res.json();
});

const copy = (value) => structuredClone(value);
const CATEGORY = { Good: 'ok', Uncertain: 'review', Defect: 'defect' };
const decisions = [];
let run; // the batch on show: undefined = full recording, null = deleted

async function current() {
  const d = await data;
  if (run === undefined) run = { names: d.samples.map((s) => s.name), started: 0 };
  return { d, run };
}

function itemsSoFar(d, batch) {
  const items = d.batch.items.filter((i) => batch.names.includes(i.name));
  return { items, done: Math.min(items.length, Math.floor((Date.now() - batch.started) / STEP_MS)) };
}

// A file counts as a sample only if both its name and size match one.
async function sampleNames(files) {
  const { samples } = await data;
  const known = files.every((f) => samples.some((s) => s.name === f.name && s.size === f.size));
  if (!known) throw new Error(NO_MODEL);
  return files.map((f) => f.name);
}

export async function getStatus() {
  const { d } = await current();
  const waiting = (await getPendingReviews()).length;
  return { ...copy(d.status), reviews: decisions.length, waiting_for_review: waiting };
}

export async function getBatches() {
  const { d, run: batch } = await current();
  if (!batch) return [];
  const { items, done } = itemsSoFar(d, batch);
  const counts = { total: items.length, done, ok: 0, review: 0, defect: 0, error: 0 };
  items.slice(0, done).forEach((i) => { counts[CATEGORY[i.verdict] ?? 'error'] += 1; });
  return [{ ...d.batches[0], status: done < items.length ? 'running' : 'done', counts }];
}

export const getDecisions = async () => copy(decisions);

export async function getPendingReviews() {
  const { d, run: batch } = await current();
  if (!batch) return [];
  return d.pending
    .filter((p) => batch.names.includes(p.name) && !decisions.some((x) => x.image === p.image))
    .map((p) => ({ ...p, thumb_url: thumbUrl(p.batch_id, p.name) }));
}

export async function getBatch() {
  const { d, run: batch } = await current();
  if (!batch) throw new Error('This batch was deleted. Reload the page to restore the demo.');
  const { items, done } = itemsSoFar(d, batch);
  const status = done < items.length ? 'running' : 'done';
  return { status, total: items.length, done, error: null, items: copy(items.slice(0, done)) };
}

export async function getItem(id, name) {
  const { items } = await data;
  return copy(items[name]);
}

export async function deleteBatch() {
  run = null;
}

export const thumbUrl = (id, name) => `${ROOT}/thumbs/${encodeURIComponent(name)}`;
export const imageUrl = (id, name) => `${ROOT}/images/${encodeURIComponent(name)}`;
export const csvUrl = () => `${ROOT}/export.csv`;

export async function predictImage(file) {
  const [name] = await sampleNames([file]);
  const { predictions } = await data;
  return copy(predictions[name]);
}

export async function saveDecision(image, verdict, decision) {
  decisions.unshift({ time_unix: Math.floor(Date.now() / 1000), image, verdict, decision });
}

export async function startBatch(files, onProgress) {
  const names = await sampleNames([...files]);
  const { d } = await current();
  onProgress(1);
  run = { names, started: Date.now() };
  decisions.length = 0;
  return { batch_id: d.batches[0].id };
}

// ---- camera mode: frames never leave the browser --------------------------

export const getLive = async () => ({ age_ms: null, frame_seq: 0, result_seq: 0, result: null });
export const liveFrameUrl = () => '';
export const pushFrame = async () => {};
export const predictFrame = async () => { throw new Error(CAMERA_OFF); };
export const learnProduct = async () => { throw new Error(CAMERA_OFF); };
