import { useEffect, useRef } from 'react';

/**
 * Искры с раскалённой кромки оверлея: короткие сеансы по несколько искр,
 * а не постоянный поток. Кадры рисуются только пока искра летит — между
 * сеансами окно не перерисовывается (урок 19.10: постоянная анимация
 * прозрачного окна стоит процессора). При «уменьшить движение» — без искр.
 */

/** Сеанс искр: раз в столько мс и по столько штук — по уровню жара. */
const PUFFS: Record<1 | 2 | 3, { everyMs: number; count: number }> = {
  1: { everyMs: 4500, count: 3 },
  2: { everyMs: 2500, count: 4 },
  3: { everyMs: 1500, count: 5 },
};
/** Сноп при входе в белый жар (рекордный темп). */
const BURST = 14;

interface Spark {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  max: number;
  size: number;
}

function sparkColor(k: number): string {
  return k > 0.7 ? '#fff3d6' : k > 0.45 ? '#ffc93c' : k > 0.2 ? '#ff7a1a' : '#b3261e';
}

export function Sparks({ level }: { level: 0 | 1 | 2 | 3 }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const sparks = useRef<Spark[]>([]);
  const raf = useRef(0);
  const prevLevel = useRef(level);

  useEffect(() => {
    const cv = canvas.current;
    if (!cv || level === 0 || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const ctx = cv.getContext('2d');
    if (!ctx) return;
    let last = 0;

    const frame = (t: number) => {
      const dt = Math.min(0.05, last ? (t - last) / 1000 : 0.016);
      last = t;
      const w = cv.clientWidth;
      const h = cv.clientHeight;
      const d = devicePixelRatio || 1;
      if (cv.width !== Math.round(w * d) || cv.height !== Math.round(h * d)) {
        cv.width = Math.round(w * d);
        cv.height = Math.round(h * d);
      }
      ctx.setTransform(d, 0, 0, d, 0, 0);
      ctx.clearRect(0, 0, w, h);
      for (const s of sparks.current) {
        s.life += dt;
        s.vy += 70 * dt;
        s.x += s.vx * dt;
        s.y += s.vy * dt;
        const k = 1 - s.life / s.max;
        if (k <= 0) continue;
        ctx.globalAlpha = Math.min(1, k * 1.5);
        ctx.fillStyle = sparkColor(k);
        ctx.beginPath();
        ctx.arc(s.x, s.y, s.size, 0, Math.PI * 2);
        ctx.fill();
      }
      sparks.current = sparks.current.filter((s) => s.life < s.max);
      if (sparks.current.length > 0) {
        raf.current = requestAnimationFrame(frame);
      } else {
        raf.current = 0;
        ctx.clearRect(0, 0, w, h);
      }
    };

    // Искры срываются с кромки и падают вниз, гаснут за полсекунды.
    const puff = (n: number, spread: number) => {
      if (document.hidden) return;
      const w = cv.clientWidth;
      for (let i = 0; i < n; i++) {
        sparks.current.push({
          x: 12 + Math.random() * Math.max(1, w - 24),
          y: 1,
          vx: (Math.random() - 0.5) * 40 * spread,
          vy: 15 + Math.random() * 45 * spread,
          life: 0,
          max: 0.45 + Math.random() * 0.45,
          size: 0.7 + Math.random() * 1.0,
        });
      }
      if (!raf.current) {
        last = 0;
        raf.current = requestAnimationFrame(frame);
      }
    };

    if (level === 3 && prevLevel.current !== 3) puff(BURST, 1.8);
    const { everyMs, count } = PUFFS[level];
    const timer = setInterval(() => puff(count, 1), everyMs);
    return () => {
      clearInterval(timer);
      cancelAnimationFrame(raf.current);
      raf.current = 0;
      sparks.current = [];
      ctx.clearRect(0, 0, cv.width, cv.height);
    };
  }, [level]);

  useEffect(() => {
    prevLevel.current = level;
  }, [level]);

  return <canvas ref={canvas} className="ov-sparks" aria-hidden="true" />;
}
