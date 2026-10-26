import { useEffect, useState } from 'react';

import { tr } from '../../core/i18n.js';
import { copyText } from '../format.js';

/**
 * Свой тултип вместо нативного title: тот появляется с секундной задержкой,
 * невидим, пока не догадаешься навести, и не работает с клавиатуры.
 */
export function Tip({ tip, children }: { tip: string; children: React.ReactNode }) {
  return (
    <span className="tip" tabIndex={0}>
      {children}
      <span className="tip-pop" role="tooltip">
        {tip}
      </span>
    </span>
  );
}

/**
 * Имя предмета, копируемое кликом. Цену пробивают поиском на бирже,
 * и перепечатывать «Voidlands Beacon (Timemark 8)» руками — мучение.
 * Подтверждение показывается на месте имени: взгляд уже там.
 */
export function CopyName({ name }: { name: string }) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1100);
    return () => clearTimeout(t);
  }, [copied]);

  return (
    <button
      className={`drop-name copy-name ${copied ? 'copied' : ''}`}
      title={tr('Скопировать название', 'Copy name')}
      onClick={() => void copyText(name).then(() => setCopied(true))}
    >
      {copied ? tr('Скопировано ✓', 'Copied ✓') : name}
    </button>
  );
}

/**
 * Иконка предмета. Загрузка может не удаться: нет интернета при первом показе
 * либо предмета нет в справочнике. Тогда остаётся пустое место того же
 * размера — строка не съезжает.
 */
export function ItemIcon({ src }: { src: string | null }) {
  const [broken, setBroken] = useState(false);
  if (!src || broken) return <span className="no-icon" />;
  return <img src={src} alt="" loading="lazy" onError={() => setBroken(true)} />;
}

export function Tile({
  label,
  value,
  live = false,
  warn,
  tip,
}: {
  label: string;
  value: React.ReactNode;
  live?: boolean;
  /** Пояснение, почему цифре нельзя верить как есть. */
  warn?: string;
  /** Что это за метрика — почти одинаковые подписи «в час» требуют слов. */
  tip?: string;
}) {
  const caption = tip ? <Tip tip={tip}>{label}</Tip> : label;
  return (
    <div className={`tile ${live ? 'tile-live' : ''} ${warn ? 'tile-warn' : ''}`}>
      <div className="tile-label">
        {caption}
        {warn && (
          <span className="tile-warn-mark" title={warn}>
            !
          </span>
        )}
      </div>
      <div className="tile-value">{value}</div>
      {warn && <div className="tile-note">{warn}</div>}
    </div>
  );
}
