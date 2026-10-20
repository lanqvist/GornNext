import { useEffect, useState } from 'react';

/**
 * Текущее время с шагом stepMs. Тикает только компонент, который его
 * спросил: раньше секундный таймер жил в App и каждую секунду перерисовывал
 * всё окно — историю, графики, таблицы (~2,3% ядра у рендерера даже в
 * свёрнутом окне, замер 19.10). Поэтому время спрашивают маленькие
 * компоненты: часы забега, возраст лога, «обновлены N назад».
 */
export function useNow(stepMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), stepMs);
    return () => clearInterval(t);
  }, [stepMs]);
  return now;
}
