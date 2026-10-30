import ReactDOM from 'react-dom/client';
import { GetBoundingClientRectAdapter, init, setKeyMap } from '@noriginmedia/norigin-spatial-navigation';

import '@fontsource/poppins/400.css';
import '@fontsource/poppins/500.css';
import '@fontsource/poppins/600.css';
import '@fontsource/poppins/700.css';
import './styles/tokens.css';
import './styles/app.css';
import './styles/twitch.css';
import './styles/streaming.css';
import './styles/player.css';
import './styles/sports.css';
import './styles/calendar.css';
import './styles/live.css';
import './styles/clock.css';
import './styles/retro.css';
import './styles/smarthome.css';
import './styles/rooms.css';

import App from './App';
import { ErrorBoundary } from './components/ErrorBoundary';

// On-screen rects, so navigation stays right inside scrolled rows.
init({ distanceCalculationMethod: 'center', layoutAdapter: GetBoundingClientRectAdapter });
// All keys go through lib/input.ts so the keyboard and the controller share one path.
setKeyMap({ left: [], right: [], up: [], down: [], enter: [] });

ReactDOM.createRoot(document.getElementById('root')!).render(
  <ErrorBoundary>
    <App />
  </ErrorBoundary>,
);
