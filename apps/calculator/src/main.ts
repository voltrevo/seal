import './style.css'

type Op = '+' | '-' | '×' | '÷' | null;

let current = '0';
let previous = '';
let operator: Op = null;
let shouldReset = false;

function render() {
  const expr = document.querySelector<HTMLDivElement>('.expression')!;
  const val = document.querySelector<HTMLDivElement>('.value')!;
  expr.textContent = previous + (operator ? ` ${operator}` : '');
  val.textContent = current;
}

function inputDigit(d: string) {
  if (shouldReset) {
    current = d;
    shouldReset = false;
  } else {
    current = current === '0' ? d : current + d;
  }
}

function inputDecimal() {
  if (shouldReset) {
    current = '0.';
    shouldReset = false;
    return;
  }
  if (!current.includes('.')) {
    current += '.';
  }
}

function calculate(a: number, b: number, op: Op): number {
  switch (op) {
    case '+': return a + b;
    case '-': return a - b;
    case '×': return a * b;
    case '÷': return b === 0 ? NaN : a / b;
    default: return b;
  }
}

function formatNumber(n: number): string {
  if (isNaN(n)) return 'Error';
  if (!isFinite(n)) return 'Error';
  const s = String(n);
  if (s.length > 12) {
    return Number(n.toPrecision(10)).toString();
  }
  return s;
}

function handleOperator(nextOp: Op) {
  const val = parseFloat(current);
  if (operator && !shouldReset) {
    const prev = parseFloat(previous);
    const result = calculate(prev, val, operator);
    current = formatNumber(result);
    previous = current;
  } else {
    previous = current;
  }
  operator = nextOp;
  shouldReset = true;
}

function handleEquals() {
  if (!operator) return;
  const prev = parseFloat(previous);
  const val = parseFloat(current);
  const result = calculate(prev, val, operator);
  current = formatNumber(result);
  previous = '';
  operator = null;
  shouldReset = true;
}

function handleClear() {
  current = '0';
  previous = '';
  operator = null;
  shouldReset = false;
}

function handleToggleSign() {
  if (current !== '0') {
    current = current.startsWith('-') ? current.slice(1) : '-' + current;
  }
}

function handlePercent() {
  current = formatNumber(parseFloat(current) / 100);
}

function handleBackspace() {
  if (shouldReset) return;
  current = current.length > 1 ? current.slice(0, -1) : '0';
}

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <div class="calculator">
    <div class="display">
      <div class="expression"></div>
      <div class="value">0</div>
    </div>
    <div class="buttons">
      <button class="btn function" data-action="clear">AC</button>
      <button class="btn function" data-action="sign">+/−</button>
      <button class="btn function" data-action="percent">%</button>
      <button class="btn operator" data-action="op" data-op="÷">÷</button>
      <button class="btn number" data-digit="7">7</button>
      <button class="btn number" data-digit="8">8</button>
      <button class="btn number" data-digit="9">9</button>
      <button class="btn operator" data-action="op" data-op="×">×</button>
      <button class="btn number" data-digit="4">4</button>
      <button class="btn number" data-digit="5">5</button>
      <button class="btn number" data-digit="6">6</button>
      <button class="btn operator" data-action="op" data-op="-">−</button>
      <button class="btn number" data-digit="1">1</button>
      <button class="btn number" data-digit="2">2</button>
      <button class="btn number" data-digit="3">3</button>
      <button class="btn operator" data-action="op" data-op="+">+</button>
      <button class="btn number wide" data-digit="0">0</button>
      <button class="btn number" data-action="decimal">.</button>
      <button class="btn equals" data-action="equals">=</button>
    </div>
  </div>
`;

document.querySelector('.buttons')!.addEventListener('click', (e) => {
  const btn = (e.target as HTMLElement).closest('.btn') as HTMLElement | null;
  if (!btn) return;

  const digit = btn.dataset.digit;
  const action = btn.dataset.action;

  if (digit) inputDigit(digit);
  else if (action === 'op') handleOperator(btn.dataset.op as Op);
  else if (action === 'equals') handleEquals();
  else if (action === 'clear') handleClear();
  else if (action === 'sign') handleToggleSign();
  else if (action === 'percent') handlePercent();
  else if (action === 'decimal') inputDecimal();

  render();
});

document.addEventListener('keydown', (e) => {
  if (e.key >= '0' && e.key <= '9') inputDigit(e.key);
  else if (e.key === '.') inputDecimal();
  else if (e.key === '+') handleOperator('+');
  else if (e.key === '-') handleOperator('-');
  else if (e.key === '*') handleOperator('×');
  else if (e.key === '/') { e.preventDefault(); handleOperator('÷'); }
  else if (e.key === 'Enter' || e.key === '=') handleEquals();
  else if (e.key === 'Escape') handleClear();
  else if (e.key === 'Backspace') handleBackspace();
  else if (e.key === '%') handlePercent();
  else return;
  render();
});

render();
