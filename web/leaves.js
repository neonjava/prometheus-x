// Falling leaves on a full-screen canvas behind the page.
// Same recipe as the ui-designs "GMU ERP glass" brief (react-snowfall with custom leaf images),
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
  const WIND = [-0.4, 1.2];
  const ROTATION = [-1.2, 1.2];
  const DRAW_SCALE = 1.5;
  const FRAME_MS = 1000 / 60;

  const random = ([low, high]) => low + Math.random() * (high - low);

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

  function newLeaf(startAnywhere) {
    return {
      image: images[Math.floor(Math.random() * images.length)],
      size: random(RADIUS) * DRAW_SCALE,
      speed: random(SPEED),
      wind: random(WIND),
      spin: random(ROTATION),
      angle: Math.random() * 360,
      x: Math.random() * canvas.width,
      y: startAnywhere ? Math.random() * canvas.height : -random(RADIUS) * DRAW_SCALE,
    };
  }

  function resize() {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    const wanted = Math.round(
      Math.min(COUNT.max, Math.max(COUNT.min, window.innerWidth / COUNT.perPixel)),
    );
    leaves = Array.from({ length: wanted }, () => newLeaf(true));
  }

  function step(leaf, frames) {
    leaf.y += leaf.speed * frames;
    leaf.x += leaf.wind * frames;
    leaf.angle += leaf.spin * frames;
    if (leaf.y > canvas.height + leaf.size) Object.assign(leaf, newLeaf(false));
    if (leaf.x > canvas.width + leaf.size) leaf.x = -leaf.size;
    if (leaf.x < -leaf.size) leaf.x = canvas.width + leaf.size;
  }

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
    requestAnimationFrame(frame);
  }

  window.addEventListener('resize', resize);
  resize();
  requestAnimationFrame(frame);
})();
