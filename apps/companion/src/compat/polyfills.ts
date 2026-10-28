// What iOS 12's Safari lacks at runtime (the build already lowers the syntax, vite.config.ts).
// Imported first, before React.

type Loose = Record<string, unknown>;

if (typeof globalThis === 'undefined') (window as unknown as Loose).globalThis = window;

if (!Object.fromEntries) {
  Object.fromEntries = <T,>(entries: Iterable<readonly [PropertyKey, T]>) => {
    const o: Record<PropertyKey, T> = {};
    for (const [k, v] of entries) o[k] = v;
    return o;
  };
}

if (!(Promise as unknown as Loose).allSettled) {
  (Promise as unknown as Loose).allSettled = (ps: Iterable<unknown>) =>
    Promise.all(
      Array.from(ps, (p) =>
        Promise.resolve(p).then(
          (value) => ({ status: 'fulfilled', value }),
          (reason) => ({ status: 'rejected', reason }),
        ),
      ),
    );
}

// Safari 15.4. The TV's api copies its sample data with it (`?demo`), plain JSON all of it.
if (typeof structuredClone !== 'function') {
  (globalThis as unknown as Loose).structuredClone = <T,>(value: T): T => (value === undefined ? value : JSON.parse(JSON.stringify(value)));
}

// ---------- Full screen height (iOS 12 home screen apps) ----------
// Opened from the home screen with the status bar over the page, iOS 12 lays the page out as tall
// as the screen less the status bar, yet draws it from the screen's top edge: a black band is
// left along the bottom. There #root (styles.css) is made as tall as the screen instead.

function fitScreen() {
  const root = document.documentElement;
  const full = window.innerWidth > window.innerHeight ? Math.min(screen.width, screen.height) : Math.max(screen.width, screen.height);
  const missing = full - root.clientHeight;
  root.style.setProperty('--app-h', missing > 0 && missing <= 40 ? `${full}px` : '');
}

if ((navigator as Navigator & { standalone?: boolean }).standalone) {
  fitScreen();
  window.addEventListener('resize', fitScreen);
  // iOS reports the new size a moment after it turns.
  window.addEventListener('orientationchange', () => window.setTimeout(fitScreen, 300));
}

// ---------- Flexbox gap (Safari 14.1) ----------
// Safari 12 knows `gap` only for grids: flex rows and columns lay out with no space between
// their items. Where that's so, the items of every flex container with an inline gap get the
// space as a margin instead, kept up to date as React changes the page. A run of text is an item
// too ("● Offline", an icon and its label): the space beside it goes on the element next to it.

function flexGapWorks() {
  const box = document.createElement('div');
  box.style.display = 'flex';
  box.style.flexDirection = 'column';
  box.style.rowGap = '1px';
  box.appendChild(document.createElement('div'));
  box.appendChild(document.createElement('div'));
  document.body.appendChild(box);
  const works = box.scrollHeight === 1;
  document.body.removeChild(box);
  return works;
}

/** Margins this shim set, so they can change or go when the layout does. */
const MARK = 'data-gap';

function spaceItems(box: HTMLElement) {
  const s = box.style;
  const flex = s.display === 'flex' || s.display === 'inline-flex';
  const [rowGap = '', colGap = rowGap] = (s.gap || '').split(' ');
  const row = s.rowGap || rowGap;
  const col = s.columnGap || colGap;
  const column = s.flexDirection === 'column' || s.flexDirection === 'column-reverse';
  const wrap = s.flexWrap === 'wrap';
  const space = column ? row : col;
  // What lays out in the line, in order: elements in the flow and text that isn't blank.
  const items: Node[] = [];
  for (let i = 0; i < box.childNodes.length; i++) {
    const n = box.childNodes[i];
    if (n.nodeType === Node.TEXT_NODE) {
      if (n.nodeValue && n.nodeValue.trim()) items.push(n);
    } else if (n.nodeType === Node.ELEMENT_NODE) {
      const st = (n as HTMLElement).style;
      if (st.position !== 'absolute' && st.position !== 'fixed' && st.display !== 'none') items.push(n);
    }
  }
  for (let i = 0; i < box.children.length; i++) {
    const item = box.children[i] as HTMLElement;
    const at = items.indexOf(item);
    const want: [string, string][] = [];
    if (flex && at >= 0 && (row || col)) {
      if (wrap) {
        if (col) want.push(['marginRight', col]);
        if (row) want.push(['marginBottom', row]);
      } else if (space) {
        if (at > 0) want.push([column ? 'marginTop' : 'marginLeft', space]);
        const next = items[at + 1];
        if (next && next.nodeType === Node.TEXT_NODE) want.push([column ? 'marginBottom' : 'marginRight', space]);
      }
    }
    const style = item.style as unknown as Record<string, string>;
    const had = item.getAttribute(MARK);
    const now = want.map(([k, v]) => `${k}:${v}`).join(';');
    // Still as this shim left it? React rewrites a margin it changes, shorthand and all.
    if (had === null ? !now : had === now && want.every(([k, v]) => style[k] === v)) continue;
    // Undo what was set before (where it's still there), then set what's wanted, leaving the
    // item's own margins be; a 0 on the gap's side ("margin: -12px -8px -12px 0") is none to keep.
    if (had) {
      for (const kv of had.split(';')) {
        const k = kv.split(':')[0];
        if (style[k] === kv.slice(k.length + 1)) style[k] = '';
      }
    }
    const set: string[] = [];
    for (const [k, v] of want) {
      if (style[k] && !/^0(px)?$/.test(style[k])) continue;
      style[k] = v;
      set.push(`${k}:${v}`);
    }
    if (set.length) item.setAttribute(MARK, set.join(';'));
    else item.removeAttribute(MARK);
  }
}

function startFlexGap() {
  let queued = false;
  const observer = new MutationObserver(() => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(run);
  });
  const watch = () => observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['style'] });
  function run() {
    queued = false;
    observer.disconnect();
    const boxes = document.querySelectorAll<HTMLElement>('[style*="gap"]');
    for (let i = 0; i < boxes.length; i++) spaceItems(boxes[i]);
    observer.takeRecords();
    watch();
  }
  watch();
  requestAnimationFrame(run);
}

if (!flexGapWorks()) startFlexGap();
