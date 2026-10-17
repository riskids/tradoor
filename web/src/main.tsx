import { render } from 'solid-js/web';
import { App } from './app/App';
import { warmEngine } from './engine/client';
import { loadOtherStreams } from './engine/streams';
import './styles.css';
import { installTooltips } from './lib/tooltip';

// What every first screen needs, begun before the layout has loaded to ask for it.
warmEngine();
loadOtherStreams();
installTooltips();

render(() => <App />, document.getElementById('root')!);
