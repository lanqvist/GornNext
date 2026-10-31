import { useEffect, useRef, useState } from 'react';

import { tr } from '../core/i18n.js';
import type { ToastSnapshot } from '../main/toastFeed.js';
import { ToastList, useIncomeToasts, type Toast } from './overlay/toasts.js';

const CDN = 'https://cdn.tlidb.com/UI/Textures/Common/Icon';

/**
 * Стенд плашек (overlay-lab, toasts.html?preview=1): все виды сразу
 * и без затухания — живые плашки гаснут за секунды, их не разглядеть.
 * Функция, а не константа: названия зависят от языка, а он известен
 * только после initLanguage.
 */
function previewToasts(): Toast[] {
  const compass = `${CDN}/Compass/128/UI_Compass_MonsterNormal_1_1_Icon_128.webp`;
  const ember = `${CDN}/Ash/128/UI_Ash_20StonelowYOUZHI_Icon_128.webp`;
  const echo = `${CDN}/Item/128/UI_Item_Echo_01_Icon_128.webp`;
  return [
    { id: 1, kind: 'pickup', fe: 0.8 },
    {
      id: 6,
      kind: 'notable',
      fe: 14.6,
      name: 'Precious Ember ×2',
      icon: ember,
      more: 2,
    },
    {
      id: 2,
      kind: 'jackpot',
      fe: 68,
      name: 'Soldier Compass ×2',
      icon: compass,
    },
    {
      id: 3,
      kind: 'runEnd',
      fe: 92,
      durationMs: 112_000,
      mapName: tr('Мерзкий лес', 'Foul Forest'),
      deltaPct: 41,
      topDrops: [
        { name: 'Soldier Compass', icon: compass, qty: 1, fe: 68, tier: 'gold' },
        { name: 'Precious Ember', icon: ember, qty: 2, fe: 13, tier: 'orange' },
        { name: 'Netherrealm resonance', icon: echo, qty: 1, fe: 3.1, tier: 'plain' },
      ],
    },
    {
      // Слабая карта с длинным названием: хвост «(метка времени 8)» — главное в нём.
      id: 7,
      kind: 'runEnd',
      fe: 1.3,
      durationMs: 96_000,
      mapName: tr('Громовые пустоши', 'Thunder Wastes'),
      deltaPct: -74,
      topDrops: [
        {
          name: tr('Маячок Громовых пустошей (метка времени 8)', 'Thunder Wastes Beacon (Timemark 8)'),
          icon: compass,
          qty: 1,
          fe: 0.46,
          tier: 'plain',
        },
        {
          // Нарочно длиннее строки: многоточие — в основе, хвост и ×3 целы.
          name: tr(
            'Карта-указатель к пепельному хранилищу ветров (метка времени 8)',
            'Waypoint Map to the Ashen Vault of Winds (Timemark 8)',
          ),
          icon: compass,
          qty: 3,
          fe: 0.42,
          tier: 'plain',
        },
        { name: tr('Отзвук Преисподней', 'Netherrealm resonance'), icon: echo, qty: 1, fe: 0.31, tier: 'plain' },
      ],
    },
    {
      id: 4,
      kind: 'runEnd',
      fe: 31,
      durationMs: 60_000,
      mapName: tr('Святилище агонии', 'Sanctum of Agony'),
      record: true,
      deltaPct: 57,
      topDrops: [{ name: 'Precious Ember', icon: ember, qty: 4, fe: 13, tier: 'orange' }],
    },
    {
      id: 5,
      kind: 'prison',
      fe: 171,
      mapName: tr('Тюрьма Талоса', 'Talos Prison'),
      topDrops: [
        {
          name: 'Flame Elementium',
          icon: `${CDN}/Item/128/UI_Item_chuhuoyuanzhiV3_Icon_128.webp`,
          qty: 1,
          fe: 104,
          tier: 'gold',
        },
        { name: 'Precious Ember', icon: ember, qty: 3, fe: 12, tier: 'orange' },
      ],
    },
  ];
}

const params = new URLSearchParams(location.search);
const preview = import.meta.env.DEV && params.get('preview') === '1';
/**
 * Сцена стенда (`&scene=1`): окно слоя 560×260 на своём месте — по центру,
 * нижняя кромка на 36% высоты экрана, как в layers.rs, — поверх макета
 * яркой игры. Соотношение сторон задаёт размер окна браузера: 16:9, 21:9.
 * `&only=3,7` — показать только эти плашки стенда.
 */
const scene = preview && params.has('scene');
const only = params.get('only')?.split(',').map(Number) ?? null;

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
    heat: true,
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
        heat: s.heat ?? true,
      });
    });
    const offSnapshot = window.gorn.onSnapshot(setSnapshot);
    const offPrefs = window.gorn.overlay.onPrefs(setPrefs);
    return () => {
      offSnapshot();
      offPrefs();
    };
  }, []);

  const shown = preview
    ? previewToasts().filter((t) => !only || only.includes(t.id))
    : toasts;
  const layer = (
    <div ref={root} className={preview ? 'tl tl-preview' : 'tl'}>
      <ToastList toasts={shown} heat={prefs.heat} />
    </div>
  );
  if (!scene) return layer;
  return (
    <div className="tl-scene">
      <i className="tl-scene-boss" />
      <i className="tl-scene-map" />
      <i className="tl-scene-hero" />
      <i className="tl-scene-bar" />
      {layer}
    </div>
  );
}
