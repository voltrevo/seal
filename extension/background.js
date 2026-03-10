// Background service worker.
// Manages app state, handles redirects, upgrades tentative → yes.

// App storage shape:
// {
//   apps: {
//     [sealUrl]: {
//       sealUrl, webUrl, manifestUrl, pageTitle,
//       redirect: "no" | "tentative" | "yes" | null,
//       lastSeen: timestamp
//     }
//   }
// }

async function getApps() {
  const data = await chrome.storage.local.get('apps');
  return data.apps || {};
}

async function saveApps(apps) {
  await chrome.storage.local.set({ apps });
}

async function getApp(sealUrl) {
  const apps = await getApps();
  return apps[sealUrl] || null;
}

async function setApp(sealUrl, updates) {
  const apps = await getApps();
  apps[sealUrl] = { ...(apps[sealUrl] || {}), sealUrl, ...updates };
  await saveApps(apps);
  return apps[sealUrl];
}

// Fetch manifest JSON from a URL
async function fetchManifest(manifestUrl) {
  try {
    const resp = await fetch(manifestUrl);
    if (!resp.ok) return null;
    return await resp.json();
  } catch {
    return null;
  }
}

// Handle messages from content script and popup
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'seal-detected') {
    handleDetected(msg, sender);
  } else if (msg.type === 'redirect-to-seal') {
    handleRedirect(msg, sender);
  } else if (msg.type === 'get-apps') {
    getApps().then(sendResponse);
    return true; // async
  } else if (msg.type === 'set-redirect') {
    setApp(msg.sealUrl, { redirect: msg.redirect }).then(sendResponse);
    return true;
  } else if (msg.type === 'remove-app') {
    removeApp(msg.sealUrl).then(sendResponse);
    return true;
  } else if (msg.type === 'get-current-tab-app') {
    getCurrentTabApp().then(sendResponse);
    return true;
  }
});

async function removeApp(sealUrl) {
  const apps = await getApps();
  delete apps[sealUrl];
  await saveApps(apps);
}

async function getCurrentTabApp() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.url) return null;
  const apps = await getApps();
  // Find app whose webUrl matches current tab
  for (const app of Object.values(apps)) {
    if (tab.url.startsWith(app.webUrl)) return app;
  }
  return null;
}

async function handleDetected(msg, sender) {
  const { manifestUrl, pageUrl, pageTitle } = msg;
  const tabId = sender.tab?.id;
  if (!tabId) return;

  // Fetch manifest to get seal_url
  const manifest = await fetchManifest(manifestUrl);
  if (!manifest?.seal_url) return;

  const sealUrl = manifest.seal_url;

  // Derive the web URL base from manifest location
  // manifest is at <webUrl>/.seal/manifest.json
  const webUrl = manifestUrl.replace(/\/?\.seal\/manifest\.json$/, '/');

  // Store/update the app
  const app = await setApp(sealUrl, {
    webUrl,
    manifestUrl,
    pageTitle,
    lastSeen: Date.now(),
  });

  // Set badge
  await chrome.action.setBadgeBackgroundColor({ color: '#6366f1', tabId });
  await chrome.action.setBadgeText({ text: '✓', tabId });

  // Check redirect setting
  if (app.redirect === 'yes') {
    // Auto-redirect
    chrome.tabs.update(tabId, { url: sealUrl });
  } else if (app.redirect === 'tentative') {
    // Redirect and listen for success
    chrome.tabs.update(tabId, { url: sealUrl });
    // We'll upgrade to 'yes' when we detect successful navigation
    // (handled in tab update listener)
  } else {
    // Show banner via content script
    chrome.tabs.sendMessage(tabId, {
      type: 'show-banner',
      sealUrl,
      redirect: app.redirect,
    });
  }
}

async function handleRedirect(msg, sender) {
  const { sealUrl, setRedirect } = msg;
  const tabId = sender.tab?.id;
  if (!tabId) return;

  if (setRedirect) {
    await setApp(sealUrl, { redirect: setRedirect });
  }

  chrome.tabs.update(tabId, { url: sealUrl });
}

// Listen for completed navigations to upgrade tentative → yes
chrome.webNavigation?.onCompleted?.addListener(async (details) => {
  if (details.frameId !== 0) return; // main frame only

  const url = details.url;
  if (!url.includes('.seal')) return;

  // Check if any tentative app matches this URL
  const apps = await getApps();
  for (const app of Object.values(apps)) {
    if (app.redirect === 'tentative' && url.startsWith(app.sealUrl)) {
      await setApp(app.sealUrl, { redirect: 'yes' });
      break;
    }
  }
});
