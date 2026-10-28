// TV mode: the D-pad moves a highlight between a site's links and buttons.
// The launcher calls window.__tvnav.press(name) through the DevTools protocol (cdp.rs) and
// acts on the answer: true = handled here, a key name = the launcher presses that key for
// real, false = the launcher presses the button's usual key.
(() => {
  if (window.top !== window || window.__tvnav) return;

  const SELECTOR = [
    'a[href]', 'button', 'input:not([type=hidden])', 'select', 'textarea', 'summary',
    '[role=button]', '[role=link]', '[role=tab]', '[role=menuitem]', '[role=option]',
    '[role=checkbox]', '[role=switch]', '[role=radio]', '[tabindex]:not([tabindex="-1"])',
  ].join(',');
  const DIALOGS = '[aria-modal="true"],dialog[open],[role=dialog],[role=alertdialog]';
  // Player buttons that A presses while watching: skip intro/recap, next episode.
  const SKIP = /^(skip|pular|ignorar|next episode|pr[oó]ximo epis[oó]dio|assistir pr[oó]ximo|watch next)/i;
  const NEXT = /next|seguinte|pr[oó]xim|avan[cç]ar|right|direita|see more|ver mais/i;
  const PREV = /prev|anterior|left|esquerda/i;
  const CONTROLS_IDLE_MS = 6000;
  const MIN_SIZE = 6;
  // The launcher's own player controls (player/overlay.js), on sites it has an adapter for.
  const overlay = window.__tvplayer || null;

  let current = null;
  let ringShown = false;
  let ring = null;
  // While watching, Up/Down open the player's controls to the D-pad until this time.
  let controlsUntil = 0;
  // Last time a real mouse (the right stick) moved.
  let mouseAt = 0;
  // A press woke a player's controls: the launcher also moves the mouse there for real (some
  // players ignore made-up moves), and that move isn't someone using the mouse.
  let moveTo = null;
  let launcherMoveUntil = 0;
  let controlsWereUp = false;

  const vw = () => window.innerWidth;
  const vh = () => window.innerHeight;
  const rectOf = (el) => el.getBoundingClientRect();
  const centerOf = (r) => ({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  const onScreen = (r) => r.bottom > 0 && r.top < vh() && r.right > 0 && r.left < vw();
  const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);

  // ---------- finding things, shadow DOM included ----------

  function deepQuery(root, selector, out = []) {
    root.querySelectorAll(selector).forEach((el) => out.push(el));
    root.querySelectorAll('*').forEach((el) => {
      if (el.shadowRoot) deepQuery(el.shadowRoot, selector, out);
    });
    return out;
  }

  function deepHit(x, y) {
    let hit = document.elementFromPoint(x, y);
    while (hit && hit.shadowRoot) {
      const inner = hit.shadowRoot.elementFromPoint(x, y);
      if (!inner || inner === hit) break;
      hit = inner;
    }
    return hit;
  }

  function deepActive() {
    let a = document.activeElement;
    while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement;
    return a;
  }

  // `node` is `el` or inside it, across shadow roots.
  function holds(el, node) {
    for (let n = node; n; n = n.parentNode || n.host) if (n === el) return true;
    return false;
  }

  function shown(el) {
    const s = getComputedStyle(el);
    return (
      s.visibility !== 'hidden' &&
      s.display !== 'none' &&
      Number(s.opacity) > 0.05 &&
      !el.disabled &&
      !el.closest('[aria-hidden="true"],[inert]')
    );
  }

  // On screen right now, faded-out ancestors included: players hide their controls by
  // fading the bar that holds them, not each button.
  function visibleNow(el) {
    if (!onScreen(rectOf(el))) return false;
    let opacity = 1;
    for (let n = el; n && n.nodeType === 1; n = n.parentElement || n.getRootNode().host) {
      const s = getComputedStyle(n);
      // Visibility is inherited, and a child can show inside a hidden parent: only its own counts.
      if (s.display === 'none' || (n === el && s.visibility === 'hidden')) return false;
      opacity *= Number(s.opacity);
      if (opacity < 0.1) return false;
    }
    return true;
  }

  // Visible and not covered. Elements above or below the screen are judged when scrolled to.
  function reachable(el, r) {
    if (!shown(el)) return false;
    if (!onScreen(r)) return true;
    const c = centerOf(r);
    const hit = deepHit(clamp(c.x, 1, vw() - 1), clamp(c.y, 1, vh() - 1));
    return !!hit && (holds(el, hit) || holds(hit, el));
  }

  // The open dialog, if any: navigation stays inside it.
  function dialog() {
    const area = vw() * vh() * 0.08;
    const open = deepQuery(document, DIALOGS).filter((d) => {
      const r = rectOf(d);
      return r.width * r.height > area && shown(d);
    });
    return open[open.length - 1] || null;
  }

  function candidates() {
    const out = [];
    for (const el of deepQuery(dialog() || document, SELECTOR)) {
      const r = rectOf(el);
      if (r.width < MIN_SIZE || r.height < MIN_SIZE) continue;
      // Sideways off screen is clipped carousel content: reached by scrolling the row.
      if (r.right <= 0 || r.left >= vw() || r.bottom < -3 * vh() || r.top > 4 * vh()) continue;
      // The same box as its clickable parent (a button inside a link): keep only the parent.
      const p = el.parentElement && el.parentElement.closest(SELECTOR);
      if (p) {
        const pr = rectOf(p);
        if (pr.width >= MIN_SIZE && pr.height >= MIN_SIZE && pr.width * pr.height <= r.width * r.height * 1.3) continue;
      }
      out.push({ el, r });
    }
    return out;
  }

  function valid(el) {
    if (!el || !el.isConnected || !shown(el)) return false;
    const d = dialog();
    return !d || holds(d, el);
  }

  function nearest(x, y) {
    const list = candidates()
      .filter((c) => onScreen(c.r))
      .map((c) => {
        const p = centerOf(c.r);
        return { ...c, d: Math.hypot(p.x - x, p.y - y) };
      })
      .sort((a, b) => a.d - b.d);
    const found = list.find((c) => reachable(c.el, c.r));
    return found ? found.el : null;
  }

  // ---------- the highlight ----------

  // In a player with the launcher's overlay, the highlighted element lights up itself
  // (.tvl-focused: brighter and larger, like the launcher's own buttons) instead of the ring.
  let lit = null;
  function light(el) {
    if (lit === el) return;
    if (lit) lit.classList.remove('tvl-focused');
    lit = el;
    if (el) el.classList.add('tvl-focused');
  }

  function drawRing() {
    if (!ring) {
      ring = document.createElement('div');
      ring.setAttribute('aria-hidden', 'true');
      Object.assign(ring.style, {
        position: 'fixed',
        zIndex: '2147483647',
        pointerEvents: 'none',
        boxSizing: 'border-box',
        margin: '0',
        border: '4px solid #fff',
        borderRadius: '10px',
        boxShadow: '0 0 0 3px rgba(0,0,0,.55), 0 0 28px 6px rgba(255,255,255,.45)',
        transition: 'left .12s ease-out, top .12s ease-out, width .12s ease-out, height .12s ease-out',
      });
    }
    // Inside the fullscreen element, or it would be hidden behind it.
    const host = document.fullscreenElement || document.documentElement;
    if (host && ring.parentNode !== host) host.appendChild(ring);
    const visible = ringShown && current && current.isConnected && visibleNow(current);
    // The page's .tvl-focused style can't reach into shadow roots: what's in one keeps the ring.
    const inPlayer = visible && overlay && overlay.watching() && current.getRootNode() === document;
    light(inPlayer ? current : null);
    if (visible && !inPlayer) {
      const r = rectOf(current);
      Object.assign(ring.style, {
        display: 'block',
        left: `${r.left - 4}px`,
        top: `${r.top - 4}px`,
        width: `${r.width + 8}px`,
        height: `${r.height + 8}px`,
      });
    } else {
      ring.style.display = 'none';
    }
  }

  function frame() {
    try {
      const now = Date.now();
      const up = now < controlsUntil;
      // The player's controls just went away (Back or idle): so does the highlight on them.
      // A skip button showing gets it back from the check below.
      if (controlsWereUp && !up) ringShown = false;
      controlsWereUp = up;
      if (overlay) overlay.frame(up, now - mouseAt < CONTROLS_IDLE_MS);
    } catch (e) {
      console.warn('tvplayer', e);
    }
    try {
      drawRing();
    } catch (_) {
      // The page swapped its document out; try again next frame.
    }
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  // A real mouse (the right stick) takes over: hide the highlight until the D-pad is used again.
  window.addEventListener(
    'mousemove',
    (e) => {
      if (Date.now() < launcherMoveUntil) return;
      if (e.isTrusted && Math.abs(e.movementX) + Math.abs(e.movementY) > 2) {
        ringShown = false;
        mouseAt = Date.now();
      }
    },
    true,
  );

  function show(el, scroll = true) {
    current = el;
    ringShown = !!el;
    if (!el) return;
    try {
      el.focus({ preventScroll: true });
    } catch (_) {}
    if (!scroll) return;
    const r = rectOf(el);
    if (r.top < vh() * 0.15 || r.bottom > vh() * 0.9) {
      el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'smooth' });
    } else if (r.left < 0 || r.right > vw()) {
      el.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' });
    }
  }

  function pick() {
    const a = deepActive();
    if (a && a !== document.body && a.matches && a.matches(SELECTOR) && valid(a) && onScreen(rectOf(a))) {
      show(a);
    } else {
      show(nearest(vw() * 0.35, vh() * 0.4));
    }
  }

  // ---------- moving ----------

  function best(dir) {
    const from = rectOf(current);
    const fc = centerOf(from);
    const across = dir === 'left' || dir === 'right';
    const scored = [];
    for (const c of candidates()) {
      if (c.el === current || holds(c.el, current) || holds(current, c.el)) continue;
      const r = c.r;
      const p = centerOf(r);
      let primary, ahead, gap, offset, size;
      if (across) {
        primary = dir === 'right' ? r.left - from.right : from.left - r.right;
        ahead = dir === 'right' ? p.x - fc.x : fc.x - p.x;
        gap = Math.max(0, Math.max(r.top, from.top) - Math.min(r.bottom, from.bottom));
        offset = Math.abs(p.y - fc.y);
        size = Math.min(r.width, from.width);
      } else {
        primary = dir === 'down' ? r.top - from.bottom : from.top - r.bottom;
        ahead = dir === 'down' ? p.y - fc.y : fc.y - p.y;
        gap = Math.max(0, Math.max(r.left, from.left) - Math.min(r.right, from.right));
        offset = Math.abs(p.x - fc.x);
        size = Math.min(r.height, from.height);
      }
      if (ahead <= 1 || primary < -size / 2) continue;
      scored.push({ ...c, s: Math.max(primary, 0) + gap * 3 + offset * 0.5 });
    }
    scored.sort((a, b) => a.s - b.s);
    const found = scored.find((c) => reachable(c.el, c.r));
    return found ? found.el : null;
  }

  function scrollerOf(el, axis) {
    for (let n = el && el.parentElement; n && n !== document.body && n !== document.documentElement; n = n.parentElement) {
      const s = getComputedStyle(n);
      const overflow = axis === 'x' ? s.overflowX : s.overflowY;
      const more = axis === 'x' ? n.scrollWidth > n.clientWidth + 4 : n.scrollHeight > n.clientHeight + 4;
      if (more && /(auto|scroll|hidden)/.test(overflow)) return n;
    }
    return null;
  }

  function scrollPage(dy, smooth) {
    const target = scrollerOf(current, 'y');
    const opts = { top: dy, behavior: smooth ? 'smooth' : 'auto' };
    if (target) target.scrollBy(opts);
    else window.scrollBy(opts);
  }

  // End of the visible part of a row: scroll the row, or press its own arrow button.
  function scrollRow(sign) {
    const row = scrollerOf(current, 'x');
    if (row) {
      const before = row.scrollLeft;
      row.scrollBy({ left: sign * row.clientWidth * 0.8 });
      if (row.scrollLeft !== before) return true;
    }
    const r = rectOf(current);
    const re = sign > 0 ? NEXT : PREV;
    const arrow = deepQuery(document, SELECTOR).find((el) => {
      const label = `${el.getAttribute('aria-label') || ''} ${typeof el.className === 'string' ? el.className : ''}`;
      if (!re.test(label)) return false;
      const a = rectOf(el);
      return a.width > 0 && a.top < r.bottom && a.bottom > r.top && (sign > 0 ? a.left >= r.left : a.right <= r.right);
    });
    if (!arrow) return false;
    activate(arrow);
    return true;
  }

  function navigate(dir) {
    if (!valid(current)) {
      pick();
      return true;
    }
    if (!ringShown) {
      ringShown = true;
      return true;
    }
    const next = best(dir);
    if (next) {
      show(next);
      return true;
    }
    if (dir === 'up' || dir === 'down') {
      // Nothing that way yet: scroll, which also makes lazy pages load more.
      scrollPage((dir === 'down' ? 1 : -1) * vh() * 0.6, true);
      return true;
    }
    if (scrollRow(dir === 'right' ? 1 : -1)) {
      setTimeout(() => {
        const after = valid(current) && best(dir);
        if (after) show(after);
      }, 450);
      return true;
    }
    return false;
  }

  // LB / RB: a screen up or down, keeping the highlight at the same height.
  function page(sign) {
    const at = valid(current) ? centerOf(rectOf(current)) : { x: vw() / 2, y: vh() / 2 };
    scrollPage(sign * vh() * 0.8, false);
    show(nearest(at.x, clamp(at.y, 0, vh())), false);
  }

  // ---------- pressing ----------

  function activate(el) {
    const tag = el.tagName;
    const type = (el.getAttribute('type') || '').toLowerCase();
    const textField =
      tag === 'TEXTAREA' || (tag === 'INPUT' && !['button', 'submit', 'reset', 'checkbox', 'radio', 'image'].includes(type));
    if (textField || tag === 'SELECT') {
      el.focus();
      return true;
    }
    // A focused link or button: a real Enter, which every site handles like a click.
    if ((tag === 'A' || tag === 'BUTTON') && deepActive() === el) return 'Enter';
    const c = centerOf(rectOf(el));
    const opts = { bubbles: true, cancelable: true, composed: true, view: window, clientX: c.x, clientY: c.y, button: 0 };
    const pointer = { ...opts, pointerId: 1, pointerType: 'mouse', isPrimary: true };
    el.dispatchEvent(new PointerEvent('pointerdown', pointer));
    el.dispatchEvent(new MouseEvent('mousedown', opts));
    el.dispatchEvent(new PointerEvent('pointerup', pointer));
    el.dispatchEvent(new MouseEvent('mouseup', opts));
    el.click();
    return true;
  }

  function back() {
    const d = dialog();
    // Played from the launcher's copy of the service: its own pages are never the way back.
    if (window.__tvlWatch && !d) return 'close';
    if (document.fullscreenElement) {
      document.exitFullscreen();
      return true;
    }
    if (d) {
      const close = deepQuery(d, SELECTOR).find((el) =>
        /close|fechar|dismiss|cancel/i.test(`${el.getAttribute('aria-label') || ''} ${el.getAttribute('data-uia') || ''}`),
      );
      if (close) {
        activate(close);
        current = null;
        return true;
      }
      return 'Escape';
    }
    if (history.length > 1) {
      current = null;
      history.back();
      return true;
    }
    return 'Escape';
  }

  // ---------- the video player ----------

  // The video filling most of the screen, if one is.
  function player() {
    let found = null;
    let most = 0;
    for (const v of deepQuery(document, 'video')) {
      const r = rectOf(v);
      const w = Math.max(0, Math.min(r.right, vw()) - Math.max(r.left, 0));
      const h = Math.max(0, Math.min(r.bottom, vh()) - Math.max(r.top, 0));
      if (w * h > most && v.readyState > 0) {
        found = v;
        most = w * h;
      }
    }
    return most > vw() * vh() * 0.5 ? found : null;
  }

  // Players show their controls on mouse movement.
  function wake(v) {
    const r = rectOf(v);
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2 + (Date.now() % 2);
    moveTo = { x: Math.round(x), y: Math.round(y) };
    const opts = { bubbles: true, composed: true, view: window, clientX: x, clientY: y };
    for (const target of new Set([deepHit(x, y), v])) {
      if (!target) continue;
      target.dispatchEvent(new PointerEvent('pointermove', { ...opts, pointerType: 'mouse' }));
      target.dispatchEvent(new MouseEvent('mousemove', opts));
    }
  }

  function skipButton() {
    return (
      candidates().find(({ el, r }) => {
        if (!onScreen(r) || (overlay && overlay.owns(el))) return false;
        const label = (el.getAttribute('aria-label') || el.textContent || '').trim();
        return label.length < 40 && SKIP.test(label) && reachable(el, r);
      })?.el || null
    );
  }

  // While watching, a skip button that shows up gets the highlight, so A presses it.
  // Otherwise the highlight only lives as long as the player's controls do.
  setInterval(() => {
    if (!player()) return;
    const now = Date.now();
    if (now < controlsUntil) {
      // The player hid its controls on its own (give them a moment to fade in first).
      const settled = now > controlsUntil - CONTROLS_IDLE_MS + 1500;
      if (settled && current && !visibleNow(current)) {
        controlsUntil = 0;
        ringShown = false;
      }
      return;
    }
    const s = skipButton();
    if (s) {
      if (s !== current || !ringShown) show(s, false);
    } else {
      ringShown = false;
    }
  }, 1000);

  // A seek or play/pause with the controls down: the overlay shows the progress for a moment
  // and presses the site's own button for it where it has to.
  function peek(key) {
    if (!overlay) return key;
    overlay.peek();
    return overlay.command(key);
  }

  function watching(name, v) {
    switch (name) {
      case 'left':
        return peek('ArrowLeft');
      case 'right':
        return peek('ArrowRight');
      case 'select': {
        const s = skipButton();
        if (s) {
          ringShown = false;
          return activate(s);
        }
        return peek(' ');
      }
      case 'up':
      case 'down':
        wake(v);
        controlsUntil = Date.now() + CONTROLS_IDLE_MS;
        current = null;
        setTimeout(() => show(skipButton() || nearest(vw() / 2, vh() * 0.88), false), 200);
        return true;
      case 'back':
        if (document.fullscreenElement) {
          document.exitFullscreen();
          return true;
        }
        if (history.length > 1) {
          ringShown = false;
          history.back();
          return true;
        }
        return 'Escape';
      case 'playpause':
        return peek(' ');
      case 'y':
        return 'f';
      default:
        return true;
    }
  }

  function press(name) {
    // A live sports channel from the Sports tab: its overlay (player/livetv.js) comes first.
    const live = window.__tvlLive;
    if (live) {
      const answer = live.press(name);
      if (answer !== undefined) return answer;
    }
    const v = player();
    // The launcher's own player controls, where there are some, take the pad first.
    if (v && overlay) {
      const answer = overlay.press(name);
      if (answer !== undefined) return answer;
    }
    // The left stick's up / down: the same as the D-pad's here.
    if (name === 'stickup' || name === 'stickdown') name = name.slice(5);
    const controlsUp = Date.now() < controlsUntil;
    // Played from the launcher: Back leaves the player for the launcher's details screen
    // (with the controls up, it closes them first).
    if (v && name === 'back' && window.__tvlWatch && !controlsUp) return 'close';
    if (v && !controlsUp) return watching(name, v);
    if (v) {
      // On the player's controls: keep them up while the D-pad is in use.
      wake(v);
      controlsUntil = Date.now() + CONTROLS_IDLE_MS;
      if (name === 'back') {
        controlsUntil = 0;
        ringShown = false;
        return true;
      }
      if (name === 'playpause') return ' ';
      if (name === 'y') return 'f';
    }
    switch (name) {
      case 'up':
      case 'down':
      case 'left':
      case 'right':
        return navigate(name);
      case 'select':
        if (!valid(current) || !ringShown) {
          pick();
          return true;
        }
        return activate(current);
      case 'back':
        return back();
      case 'pageup':
        page(-1);
        return true;
      case 'pagedown':
        page(1);
        return true;
      default:
        // X and Y mean nothing outside the player.
        return true;
    }
  }

  if (overlay) {
    overlay.attach({
      wake() {
        const v = player();
        if (v) wake(v);
      },
      focusNear: (x, y) => show(nearest(x, y), false),
      handOver() {
        controlsUntil = Date.now() + CONTROLS_IDLE_MS;
        current = null;
        ringShown = false;
      },
      quiet() {
        ringShown = false;
      },
    });
  }

  window.__tvnav = {
    press(name) {
      moveTo = null;
      let answer;
      try {
        answer = press(name);
      } catch (e) {
        console.warn('tvnav', e);
        answer = false;
      }
      if (!moveTo) return answer;
      // { move, then }: the launcher moves the mouse for real, then acts on the answer.
      launcherMoveUntil = Date.now() + 1000;
      const move = moveTo;
      moveTo = null;
      return { move, then: answer };
    },
  };
})();
