import { useEffect, useState } from 'react';

import { api, type Download, type Downloads } from '../lib/api';
import { brandOf, episodeLabel } from '../lib/brands';
import { etaText, speedText } from '../lib/library';
import { toast } from '../lib/toast';
import { FocusButton } from './FocusButton';
import { FocusGroup } from './FocusGroup';
import { Icon } from './Icon';
import { ProgressBar } from './SvcParts';

/** How often the list is read again. */
const EVERY_MS = 3000;

function meta(d: Download) {
  if (d.note) return d.note;
  if (d.paused) return 'Paused';
  return [`${Math.round(d.progress * 100)}%`, d.speed ? speedText(d.speed) : null, d.eta_s ? etaText(d.eta_s) : null].filter(Boolean).join(' · ');
}

/** What's on its way to the library. A pauses or resumes it. */
export function LibraryDownloads() {
  const [all, setAll] = useState<Downloads | null>(null);
  const [error, setError] = useState<string | null>(null);
  const brand = brandOf('library');

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const read = () =>
      api
        .libraryDownloads()
        .then((d) => alive && setAll(d))
        .catch((e) => alive && setError(String(e)))
        .finally(() => {
          if (alive) timer = setTimeout(read, EVERY_MS);
        });
    read();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, []);

  const flip = (d: Download) => {
    if (!d.hash) return;
    api
      .libraryDownloadControl(d.hash, d.paused ? 'resume' : 'pause')
      .then(() => setAll((cur) => cur && { ...cur, items: cur.items.map((x) => (x.hash === d.hash ? { ...x, paused: !d.paused } : x)) }))
      .catch(toast);
  };

  const items = all?.items ?? [];
  return (
    <section className="lib-downloads" aria-label="Downloads">
      <h2>
        Downloads
        {all && all.speed > 0 && <span className="lib-speed">{speedText(all.speed)}</span>}
      </h2>
      {all?.problems.map((p) => (
        <p key={p} className="lib-note">
          {p}
        </p>
      ))}
      {items.length > 0 ? (
        <FocusGroup className="lib-list scroller" column>
          {items.map((d, i) => (
            <FocusButton key={`${d.hash ?? d.title}-${i}`} focusKey={`lib-dl-${i}`} className="lib-item" onSelect={() => flip(d)}>
              <span className="lib-item-text">
                <strong>
                  {d.title}
                  {d.season > 0 && ` · ${d.episode > 0 ? episodeLabel(brand, d.season, d.episode) : `Season ${d.season}`}`}
                </strong>
                <span>{meta(d)}</span>
              </span>
              <ProgressBar share={d.progress} className="svc-bar lib-bar" />
              {d.hash && <Icon name={d.paused ? 'play' : 'pause'} size={26} />}
            </FocusButton>
          ))}
        </FocusGroup>
      ) : (
        <p className="svc-empty">{error ?? (all ? 'Nothing is downloading.' : '')}</p>
      )}
    </section>
  );
}
