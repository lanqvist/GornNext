import { useEffect, useRef, useState } from 'react';

import type { ToastSnapshot } from '../main/toastFeed.js';
import { ToastList, useIncomeToasts, type Toast } from './overlay/toasts.js';

const CDN = 'https://cdn.tlidb.com/UI/Textures/Common/Icon';

/**
 * Стенд плашек (overlay-lab, toasts.html?preview=1): все виды сразу
 * и без затухания — живые плашки гаснут за секунды, их не разглядеть.
 */
const PREVIEW: Toast[] = [
  { id: 1, kind: 'pickup', fe: 0.8 },
  {
    id: 6,
    kind: 'notable',
    fe: 14.6,
    name: 'Precious Ember ×2',
    icon: `${CDN}/Ash/128/UI_Ash_20StonelowYOUZHI_Icon_128.webp`,
    more: 2,
  },
  {
    id: 2,
    kind: 'jackpot',
    fe: 68,
    name: 'Soldier Compass ×2',
    icon: `${CDN}/Compass/128/UI_Compass_MonsterNormal_1_1_Icon_128.webp`,
  },
  {
    id: 3,
    kind: 'runEnd',
    fe: 92,
    durationMs: 112_000,
    deltaPct: 41,
    topDrops: [
      { name: 'Soldier Compass', icon: `${CDN}/Compass/128/UI_Compass_MonsterNormal_1_1_Icon_128.webp`, fe: 68, tier: 'gold' },
      { name: 'Precious Ember', icon: `${CDN}/Ash/128/UI_Ash_20StonelowYOUZHI_Icon_128.webp`, fe: 13, tier: 'orange' },
      { name: 'Netherrealm resonance', icon: `${CDN}/Item/128/UI_Item_Echo_01_Icon_128.webp`, fe: 3.1, tier: 'plain' },
    ],
  },
  {
    id: 4,
    kind: 'runEnd',
    fe: 31,
    durationMs: 60_000,
    record: true,
    deltaPct: 57,
    topDrops: [
      { name: 'Precious Ember', icon: `${CDN}/Ash/128/UI_Ash_20StonelowYOUZHI_Icon_128.webp`, fe: 13, tier: 'orange' },
    ],
  },
  {
    id: 5,
    kind: 'prison',
    fe: 171,
    topDrops: [
      { name: 'Flame Elementium', icon: `${CDN}/Item/128/UI_Item_chuhuoyuanzhiV3_Icon_128.webp`, fe: 104, tier: 'gold' },
      { name: 'Precious Ember', icon: `${CDN}/Ash/128/UI_Ash_20StonelowYOUZHI_Icon_128.webp`, fe: 12, tier: 'orange' },
    ],
  },
];

const preview =
  import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === '1';

/**
 * Слой всплывающего дохода — отдельное прозрачное окно у центра экрана,
 * поверх игры, всегда пропускающее клики.
 *
 * Почему не внутри оверлея: оверлей — приборная панель в углу, на неё
 * смотрят изредка и специально. Момент «что сейчас упало» должен ловиться
 * боковым зрением у персонажа, как floating combat text в самой игре.
 *
 * Окно живёт скрытым и показывается только пока есть плашки: прозрачное
 * окно стоит композитору дорого, а этот слой большую часть времени пуст.
 */
export function ToastLayerApp() {
  const [snapshot, setSnapshot] = useState<ToastSnapshot | null>(null);
  const [prefs, setPrefs] = useState<GornOverlayPrefs>({
    popups: true,
    sound: true,
    volume: 0.5,
    divineFe: 20,
    jackpotFe: 50,
    exaltedFe: 5,
    fontScale: 1,
    goalFe: 0,
  });
  const root = useRef<HTMLDivElement>(null);

  const toasts = useIncomeToasts(snapshot, prefs);

  // Главному процессу сообщается и высота, и «есть ли что показывать»:
  // по второму он прячет окно целиком.
  useEffect(() => {
    window.gorn?.toasts.reportActive(toasts.length > 0);
  }, [toasts.length]);

  useEffect(() => {
    const el = root.current;
    if (!el || !window.gorn) return;
    const observer = new ResizeObserver(() => {
      window.gorn.toasts.reportHeight(el.getBoundingClientRect().height);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    // В браузере имитируем подборы — вёрстку и звук видно без игры.
    if (!window.gorn) {
      if (import.meta.env.DEV && !preview) {
        let timer: ReturnType<typeof setInterval> | undefined;
        void import('./demo.js').then((m) => {
          let current = m.demoSnapshot;
          setSnapshot(current);
          timer = setInterval(() => {
            const gained = Math.random() < 0.18 ? 55 + Math.random() * 60 : Math.random() * 7;
            const cur = current.current;
            if (!cur) return;
            current = {
              ...current,
              current: {
                ...cur,
                netFe: cur.netFe + gained,
                valueFe: cur.valueFe + gained,
                drops: cur.drops.map((d, i) =>
                  i === (gained > 50 ? 0 : 3)
                    ? { ...d, qty: d.qty + 1, totalFe: (d.totalFe ?? 0) + gained, unitFe: gained }
                    : d,
                ),
              },
            };
            setSnapshot(current);
          }, 2200);
        });
        return () => timer && clearInterval(timer);
      }
      return;
    }

    void window.gorn.getSnapshot().then((s) => s && setSnapshot(s));
    void window.gorn.overlay.getState().then((s) => {
      setPrefs({
        popups: s.popups ?? true,
        sound: s.sound ?? true,
        volume: s.volume ?? 0.5,
        divineFe: s.divineFe ?? 20,
        jackpotFe: s.jackpotFe ?? 50,
        exaltedFe: s.exaltedFe ?? 5,
        fontScale: s.fontScale ?? 1,
        goalFe: s.goalFe ?? 0,
      });
    });
    const offSnapshot = window.gorn.onSnapshot(setSnapshot);
    const offPrefs = window.gorn.overlay.onPrefs(setPrefs);
    return () => {
      offSnapshot();
      offPrefs();
    };
  }, []);

  return (
    <div ref={root} className={preview ? 'tl tl-preview' : 'tl'}>
      <ToastList toasts={preview ? PREVIEW : toasts} />
    </div>
  );
}
