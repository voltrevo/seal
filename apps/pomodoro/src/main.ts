import './style.css'

type Mode = 'focus' | 'short-break' | 'long-break';

const DURATIONS: Record<Mode, number> = {
  'focus': 25 * 60,
  'short-break': 5 * 60,
  'long-break': 15 * 60,
};

const CIRCUMFERENCE = 2 * Math.PI * 108;

let mode: Mode = 'focus';
let timeLeft = DURATIONS[mode];
let running = false;
let interval: number | null = null;
let sessionsCompleted = 0;

function formatTime(s: number): string {
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
}

function render() {
  document.body.className = mode;

  const total = DURATIONS[mode];
  const fraction = timeLeft / total;
  const offset = CIRCUMFERENCE * (1 - fraction);

  const progress = document.querySelector<SVGCircleElement>('.progress')!;
  progress.style.strokeDasharray = `${CIRCUMFERENCE}`;
  progress.style.strokeDashoffset = `${offset}`;

  document.querySelector<HTMLDivElement>('.time')!.textContent = formatTime(timeLeft);

  const playBtn = document.querySelector<HTMLButtonElement>('.play-btn')!;
  playBtn.innerHTML = running
    ? '<svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/></svg>'
    : '<svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><polygon points="8,4 20,12 8,20"/></svg>';

  document.querySelectorAll('.mode-btn').forEach(btn => {
    btn.classList.toggle('active', btn.getAttribute('data-mode') === mode);
  });

  document.querySelectorAll('.dot').forEach((dot, i) => {
    dot.classList.toggle('completed', i < sessionsCompleted);
  });

  const label = document.querySelector<HTMLDivElement>('.session-label')!;
  label.textContent = mode === 'focus' ? 'Focus' : mode === 'short-break' ? 'Short Break' : 'Long Break';

  document.title = `${formatTime(timeLeft)} - Pomodoro`;
}

function tick() {
  if (timeLeft <= 0) {
    stop();
    if (mode === 'focus') {
      sessionsCompleted = Math.min(sessionsCompleted + 1, 4);
      switchMode(sessionsCompleted % 4 === 0 ? 'long-break' : 'short-break');
    } else {
      switchMode('focus');
    }
    try { new Audio('data:audio/wav;base64,UklGRl9vT19teleXBlfm10MDAQABAAEARAAEACAABAAQABAA').play().catch(() => {}); } catch {}
    render();
    return;
  }
  timeLeft--;
  render();
}

function start() {
  if (running) return;
  running = true;
  interval = window.setInterval(tick, 1000);
  render();
}

function stop() {
  running = false;
  if (interval !== null) {
    clearInterval(interval);
    interval = null;
  }
  render();
}

function toggle() {
  running ? stop() : start();
}

function reset() {
  stop();
  timeLeft = DURATIONS[mode];
  render();
}

function switchMode(m: Mode) {
  stop();
  mode = m;
  timeLeft = DURATIONS[mode];
  render();
}

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div class="pomodoro">
    <div class="modes">
      <button class="mode-btn" data-mode="focus">Focus</button>
      <button class="mode-btn" data-mode="short-break">Short Break</button>
      <button class="mode-btn" data-mode="long-break">Long Break</button>
    </div>
    <div class="timer-ring">
      <svg viewBox="0 0 240 240">
        <circle class="track" cx="120" cy="120" r="108"/>
        <circle class="progress" cx="120" cy="120" r="108"/>
      </svg>
      <div class="timer-text">
        <div class="time">25:00</div>
        <div class="session-label">Focus</div>
      </div>
    </div>
    <div class="controls">
      <button class="reset-btn" title="Reset">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/></svg>
      </button>
      <button class="play-btn"></button>
      <div style="width:44px"></div>
    </div>
    <div class="sessions">
      <div class="dot"></div>
      <div class="dot"></div>
      <div class="dot"></div>
      <div class="dot"></div>
    </div>
  </div>
`;

document.querySelector('.modes')!.addEventListener('click', (e) => {
  const btn = (e.target as HTMLElement).closest('.mode-btn') as HTMLElement | null;
  if (!btn) return;
  switchMode(btn.dataset.mode as Mode);
});

document.querySelector('.play-btn')!.addEventListener('click', toggle);
document.querySelector('.reset-btn')!.addEventListener('click', reset);

document.addEventListener('keydown', (e) => {
  if (e.code === 'Space') { e.preventDefault(); toggle(); }
  else if (e.key === 'r' || e.key === 'R') reset();
});

render();
