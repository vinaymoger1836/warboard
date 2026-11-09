import { tokenCss } from '@warboard/symbols';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import './styles.css';

// Colour tokens as CSS variables (doc 10 §2); styles.css uses only these.
const tokens = document.createElement('style');
tokens.textContent = tokenCss();
document.head.prepend(tokens);

const root = document.getElementById('root');
if (!root) throw new Error('Missing #root element');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
