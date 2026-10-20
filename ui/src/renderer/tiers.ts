import { createContext, useContext, useEffect, useState } from 'react';

/** Пороги ступеней ценности, ФЕ: «золото» и «оранжевое». */
export interface Tiers {
  gold: number;
  orange: number;
}

export type Tier = 'gold' | 'orange' | 'plain';

/**
 * Те же пороги, что у полос дропа в оверлее и у всплывашек: явные из
 * настроек оверлея либо «авто» — P95 цен сезона, оранжевое — пятая часть.
 */
export function tiersOf(jackpotFe: number, exaltedFe: number, jackpotAutoFe: number): Tiers {
  const gold = jackpotFe > 0 ? jackpotFe : jackpotAutoFe;
  return { gold, orange: exaltedFe > 0 ? exaltedFe : gold / 5 };
}

export function tierOf(fe: number | null, t: Tiers): Tier | null {
  if (fe === null || t.gold <= 0) return null;
  return fe >= t.gold ? 'gold' : fe >= t.orange ? 'orange' : 'plain';
}

/** Пороги для главного окна: из настроек оверлея, обновляются вместе с ними. */
export function useTiers(jackpotAutoFe: number): Tiers {
  const [prefs, setPrefs] = useState({ jackpotFe: 0, exaltedFe: 0 });
  useEffect(() => {
    if (!window.gorn) return;
    void window.gorn.overlay
      .getState()
      .then((s) => s && setPrefs({ jackpotFe: s.jackpotFe ?? 0, exaltedFe: s.exaltedFe ?? 0 }));
    return window.gorn.overlay.onPrefs((p) =>
      setPrefs({ jackpotFe: p.jackpotFe, exaltedFe: p.exaltedFe }),
    );
  }, []);
  return tiersOf(prefs.jackpotFe, prefs.exaltedFe, jackpotAutoFe);
}

/** Пороги, раздаваемые из App: списку добычи не нужно знать про настройки. */
export const TiersContext = createContext<Tiers>({ gold: 0, orange: 0 });

export const useTierContext = (): Tiers => useContext(TiersContext);
