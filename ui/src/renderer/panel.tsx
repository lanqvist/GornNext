import './bridge.js';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { tr } from '../core/i18n.js';
import { initLanguage } from './language.js';
import { PanelApp } from './PanelApp.js';
import './panel.css';

const root = document.getElementById('root');
if (!root) throw new Error('Не найден корневой элемент');

void initLanguage().then(() => {
  document.title = tr('Горн — пульт', 'Gorn — panel');
  createRoot(root).render(
    <StrictMode>
      <PanelApp />
    </StrictMode>,
  );
});
