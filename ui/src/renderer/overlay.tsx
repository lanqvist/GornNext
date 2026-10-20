import './bridge.js';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { tr } from '../core/i18n.js';
import { OverlayApp } from './OverlayApp.js';
import { initLanguage } from './language.js';
import './overlay.css';

const root = document.getElementById('root');
if (!root) throw new Error('Не найден корневой элемент');

void initLanguage().then(() => {
  document.title = tr('Горн — оверлей', 'Gorn — overlay');
  createRoot(root).render(
    <StrictMode>
      <OverlayApp />
    </StrictMode>,
  );
});
