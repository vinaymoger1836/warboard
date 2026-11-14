import { tokenCss } from '@warboard/symbols';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './styles.css';

// Colour tokens as CSS variables (doc 10 §2); styles.css uses only these.
const tokens = document.createElement('style');
tokens.textContent = tokenCss();
document.head.prepend(tokens);

// Touch or mouse (doc 06 §11, P12): `html[data-input]` switches the bottom-sheet unit card and
// the larger hit targets on. `?input=touch` forces it, for testing on a desktop browser.
const coarse = window.matchMedia('(pointer: coarse)');
const forced = new URLSearchParams(window.location.search).get('input');
const setInput = () => {
  document.documentElement.dataset.input = forced === 'touch' || forced === 'mouse' ? forced : coarse.matches ? 'touch' : 'mouse';
};
setInput();
coarse.addEventListener('change', setInput);

// App shell for offline use (doc 06 §9); production builds only.
if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  navigator.serviceWorker.register('./sw.js').catch((err: unknown) => console.warn('Service worker not registered', err));
}

const root = document.getElementById('root');
if (!root) throw new Error('Missing #root element');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
