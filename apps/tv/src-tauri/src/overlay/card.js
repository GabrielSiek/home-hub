// A calendar reminder over the page of the app in front, while the launcher is behind it.
// Called with the event's title, its start (unix ms) and its colour.
(function (title, start, color) {
  const ID = '__tvl-calendar-card';
  document.getElementById(ID)?.remove();
  const mins = Math.ceil((start - Date.now()) / 60000);
  const at = new Date(start).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });

  const card = document.createElement('div');
  card.id = ID;
  card.setAttribute('role', 'status');
  card.style.cssText = [
    'position:fixed',
    'top:48px',
    'right:48px',
    'z-index:2147483647',
    'width:460px',
    'box-sizing:border-box',
    'padding:18px 22px',
    'border-radius:22px',
    'background:rgba(20,22,32,0.92)',
    'border:1px solid rgba(255,255,255,0.12)',
    `box-shadow:0 24px 64px rgba(0,0,0,0.55),inset 4px 0 0 ${color}`,
    'backdrop-filter:blur(24px)',
    'color:#f2f2f3',
    "font-family:Poppins,'Segoe UI',sans-serif",
    'display:flex',
    'flex-direction:column',
    'gap:4px',
    'pointer-events:none',
  ].join(';');

  const kicker = document.createElement('div');
  kicker.textContent = `${mins > 0 ? `IN ${mins} MIN` : 'STARTING NOW'} · ${at}`;
  kicker.style.cssText = 'font-size:14px;font-weight:700;letter-spacing:0.06em;color:#9cc0ff';
  const name = document.createElement('div');
  name.textContent = title;
  name.style.cssText = 'font-size:22px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis';
  card.append(kicker, name);

  // A full-screen player is in the top layer, above anything else in the page: the card goes inside it.
  const full = document.fullscreenElement;
  (full && full.tagName !== 'VIDEO' ? full : document.body || document.documentElement).appendChild(card);
  setTimeout(() => card.remove(), 12000);
})
