// The companion's frame: the tablet's side rail (Hub, Watch, Home, Music, Today, then the
// assistant and the night clock) or the phone's bottom tab bar, around the screen on show.
import { lazy, Suspense, type ReactNode } from 'react';

import { HouseSheets } from './follow';
import { Icon, type IconName } from './icons';
import { go, openOver, SECTIONS, useLayout, useNav, type Layout, type Section } from './nav';
import { Overlays } from './ui';
import Hub from './screens/Hub';

const Watch = lazy(() => import('./screens/Watch'));
const Home = lazy(() => import('./screens/Home'));
const Music = lazy(() => import('./screens/Music'));
const Today = lazy(() => import('./screens/Today'));
const Night = lazy(() => import('./screens/Night'));
const Assistant = lazy(() => import('./screens/Assistant'));

const TABS: Record<Section, { label: string; icon: IconName }> = {
  hub: { label: 'Hub', icon: 'hub' },
  watch: { label: 'Watch', icon: 'watch' },
  home: { label: 'Home', icon: 'bulb' },
  music: { label: 'Music', icon: 'music' },
  today: { label: 'Today', icon: 'calendar' },
};

/** Props every screen gets. */
export interface ScreenProps {
  layout: Layout;
  sub: string | null;
}

export default function App() {
  const layout = useLayout();
  const { screen, sub } = useNav();

  let body: ReactNode;
  switch (screen) {
    case 'hub':
      body = <Hub layout={layout} sub={sub} />;
      break;
    case 'watch':
      body = <Watch layout={layout} sub={sub} />;
      break;
    case 'home':
      body = <Home layout={layout} sub={sub} />;
      break;
    case 'music':
      body = <Music layout={layout} sub={sub} />;
      break;
    case 'today':
      body = <Today layout={layout} sub={sub} />;
      break;
    case 'night':
      return (
        <Suspense fallback={null}>
          <Night layout={layout} sub={sub} />
          <HouseSheets layout={layout} />
          <Overlays layout={layout} />
        </Suspense>
      );
  }

  // The phone's remote is full screen, over the tab bar.
  const bare = layout === 'phone' && screen === 'watch' && sub === 'remote';

  return (
    <div
      style={{
        position: 'relative',
        height: '100%',
        overflow: 'hidden',
        background: 'linear-gradient(180deg, #0c0c14 0%, #0e0e18 100%)',
      }}
    >
      {layout === 'tablet' ? (
        <>
          <Rail screen={screen} />
          {/* The rail is 76 px; screens lay out in what's left, below the status bar. An offset
              rather than padding, so screens filling it with position: absolute start below too. */}
          <main
            style={{
              position: 'absolute',
              top: 'max(20px, var(--safe-top))',
              left: 'calc(76px + var(--safe-left))',
              right: 0,
              bottom: 0,
            }}
          >
            <Suspense fallback={null}>{body}</Suspense>
          </main>
        </>
      ) : (
        <>
          <main
            style={{
              position: 'absolute',
              top: 'calc(var(--safe-top) + 8px)',
              left: 0,
              right: 0,
              bottom: bare ? 0 : 'calc(57px + max(26px, var(--safe-bottom)))',
            }}
          >
            <Suspense fallback={null}>{body}</Suspense>
          </main>
          {!bare && <TabBar screen={screen} />}
        </>
      )}
      <Suspense fallback={null}>{sub === 'assistant' && <Assistant layout={layout} sub={sub} />}</Suspense>
      <HouseSheets layout={layout} />
      <Overlays layout={layout} />
    </div>
  );
}

function Rail({ screen }: { screen: string }) {
  return (
    <nav
      aria-label="Sections"
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        bottom: 0,
        width: 'calc(76px + var(--safe-left))',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 8,
        paddingTop: 'calc(max(20px, var(--safe-top)) + 14px)',
        paddingLeft: 'var(--safe-left)',
        boxSizing: 'border-box',
        background: 'rgba(18, 18, 26, 0.82)',
        borderRight: '1px solid rgba(255, 255, 255, 0.06)',
        zIndex: 30,
      }}
    >
      {SECTIONS.map((s) => {
        const on = screen === s;
        return (
          <button
            key={s}
            type="button"
            className="rl"
            aria-current={on ? 'page' : undefined}
            onClick={() => go(s)}
            style={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 3,
              width: 60,
              height: 58,
              padding: 0,
              border: 0,
              borderRadius: 18,
              background: on ? '#0149B2' : 'transparent',
              color: on ? '#ffffff' : '#8a8a94',
              fontSize: 11,
              fontWeight: on ? 600 : 500,
              cursor: 'pointer',
              boxShadow: on ? '0 8px 22px rgba(1, 73, 178, 0.45)' : 'none',
            }}
          >
            <Icon name={TABS[s].icon} size={24} />
            {TABS[s].label}
          </button>
        );
      })}
      <button
        type="button"
        className="rl"
        aria-label="Ask the assistant"
        data-mic
        onClick={() => openOver('assistant')}
        style={{
          marginTop: 'auto',
          width: 48,
          height: 48,
          padding: 0,
          border: 0,
          borderRadius: 999,
          background: '#0149B2',
          color: '#ffffff',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          cursor: 'pointer',
          boxShadow: '0 0 22px rgba(1, 73, 178, 0.55)',
        }}
      >
        <Icon name="mic" size={22} />
      </button>
      <button
        type="button"
        className="rl"
        aria-label="Night clock"
        onClick={() => go('night')}
        style={{
          marginBottom: 'calc(14px + var(--safe-bottom))',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          gap: 3,
          width: 60,
          height: 52,
          justifyContent: 'center',
          padding: 0,
          border: 0,
          borderRadius: 16,
          background: 'transparent',
          color: '#8a8a94',
          fontSize: 11,
          fontWeight: 500,
          cursor: 'pointer',
        }}
      >
        <Icon name="night" size={22} />
        Night
      </button>
    </nav>
  );
}

function TabBar({ screen }: { screen: string }) {
  return (
    <nav
      aria-label="Sections"
      style={{
        position: 'absolute',
        left: 0,
        right: 0,
        bottom: 0,
        height: 'calc(57px + max(26px, var(--safe-bottom)))',
        display: 'flex',
        padding: '0 calc(6px + var(--safe-right)) 0 calc(6px + var(--safe-left))',
        boxSizing: 'border-box',
        background: 'rgba(16, 16, 24, 0.94)',
        borderTop: '1px solid rgba(255, 255, 255, 0.07)',
        zIndex: 40,
      }}
    >
      {SECTIONS.map((s) => {
        const on = screen === s;
        return (
          <button
            key={s}
            type="button"
            className="tb"
            aria-current={on ? 'page' : undefined}
            onClick={() => go(s)}
            style={{
              flex: '1 1 0',
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              gap: 4,
              paddingTop: 8,
              border: 0,
              background: 'transparent',
              color: on ? '#ffffff' : '#8a8a94',
              fontSize: 11,
              fontWeight: on ? 600 : 500,
              cursor: 'pointer',
            }}
          >
            <span
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 56,
                height: 30,
                borderRadius: 999,
                background: on ? '#0149B2' : 'transparent',
                boxShadow: on ? '0 6px 16px rgba(1, 73, 178, 0.45)' : 'none',
                transition: 'transform .15s var(--ease)',
              }}
            >
              <Icon name={TABS[s].icon} size={22} />
            </span>
            {TABS[s].label}
          </button>
        );
      })}
    </nav>
  );
}
