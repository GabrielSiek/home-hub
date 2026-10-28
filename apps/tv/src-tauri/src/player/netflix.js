// Netflix for the launcher's player overlay (player/overlay.js, which describes an adapter).
(() => {
  const sites = (window.__tvlPlayerSites = window.__tvlPlayerSites || {});

  // The episode playing, from the address (/watch/70274103).
  const playingId = () => (location.pathname.match(/\/watch\/(\d+)/) || [])[1] || null;
  // The show's metadata, read for one episode: { for, video } (video null while it loads).
  let meta = null;
  // The season the episodes panel shows (null: the one playing).
  let shownSeason = null;

  sites['www.netflix.com'] = {
    accent: '#e50914',
    font: 'Netflix Sans',
    // Next and Episodes are offered from the title's episode line, before Netflix shows them.
    seriesFromTitle: true,
    // Netflix's own controls, unless handed over (html.tvl-native). Its skip and
    // end-of-episode buttons still show by themselves.
    css: `
      html:not(.tvl-native) [data-uia="player"]
        > :not([data-uia="video-canvas"]):not(:has([data-uia^="player-skip"], [data-uia^="next-episode"])) {
        display: none !important;
      }`,
    player: () => document.querySelector('[data-uia="player"]'),
    videoId() {
      const p = this.player();
      return p && p.getAttribute('data-videoid');
    },
    // Its own end-of-episode screen (watch credits / next episode).
    ending: () => !!document.querySelector('.SeamlessControls--container, [data-uia^="next-episode-seamless"]'),
    // The page's own player object: exact time, volume and tracks.
    api() {
      try {
        const vp = window.netflix.appContext.state.playerApp.getAPI().videoPlayer;
        const ids = vp.getAllPlayerSessionIds();
        const id = ids.find((s) => s.startsWith('watch')) || ids[0];
        return id ? vp.getVideoPlayerBySessionId(id) : null;
      } catch (_) {
        return null;
      }
    },
    // Only in the page while Netflix's controls are up (hidden or not).
    title() {
      const t = document.querySelector('[data-uia="video-title"]');
      if (!t) return null;
      const h = t.querySelector('h4');
      const spans = [...t.querySelectorAll('span')].map((s) => s.textContent.trim()).filter(Boolean);
      return { title: (h ? h.textContent : t.textContent).trim(), episode: spans.join(' · ') };
    },
    // The player object's clock, or else the timeline's (only there while the controls are up).
    clock() {
      const p = this.api();
      if (p) {
        const length = p.getDuration() / 1000;
        if (length > 0) return { at: p.getCurrentTime() / 1000, length };
      }
      const k = document.querySelector('[data-uia="timeline-knob"]');
      if (!k) return null;
      const length = Number(k.getAttribute('aria-valuemax')) / 1000;
      return length > 0 ? { at: Number(k.getAttribute('aria-valuenow')) / 1000, length } : null;
    },
    volume() {
      const p = this.api();
      return p ? { level: p.getVolume(), muted: p.isMuted() } : null;
    },
    setVolume(level, muted) {
      const p = this.api();
      if (!p) return false;
      p.setVolume(level);
      p.setMuted(muted);
      return true;
    },
    tracks() {
      const p = this.api();
      if (!p) return null;
      const list = (tracks, on) =>
        tracks
          .filter((x) => x.displayName && !x.isForced)
          .map((x) => ({ id: String(x.trackId), name: x.displayName, on: !!on && String(x.trackId) === String(on.trackId) }));
      return {
        audio: list(p.getAudioTrackList(), p.getAudioTrack()),
        text: list(p.getTimedTextTrackList(), p.getTimedTextTrack()),
      };
    },
    pick(kind, id) {
      const p = this.api();
      if (!p) return;
      const list = kind === 'audio' ? p.getAudioTrackList() : p.getTimedTextTrackList();
      const track = list.find((x) => String(x.trackId) === id);
      if (!track) return;
      if (kind === 'audio') p.setAudioTrack(track);
      else p.setTimedTextTrack(track);
    },
    // The episodes from Netflix's own metadata (what its episodes panel shows), read once for
    // the episode playing. Netflix only opens that panel for a real mouse over its controls.
    openEpisodes() {
      shownSeason = null;
      const id = playingId();
      if (!id || (meta && meta.for === id)) return;
      meta = { for: id, video: null };
      const asked = meta;
      fetch(`/nq/website/memberapi/release/metadata?movieid=${id}`, { credentials: 'include' })
        .then((r) => r.json())
        .then((j) => {
          asked.video = j.video || { seasons: [] };
        })
        .catch(() => {
          // Asked again the next time the panel opens.
          if (meta === asked) meta = null;
        });
    },
    episodes() {
      const video = meta && meta.video;
      if (!video) return null;
      const seasons = video.seasons || [];
      const playing = playingId();
      const current = seasons.find((s) => s.episodes.some((e) => String(e.id) === playing)) || seasons[0];
      if (!current) return { seasons: [], list: [] };
      const shown = seasons.find((s) => String(s.id) === shownSeason) || current;
      return {
        seasons: seasons.map((s) => ({ id: String(s.id), name: s.title || s.longName, on: s === shown })),
        list: shown.episodes.map((e) => {
          const bookmark = e.bookmark || {};
          const done = bookmark.offset >= (e.watchedToEndOffset || e.runtime);
          return {
            id: String(e.id),
            number: shown.hiddenEpisodeNumbers || e.hiddenEpisodeNumbers ? '' : e.seq,
            title: e.title,
            synopsis: e.synopsis,
            image: ((e.thumbs || [])[0] || (e.stills || [])[0] || {}).url || '',
            progress: done ? 1 : e.runtime && bookmark.offset > 0 ? bookmark.offset / e.runtime : 0,
            on: String(e.id) === playing,
          };
        }),
      };
    },
    pickSeason(id) {
      shownSeason = id;
    },
    // Netflix doesn't switch episodes for an address changed in place: the episode's page loads.
    playEpisode(id) {
      if (id !== playingId()) location.assign(`/watch/${id}`);
    },
    buttons: {
      next: '[data-uia="control-next"]',
      episodes: '[data-uia="control-episodes"]',
      audio: '[data-uia="control-audio-subtitle"]',
    },
  };
})();
