// Fullscreen without the browser's own. The window is already fullscreen, so a player asking
// for it only needs its element laid over the page: in the top layer (as a popover), as the
// real thing does. The real thing brings up the browser's "press Esc to exit" bubble each time.
(() => {
  if (window.__tvlFullscreen) return;
  window.__tvlFullscreen = true;
  // Frames keep the real one: a frame can't lay itself over the page that holds it.
  if (window !== window.top || !HTMLElement.prototype.showPopover) return;

  const ATTR = 'data-tvl-fullscreen';
  const CSS = `
    [${ATTR}] {
      position: fixed !important; inset: 0 !important; margin: 0 !important;
      box-sizing: border-box !important; transform: none !important;
      min-width: 0 !important; max-width: none !important; width: 100% !important;
      min-height: 0 !important; max-height: none !important; height: 100% !important;
      object-fit: contain;
    }
    /* Under the site's own rules, over the popover's look. */
    :where([${ATTR}]) {
      border: 0; padding: 0; overflow: visible; color: inherit; background: transparent;
    }
    [${ATTR}]::backdrop { background: #000; }
  `;
  const real = {
    request: Element.prototype.requestFullscreen,
    exit: Document.prototype.exitFullscreen,
  };
  let current = null;
  let watcher = null;

  function addStyle() {
    if (document.getElementById('__tvl-fullscreen')) return;
    const s = document.createElement('style');
    s.id = '__tvl-fullscreen';
    s.textContent = CSS;
    (document.head || document.documentElement).appendChild(s);
  }

  function changed(el) {
    for (const type of ['fullscreenchange', 'webkitfullscreenchange']) {
      const target = el && el.isConnected ? el : document;
      target.dispatchEvent(new Event(type, { bubbles: true, composed: true }));
    }
  }

  function release(el) {
    try {
      el.hidePopover();
    } catch {}
    el.removeAttribute('popover');
    el.removeAttribute(ATTR);
  }

  function enter(el) {
    if (el === current) return true;
    // A popover of the site's own is left to the real thing.
    if (!(el instanceof HTMLElement) || !el.isConnected || el.hasAttribute('popover')) return false;
    addStyle();
    el.setAttribute('popover', 'manual');
    el.setAttribute(ATTR, '');
    try {
      el.showPopover();
    } catch {
      release(el);
      return false;
    }
    const before = current;
    current = el;
    if (before) {
      release(before);
      changed(before);
    }
    // Taken out of the page: out of fullscreen, as with the real thing.
    watcher ||= new MutationObserver(() => {
      if (current && !current.isConnected) leave();
    });
    watcher.observe(document, { childList: true, subtree: true });
    changed(el);
    return true;
  }

  function leave() {
    const el = current;
    if (!el) return;
    current = null;
    watcher?.disconnect();
    release(el);
    changed(el);
  }

  function request() {
    if (enter(this)) return Promise.resolve();
    return real.request ? real.request.apply(this, arguments) : Promise.reject(new TypeError('fullscreen'));
  }

  function exit() {
    if (current) {
      leave();
      return Promise.resolve();
    }
    return real.exit && document.fullscreenElement ? real.exit.call(this) : Promise.resolve();
  }

  // The fullscreen element as seen from `root`: its shadow host there, if it's in a shadow tree.
  function seenFrom(root) {
    let el = current;
    while (el && el.getRootNode() !== root) el = el.getRootNode().host;
    return el || null;
  }

  const native = Object.getOwnPropertyDescriptor(Document.prototype, 'fullscreenElement');
  function fullscreenElement() {
    return current ? seenFrom(this) : native?.get?.call(this) ?? null;
  }

  for (const name of ['requestFullscreen', 'webkitRequestFullscreen', 'webkitRequestFullScreen']) {
    Element.prototype[name] = request;
  }
  for (const name of ['exitFullscreen', 'webkitExitFullscreen', 'webkitCancelFullScreen']) {
    Document.prototype[name] = exit;
  }
  for (const name of ['fullscreenElement', 'webkitFullscreenElement', 'webkitCurrentFullScreenElement']) {
    Object.defineProperty(Document.prototype, name, { configurable: true, get: fullscreenElement });
  }
  for (const name of ['fullscreen', 'webkitIsFullScreen']) {
    Object.defineProperty(Document.prototype, name, {
      configurable: true,
      get() {
        return !!fullscreenElement.call(this);
      },
    });
  }
  Object.defineProperty(ShadowRoot.prototype, 'fullscreenElement', {
    configurable: true,
    get() {
      return current ? seenFrom(this) : null;
    },
  });

  // Esc leaves it, as with the real thing (and, as there, the page doesn't see that Esc).
  addEventListener(
    'keydown',
    (e) => {
      if (e.key !== 'Escape' || !current) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      leave();
    },
    true,
  );
})();
