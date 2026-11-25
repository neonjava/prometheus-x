// Every call to the server lives here.

async function parse(res) {
  if (!res.ok) throw new Error(`${res.status}: ${(await res.text()) || res.statusText}`);
  return res.json();
}

export const getStatus = () => fetch('/status').then(parse);
export const getBatches = () => fetch('/batches').then(parse);
export const getDecisions = () => fetch('/decisions').then(parse);
export const getPendingReviews = () => fetch('/review/pending').then(parse);
export const getBatch = (id) => fetch(`/batch/${encodeURIComponent(id)}`).then(parse);
export const getItem = (id, name) =>
  fetch(`/batch/${encodeURIComponent(id)}/item/${encodeURIComponent(name)}`).then(parse);

export const thumbUrl = (id, name) => `/batch/${encodeURIComponent(id)}/thumb/${encodeURIComponent(name)}`;
export const imageUrl = (id, name) => `/batch/${encodeURIComponent(id)}/image/${encodeURIComponent(name)}`;
export const csvUrl = (id) => `/batch/${encodeURIComponent(id)}/export.csv`;

export function predictImage(file) {
  const data = new FormData();
  data.append('image', file);
  return fetch('/predict', { method: 'POST', body: data }).then(parse);
}

export async function saveDecision(image, verdict, decision) {
  const res = await fetch('/decision', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ image, verdict, decision }),
  });
  if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
}

// XMLHttpRequest instead of fetch so a big zip can show upload progress.
export function startBatch(files, onProgress) {
  return new Promise((resolve, reject) => {
    const data = new FormData();
    for (const file of files) data.append('image', file, file.name);
    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/batch');
    xhr.upload.onprogress = (e) => e.lengthComputable && onProgress(e.loaded / e.total);
    xhr.onerror = () => reject(new Error('Could not reach the server'));
    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) resolve(JSON.parse(xhr.responseText));
      else reject(new Error(`${xhr.status}: ${xhr.responseText}`));
    };
    xhr.send(data);
  });
}
