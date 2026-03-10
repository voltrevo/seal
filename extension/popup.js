// Popup: shows current tab's app (if any) and all known apps.

const currentEl = document.getElementById('current');
const appsEl = document.getElementById('apps');

async function load() {
  const [currentApp, apps] = await Promise.all([
    send({ type: 'get-current-tab-app' }),
    send({ type: 'get-apps' }),
  ]);

  renderCurrent(currentApp);
  renderApps(apps, currentApp?.sealUrl);
}

function renderCurrent(app) {
  if (!app) {
    currentEl.innerHTML = '';
    return;
  }

  currentEl.innerHTML = `
    <div class="current-app">
      <div class="label">This page</div>
      <div class="app-row" style="padding:0;">
        <div class="app-info">
          <div class="app-name">${esc(app.pageTitle || app.sealUrl)}</div>
          <div class="app-url">${esc(app.sealUrl)}</div>
        </div>
        <button class="go-btn" data-seal-url="${esc(app.sealUrl)}">Go</button>
        ${redirectToggle(app)}
      </div>
    </div>
  `;

  bindToggle(currentEl, app.sealUrl);
  bindGoBtn(currentEl);
}

function renderApps(apps, currentSealUrl) {
  const entries = Object.values(apps)
    .filter(a => a.sealUrl !== currentSealUrl)
    .sort((a, b) => (b.lastSeen || 0) - (a.lastSeen || 0));

  if (entries.length === 0 && !currentSealUrl) {
    appsEl.innerHTML = '<div class="empty">No Seal apps discovered yet.<br>Browse the web — apps with Seal support will appear here.</div>';
    return;
  }

  if (entries.length === 0) {
    appsEl.innerHTML = '';
    return;
  }

  let html = '<div class="section-label">All apps</div>';
  for (const app of entries) {
    html += `
      <div class="app-row">
        <div class="app-info">
          <div class="app-name">${esc(app.pageTitle || app.sealUrl)}</div>
          <div class="app-url">${esc(app.sealUrl)}</div>
        </div>
        <button class="go-btn" data-seal-url="${esc(app.sealUrl)}">Go</button>
        ${redirectToggle(app)}
      </div>
    `;
  }
  appsEl.innerHTML = html;

  for (const app of entries) {
    bindToggle(appsEl, app.sealUrl);
  }
  bindGoBtn(appsEl);
}

function redirectToggle(app) {
  const r = app.redirect || 'unset';
  return `
    <div class="redirect-toggle" data-seal-url="${esc(app.sealUrl)}">
      <button data-val="no" class="${r === 'no' ? 'active-no' : ''}" title="Never redirect">No</button>
      <button data-val="tentative" class="${r === 'tentative' ? 'active-tentative' : ''}" title="Redirect, confirm on success">Try</button>
      <button data-val="yes" class="${r === 'yes' ? 'active-yes' : ''}" title="Always redirect">Yes</button>
    </div>
  `;
}

function bindToggle(container, sealUrl) {
  const toggle = container.querySelector(`.redirect-toggle[data-seal-url="${CSS.escape(sealUrl)}"]`);
  if (!toggle) return;

  toggle.addEventListener('click', async (e) => {
    const btn = e.target.closest('button');
    if (!btn) return;

    const val = btn.dataset.val;
    await send({ type: 'set-redirect', sealUrl, redirect: val });

    // Update button states
    for (const b of toggle.querySelectorAll('button')) {
      b.className = b.dataset.val === val ? `active-${val}` : '';
    }
  });
}

function bindGoBtn(container) {
  for (const btn of container.querySelectorAll('.go-btn')) {
    btn.addEventListener('click', async () => {
      const sealUrl = btn.dataset.sealUrl;
      const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
      if (tab) {
        chrome.tabs.update(tab.id, { url: sealUrl });
        window.close();
      }
    });
  }
}

function send(msg) {
  return chrome.runtime.sendMessage(msg);
}

function esc(s) {
  const d = document.createElement('div');
  d.textContent = s || '';
  return d.innerHTML;
}

load();
