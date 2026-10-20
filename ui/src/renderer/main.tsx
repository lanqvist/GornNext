import './bridge.js';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { tr } from '../core/i18n.js';
import { App } from './App.js';
import { initLanguage } from './language.js';
import './styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('Не найден корневой элемент');

void initLanguage().then(() => {
  document.title = tr('Горн — трекер Torchlight: Infinite', 'Gorn — Torchlight: Infinite tracker');
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
