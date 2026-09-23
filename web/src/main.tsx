import { render } from 'solid-js/web';
import { App } from './app/App';
import './styles.css';
import { installTooltips } from './lib/tooltip';

installTooltips();

render(() => <App />, document.getElementById('root')!);
