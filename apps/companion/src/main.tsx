// First: what iOS 12's Safari lacks, before anything else runs.
import './compat/polyfills';

import ReactDOM from 'react-dom/client';

import '@fontsource/poppins/200.css';
import '@fontsource/poppins/300.css';
import '@fontsource/poppins/400.css';
import '@fontsource/poppins/500.css';
import '@fontsource/poppins/600.css';
import '@fontsource/poppins/700.css';
import './styles.css';

import { setTransport } from '@tv/lib/api';
import App from './App';
import { demo } from './backend';
import { startHouse } from './house';
import { startLive } from './live';
import { sayHello } from './screen';
import { checkAdmin, httpTransport } from './transport';
// Taps on what opens the assistant start sound, for the device's microphone (voice.ts).
import './voice';

// In the demo the TV's api answers with its sample data instead of calling the home server.
if (!demo) setTransport(httpTransport);
sayHello();
startLive();
startHouse();
// A session kept from before may have expired.
if (!demo) checkAdmin();

ReactDOM.createRoot(document.getElementById('root')!).render(<App />);
