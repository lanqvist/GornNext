import { isUiLanguage, setUiLanguage, type UiLanguage } from '../core/i18n.js';

/**
 * Язык окна — до первого рендера: строки вычисляются при отрисовке,
 * и кадр на чужом языке мигнул бы перед правильным.
 *
 * В браузерном демо main-процесса нет: язык берётся из `?lang=en`,
 * чтобы вёрстку на обоих языках можно было проверять без Electron.
 * При смене языка в настройках окно перезагружается само.
 */
export async function initLanguage(): Promise<UiLanguage> {
  const fromUrl = new URLSearchParams(location.search).get('lang');
  let language: UiLanguage = isUiLanguage(fromUrl) ? fromUrl : 'ru';
  if (window.gorn) {
    try {
      language = await window.gorn.getUiLanguage();
    } catch {
      // Main ещё не готов отвечать — лучше русский, чем пустое окно.
    }
    window.gorn.onUiLanguage(() => location.reload());
  }
  setUiLanguage(language);
  document.documentElement.lang = language;
  return language;
}
