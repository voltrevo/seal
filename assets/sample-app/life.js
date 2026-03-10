const W = 128, H = 80, S = 5;
const canvas = document.getElementById('c');
canvas.width = W * S; canvas.height = H * S;
const ctx = canvas.getContext('2d');
let grid = Array.from({length: H}, () => new Uint8Array(W));
let running = false, timer = null;

function draw() {
  ctx.fillStyle = '#0a0a1a';
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  for (let y = 0; y < H; y++)
    for (let x = 0; x < W; x++)
      if (grid[y][x]) {
        ctx.fillStyle = `hsl(${(x * 3 + y * 5) % 360}, 70%, 60%)`;
        ctx.fillRect(x * S, y * S, S - 1, S - 1);
      }
}

function step() {
  const next = Array.from({length: H}, () => new Uint8Array(W));
  for (let y = 0; y < H; y++)
    for (let x = 0; x < W; x++) {
      let n = 0;
      for (let dy = -1; dy <= 1; dy++)
        for (let dx = -1; dx <= 1; dx++) {
          if (!dy && !dx) continue;
          const ny = (y + dy + H) % H, nx = (x + dx + W) % W;
          n += grid[ny][nx];
        }
      next[y][x] = n === 3 || (n === 2 && grid[y][x]) ? 1 : 0;
    }
  grid = next;
  draw();
}

function toggle() {
  running = !running;
  document.getElementById('play').textContent = running ? '⏸ Pause' : '▶ Play';
  document.getElementById('play').classList.toggle('active', running);
  if (running) timer = setInterval(step, 80);
  else clearInterval(timer);
}

function randomize() {
  for (let y = 0; y < H; y++)
    for (let x = 0; x < W; x++)
      grid[y][x] = Math.random() < 0.3 ? 1 : 0;
  draw();
}

function clear() {
  grid = Array.from({length: H}, () => new Uint8Array(W));
  draw();
}

function addGliderGun() {
  clear();
  const g = [[1,5],[1,6],[2,5],[2,6],[11,5],[11,6],[11,7],[12,4],[12,8],[13,3],[13,9],[14,3],[14,9],[15,6],[16,4],[16,8],[17,5],[17,6],[17,7],[18,6],[21,3],[21,4],[21,5],[22,3],[22,4],[22,5],[23,2],[23,6],[25,1],[25,2],[25,6],[25,7],[35,3],[35,4],[36,3],[36,4]];
  const ox = 10, oy = 20;
  g.forEach(([x, y]) => { if (y+oy < H && x+ox < W) grid[y+oy][x+ox] = 1; });
  draw();
}

let painting = false;
canvas.addEventListener('mousedown', e => { painting = true; paint(e); });
canvas.addEventListener('mousemove', e => { if (painting) paint(e); });
window.addEventListener('mouseup', () => painting = false);
function paint(e) {
  const r = canvas.getBoundingClientRect();
  const x = Math.floor((e.clientX - r.left) / S);
  const y = Math.floor((e.clientY - r.top) / S);
  if (x >= 0 && x < W && y >= 0 && y < H) { grid[y][x] = 1; draw(); }
}

document.getElementById('play').onclick = toggle;
document.getElementById('step').onclick = step;
document.getElementById('clear').onclick = clear;
document.getElementById('random').onclick = randomize;
document.getElementById('glider').onclick = addGliderGun;

randomize();
