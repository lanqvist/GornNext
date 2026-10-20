import './bridge.js';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { tr } from '../core/i18n.js';
import { ToastLayerApp } from './ToastLayerApp.js';
import { initLanguage } from './language.js';
import './toasts.css';

const root = document.getElementById('root');
if (!root) throw new Error('Не найден корневой элемент');

void initLanguage().then(() => {
  document.title = tr('Горн — всплывающий доход', 'Gorn — income popups');
  createRoot(root).render(
    <StrictMode>
      <ToastLayerApp />
    </StrictMode>,
  );
});
