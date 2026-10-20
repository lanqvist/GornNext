import { useEffect, useState } from 'react';

import { tr } from '../../core/i18n.js';
import type { Snapshot } from '../../main/engine.js';
import { ago, cacheDate, copyText, num } from '../format.js';
import { useNow } from '../useNow.js';

/**
 * Самодиагностика. Формат лога меняется каждый сезон, и трекер должен
 * говорить, что ослеп, а не показывать нули как ни в чём не бывало.
 */
/** «обновлены N назад»: тикает сама, раз в 30 с — чаще слово не меняется. */
function UpdatedAgo({ at }: { at: number }) {
  const now = useNow(30_000);
  return <>{tr(` · обновлены ${ago(Math.max(0, now - at))}`, ` · updated ${ago(Math.max(0, now - at))}`)}</>;
}

export function Diagnostics({ snapshot }: { snapshot: Snapshot }) {
  const {
    unknownProtos,
    pricedItems,
    communityPrices,
    communityEnabled,
    communityFetchedAt,
    communityStale,
  } = snapshot.diagnostics;

  // Свежесть цен: счётчик «2 154 цены» ничего не говорит о том, не лежат ли
  // они в кэше третий день без сети.
  const freshness = !communityEnabled
    ? null
    : communityStale
      ? tr(
          ` · нет сети — кэш${communityFetchedAt ? ` от ${cacheDate(communityFetchedAt)}` : ''}`,
          ` · offline — cache${communityFetchedAt ? ` from ${cacheDate(communityFetchedAt)}` : ''}`,
        )
      : communityFetchedAt
        ? <UpdatedAgo at={communityFetchedAt} />
        : null;

  return (
    <section className="card diag">
      <h2>{tr('Самодиагностика', 'Self-check')}</h2>
      {/* Маркер относится к своей строке, а не к общему состоянию:
          иначе справочная строка выглядит как проблема. */}
      <div className="diag-row">
        <span className="ok">✓</span>
        <span>
          {tr(`Своих пробивок: ${num(pricedItems)}`, `Own price checks: ${num(pricedItems)}`)}
        </span>
      </div>
      {/* Только факты: ручка включения живёт в настройках. */}
      <div className="diag-row">
        <span className={communityEnabled ? 'ok' : 'muted'}>
          {communityEnabled ? '✓' : '·'}
        </span>
        <span>
          {communityEnabled
            ? tr(
                `Цен из базы сообщества: ${num(communityPrices)}`,
                `Community prices: ${num(communityPrices)}`,
              )
            : tr(
                'Цены сообщества выключены — только свои пробивки.',
                'Community prices are off — own price checks only.',
              )}
          {freshness && <span className="muted">{freshness}</span>}
        </span>
      </div>
      {unknownProtos.length > 0 && (
        <div className="diag-row">
          <span className="warn">!</span>
          <span>
            {tr('Незнакомые операции', 'Unknown operations')}:{' '}
            {unknownProtos.map(([p, n]) => `${p} ×${n}`).join(', ')}.{' '}
            {tr('Возможно, игра обновилась —', 'The game may have been updated —')}{' '}
            <a
              href="https://github.com/lanqvist/gorn-tli/issues"
              target="_blank"
              rel="noreferrer"
            >
              {tr('сообщите', 'report them')}
            </a>
            {tr(', чтобы я их учёл.', ' so I can handle them.')}
          </span>
          <CopyProtosReport snapshot={snapshot} />
        </div>
      )}
    </section>
  );
}

/**
 * Готовый текст для баг-репорта: игроку достаточно вставить его в issue,
 * не переписывая имена протоколов руками.
 */
function CopyProtosReport({ snapshot }: { snapshot: Snapshot }) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1100);
    return () => clearTimeout(t);
  }, [copied]);

  const report = [
    `Незнакомые операции в логе (${snapshot.seasonName ?? 'сезон ?'}, SeasonId ${snapshot.seasonId ?? '?'}):`,
    ...snapshot.diagnostics.unknownProtos.map(([p, n]) => `- ${p} ×${n}`),
  ].join('\n');

  return (
    <button
      className="diag-toggle"
      title={tr('Скопировать список для баг-репорта', 'Copy the list for a bug report')}
      onClick={() => void copyText(report).then(() => setCopied(true))}
    >
      {copied ? tr('скопировано ✓', 'copied ✓') : tr('скопировать отчёт', 'copy report')}
    </button>
  );
}
