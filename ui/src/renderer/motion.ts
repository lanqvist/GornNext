import { useEffect, useRef, useState, type RefObject } from 'react';

const reducedMotion = (): boolean =>
  typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/**
 * Число, которое доезжает до нового значения, а не прыгает: подбор
 * виден движением цифры. Кадры идут только полсекунды после перемены —
 * в покое хук ничего не перерисовывает. Скрытое окно и «меньше движения»
 * в системе — сразу конечное значение.
 */
export function useTween(target: number, ms = 480): number {
  const [shown, setShown] = useState(target);
  const shownRef = useRef(target);

  useEffect(() => {
    const from = shownRef.current;
    if (from === target) return;
    if (document.hidden || reducedMotion() || !Number.isFinite(from)) {
      shownRef.current = target;
      setShown(target);
      return;
    }
    const t0 = performance.now();
    let raf = 0;
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / ms);
      const v = k >= 1 ? target : from + (target - from) * (1 - (1 - k) ** 3);
      shownRef.current = v;
      setShown(v);
      if (k < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [target, ms]);

  return shown;
}

/** Ширина элемента в пикселях — чтобы график рисовался под окно, а не растягивался. */
export function useWidth(ref: RefObject<HTMLElement | null>, fallback: number): number {
  const [width, setWidth] = useState(fallback);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      const w = Math.round(entry?.contentRect.width ?? 0);
      if (w > 0) setWidth(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return width;
}
