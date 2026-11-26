// Falling leaves on a full-screen canvas behind the page. They gather along the bottom,
// and a touch or a passing pointer blows them away like a gust of wind.
// Look and fall speed follow the ui-designs "GMU ERP glass" brief (react-snowfall style),
// written without a library so the page stays dependency-free.
(() => {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;

  const TINTS = [
    'rgba(63, 125, 88, .75)',
    'rgba(47, 96, 69, .55)',
    'rgba(111, 163, 131, .8)',
    'rgba(150, 190, 125, .8)',
    'rgba(86, 140, 100, .6)',
  ];
  const IMAGE_SIZE = 64;
  const LEAF_TILT = Math.PI / 5;
  const COUNT = { min: 12, max: 30, perPixel: 70 };
  const RADIUS = [22, 42];
  const SPEED = [0.7, 1.7];
  const DRIFT = [-0.4, 1.2];
  const ROTATION = [-1.2, 1.2];
  const DRAW_SCALE = 1.5;
  const FRAME_MS = 1000 / 60;
  const PILE = { maxPerPixel: 22, depth: 30 };
  // Sweeping the pointer is a breeze; pressing or tapping is a stronger gust.
  const BREEZE = { radius: 110, push: 2.2, lift: 1.6, carry: 0.12 };
  const GUST = { radius: 190, push: 9, lift: 6, carry: 0 };
  const MAX_SPEED = 16;
  const SETTLE = 0.025;

  const random = ([low, high]) => low + Math.random() * (high - low);
  const clamp = (v, limit) => Math.max(-limit, Math.min(limit, v));

  // One pointed leaf with a thin pale midrib, drawn once per tint.
  function leafImage(color) {
    const canvas = document.createElement('canvas');
    canvas.width = IMAGE_SIZE;
    canvas.height = IMAGE_SIZE;
    const g = canvas.getContext('2d');
    g.translate(IMAGE_SIZE / 2, IMAGE_SIZE / 2);
    g.rotate(LEAF_TILT);
    g.fillStyle = color;
    g.beginPath();
    g.moveTo(0, -27);
    g.quadraticCurveTo(22, -5, 0, 27);
    g.quadraticCurveTo(-22, -5, 0, -27);
    g.fill();
    g.strokeStyle = 'rgba(255, 255, 255, .45)';
    g.lineWidth = 1.2;
    g.beginPath();
    g.moveTo(0, -22);
    g.lineTo(0, 24);
    g.stroke();
    return canvas;
  }

  const canvas = document.createElement('canvas');
  canvas.id = 'leaves';
  document.body.prepend(canvas);
  const ctx = canvas.getContext('2d');
  const images = TINTS.map(leafImage);
  let leaves = [];
  let airborneWanted = 0;

  function newLeaf(startAnywhere) {
    const size = random(RADIUS) * DRAW_SCALE;
    const speed = random(SPEED);
    const drift = random(DRIFT);
    return {
      image: images[Math.floor(Math.random() * images.length)],
      size,
      speed,
      drift,
      vx: drift,
      vy: speed,
      spin: random(ROTATION),
      angle: Math.random() * 360,
      x: Math.random() * canvas.width,
      y: startAnywhere ? Math.random() * canvas.height * 0.8 : -size,
      // How far above the very bottom this leaf comes to rest, so the pile has some depth.
      rest: Math.random() * PILE.depth,
      resting: false,
    };
  }

  const floorFor = (leaf) => canvas.height - leaf.size * 0.28 - leaf.rest;

  function resize() {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    airborneWanted = Math.round(
      Math.min(COUNT.max, Math.max(COUNT.min, window.innerWidth / COUNT.perPixel)),
    );
    if (!leaves.length) leaves = Array.from({ length: airborneWanted }, () => newLeaf(true));
    for (const leaf of leaves) if (leaf.resting) leaf.y = floorFor(leaf);
  }

  function step(leaf, frames) {
    if (leaf.resting) return;
    // After a gust the leaf eases back to its own gentle fall.
    leaf.vx += (leaf.drift - leaf.vx) * SETTLE * frames;
    leaf.vy += (leaf.speed - leaf.vy) * SETTLE * frames;
    leaf.x += leaf.vx * frames;
    leaf.y += leaf.vy * frames;
    leaf.angle += (leaf.spin + leaf.vx * 0.6) * frames;
    if (leaf.x > canvas.width + leaf.size) leaf.x = -leaf.size;
    if (leaf.x < -leaf.size) leaf.x = canvas.width + leaf.size;
    if (leaf.vy > 0 && leaf.y >= floorFor(leaf)) {
      leaf.y = floorFor(leaf);
      leaf.resting = true;
    }
  }

  // Keep new leaves coming; once the bottom is full, the oldest resting leaf makes room.
  function refill() {
    const resting = leaves.filter((l) => l.resting);
    if (leaves.length - resting.length >= airborneWanted) return;
    if (resting.length > canvas.width / PILE.maxPerPixel) {
      leaves.splice(leaves.indexOf(resting[0]), 1);
    }
    leaves.push(newLeaf(false));
  }

  function blow(x, y, moveX, moveY, wind) {
    for (const leaf of leaves) {
      const dx = leaf.x - x;
      const dy = leaf.y - y;
      const distance = Math.hypot(dx, dy) || 1;
      if (distance > wind.radius) continue;
      const force = 1 - distance / wind.radius;
      leaf.vx = clamp(leaf.vx + (dx / distance) * wind.push * force + moveX * wind.carry * force, MAX_SPEED);
      leaf.vy = clamp(leaf.vy + (dy / distance) * wind.push * force * 0.5 - wind.lift * force + moveY * wind.carry * force, MAX_SPEED);
      leaf.spin = clamp(leaf.spin + (Math.random() - 0.5) * 14 * force, 8);
      leaf.resting = false;
    }
  }

  let pointer = null;
  function sweep(x, y) {
    const moveX = pointer ? x - pointer.x : 0;
    const moveY = pointer ? y - pointer.y : 0;
    pointer = { x, y };
    blow(x, y, moveX, moveY, BREEZE);
  }

  // The canvas sits behind the page and ignores clicks, so the window is watched instead.
  window.addEventListener('pointermove', (e) => sweep(e.clientX, e.clientY), { passive: true });
  window.addEventListener('pointerdown', (e) => {
    pointer = { x: e.clientX, y: e.clientY };
    blow(e.clientX, e.clientY, 0, 0, GUST);
  }, { passive: true });
  // While a finger scrolls the page, browsers stop pointer events but still report touches.
  window.addEventListener('touchmove', (e) => sweep(e.touches[0].clientX, e.touches[0].clientY), { passive: true });
  window.addEventListener('touchend', () => { pointer = null; }, { passive: true });
  document.addEventListener('pointerleave', () => { pointer = null; });

  function draw(leaf) {
    ctx.save();
    ctx.translate(leaf.x, leaf.y);
    ctx.rotate((leaf.angle * Math.PI) / 180);
    ctx.drawImage(leaf.image, -leaf.size / 2, -leaf.size / 2, leaf.size, leaf.size);
    ctx.restore();
  }

  let last = performance.now();
  function frame(now) {
    // Scale movement by elapsed time so speed is the same on any refresh rate.
    const frames = Math.min((now - last) / FRAME_MS, 4);
    last = now;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    for (const leaf of leaves) {
      step(leaf, frames);
      draw(leaf);
    }
    refill();
    requestAnimationFrame(frame);
  }

  window.addEventListener('resize', resize);
  resize();
  requestAnimationFrame(frame);
})();
