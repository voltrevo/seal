// Content script: detect <meta name="seal-tld-manifest"> and notify background.
// Injects a banner offering redirect if the app isn't set to auto-redirect.

(function () {
  const meta = document.querySelector('meta[name="seal-tld-manifest"]');
  if (!meta) return;

  const manifestHref = meta.getAttribute('content');
  if (!manifestHref) return;

  // Resolve relative manifest URL against current page
  const manifestUrl = new URL(manifestHref, document.location.href).href;
  const pageUrl = document.location.href;
  const pageTitle = document.title || document.location.hostname;

  // Notify background
  chrome.runtime.sendMessage({
    type: 'seal-detected',
    manifestUrl,
    pageUrl,
    pageTitle,
  });

  // Listen for background telling us to show or hide the banner
  chrome.runtime.onMessage.addListener((msg) => {
    if (msg.type === 'show-banner') {
      showBanner(msg.sealUrl, msg.redirect);
    }
  });

  function showBanner(sealUrl, currentRedirect) {
    // Don't show if auto-redirecting
    if (currentRedirect === 'yes' || currentRedirect === 'tentative') return;

    // Don't double-inject
    if (document.getElementById('seal-banner-host')) return;

    const host = document.createElement('div');
    host.id = 'seal-banner-host';
    const shadow = host.attachShadow({ mode: 'closed' });

    shadow.innerHTML = `
      <style>
        :host {
          all: initial;
          display: block;
          position: fixed;
          top: 12px;
          right: 12px;
          z-index: 2147483647;
          font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
        }
        .banner {
          background: #1a1a2e;
          color: #e0e0e0;
          border: 1px solid rgba(99, 102, 241, 0.3);
          border-radius: 12px;
          padding: 16px 20px;
          max-width: 340px;
          box-shadow: 0 8px 32px rgba(0,0,0,0.4);
          animation: slideIn 0.3s ease;
        }
        @keyframes slideIn {
          from { opacity: 0; transform: translateY(-10px); }
          to { opacity: 1; transform: translateY(0); }
        }
        .title {
          font-size: 14px;
          font-weight: 600;
          margin-bottom: 4px;
          color: #fff;
        }
        .subtitle {
          font-size: 12px;
          color: rgba(255,255,255,0.5);
          margin-bottom: 12px;
        }
        .actions {
          display: flex;
          gap: 8px;
          align-items: center;
        }
        .btn {
          border: none;
          border-radius: 8px;
          padding: 8px 16px;
          font-size: 13px;
          font-weight: 500;
          cursor: pointer;
          font-family: inherit;
        }
        .btn-primary {
          background: linear-gradient(135deg, #6366f1, #8b5cf6);
          color: #fff;
        }
        .btn-primary:hover { opacity: 0.9; }
        .btn-secondary {
          background: rgba(255,255,255,0.08);
          color: rgba(255,255,255,0.7);
        }
        .btn-secondary:hover { background: rgba(255,255,255,0.14); }
        .checkbox-row {
          display: flex;
          align-items: center;
          gap: 6px;
          margin-top: 10px;
          font-size: 12px;
          color: rgba(255,255,255,0.5);
        }
        .checkbox-row input { margin: 0; }
        .close {
          position: absolute;
          top: 8px;
          right: 10px;
          background: none;
          border: none;
          color: rgba(255,255,255,0.3);
          font-size: 16px;
          cursor: pointer;
          padding: 4px;
          line-height: 1;
        }
        .close:hover { color: rgba(255,255,255,0.7); }
      </style>
      <div class="banner">
        <button class="close" id="close">&times;</button>
        <div class="title">Seal version available</div>
        <div class="subtitle">${escapeHtml(sealUrl)}</div>
        <div class="actions">
          <button class="btn btn-primary" id="redirect">Open secure version</button>
          <button class="btn btn-secondary" id="dismiss">Dismiss</button>
        </div>
        <label class="checkbox-row">
          <input type="checkbox" id="auto-redirect">
          Redirect automatically next time
        </label>
      </div>
    `;

    document.documentElement.appendChild(host);

    shadow.getElementById('redirect').addEventListener('click', () => {
      const autoRedirect = shadow.getElementById('auto-redirect').checked;
      chrome.runtime.sendMessage({
        type: 'redirect-to-seal',
        sealUrl,
        setRedirect: autoRedirect ? 'tentative' : null,
      });
      host.remove();
    });

    shadow.getElementById('dismiss').addEventListener('click', () => {
      host.remove();
    });

    shadow.getElementById('close').addEventListener('click', () => {
      host.remove();
    });
  }

  function escapeHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }
})();
