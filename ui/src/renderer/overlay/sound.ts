/**
 * Звук слоя всплывающего дохода.
 *
 * Сами звуки — готовые стерео-ассеты «Кузни»: их заранее рендерит наш
 * генератор (src/sfx/sounds.ts, npm run sounds). Звуков три, и только
 * на то, что стоит услышать не глядя:
 *  - «возвышение» — предмет дороже первого порога;
 *  - «дивайн» — предмет дороже второго порога и золотая находка;
 *  - «рекорд» — карта с лучшим темпом сессии.
 * Подбор и итог карты молчат: звук на каждый тик — фон, от которого
 * игрок за час выключит звук совсем. Громкость файлов выверена ещё при
 * рендере, поэтому здесь только выбор, когда и что проиграть.
 *
 * У частого «возвышения» несколько вариантов, они идут по кругу, и каждый
 * повтор чуть сдвинут по высоте (±0.5%) — ухо не замыливается.
 *
 * Файлы встроены в бандл как data-URL: страницы грузятся с file://,
 * и так звук работает одинаково в деве и в собранном приложении.
 * AudioContext и декодирование — лениво: слой почти всё время молчит.
 * Сами файлы — тоже: каждый в своём чанке, грузится при первом проигрывании.
 * Иначе 2,3 МБ base64 разбирались при старте каждого окна, где есть звук
 * (главное — ради кнопок прослушивания, слой плашек — даже без звука).
 */
import type { Sound } from '../../sfx/sounds.js';

const FILES = import.meta.glob<string>('../assets/sfx/*.wav', {
  query: '?inline',
  import: 'default',
});

type Variant = { path: string; url: () => Promise<string> };

/** Звук → его варианты (загрузчик data-URL), по порядку номеров в имени файла. */
const BANK = new Map<Sound, Variant[]>();
for (const [path, url] of Object.entries(FILES).sort(([a], [b]) => a.localeCompare(b))) {
  const match = /\/([A-Za-z]+)-\d+\.wav$/.exec(path);
  if (!match) continue;
  const sound = match[1] as Sound;
  BANK.set(sound, [...(BANK.get(sound) ?? []), { path, url }]);
}

let ctx: AudioContext | null = null;

function context(): AudioContext {
  if (!ctx) ctx = new AudioContext();
  return ctx;
}

let bus: DynamicsCompressorNode | null = null;

/**
 * Общая выходная шина с компрессором-лимитером: хвост рекорда и дивайн
 * могут наложиться, и на громкости ближе к единице без него возможен
 * клиппинг.
 */
function output(): AudioNode {
  const audio = context();
  if (!bus) {
    bus = audio.createDynamicsCompressor();
    bus.threshold.value = -18;
    bus.knee.value = 12;
    bus.ratio.value = 6;
    bus.attack.value = 0.002;
    bus.release.value = 0.12;
    bus.connect(audio.destination);
  }
  return bus;
}

const decoded = new Map<string, Promise<AudioBuffer>>();

/**
 * data-URL → AudioBuffer. Base64 разбираем сами, а не через fetch:
 * CSP страниц (connect-src) не пускает fetch даже к data:.
 */
function load(variant: Variant): Promise<AudioBuffer> {
  let buffer = decoded.get(variant.path);
  if (!buffer) {
    buffer = variant.url().then((url) => {
      const binary = atob(url.slice(url.indexOf(',') + 1));
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      return context().decodeAudioData(bytes.buffer);
    });
    // Не загрузился — следующая попытка начнёт заново.
    buffer.catch(() => decoded.delete(variant.path));
    decoded.set(variant.path, buffer);
  }
  return buffer;
}

const turn = new Map<Sound, number>();

/** Проиграть очередной вариант звука. Ошибки глотаются: звук — украшение. */
function play(sound: Sound, volume: number): void {
  const variants = BANK.get(sound);
  if (!variants || variants.length === 0 || volume <= 0) return;
  const n = turn.get(sound) ?? 0;
  turn.set(sound, n + 1);
  const variant = variants[n % variants.length]!;
  try {
    const audio = context();
    void audio.resume();
    load(variant)
      .then((buffer) => {
        const src = audio.createBufferSource();
        src.buffer = buffer;
        src.playbackRate.value = 1 + (Math.random() * 2 - 1) * 0.005;
        const gain = audio.createGain();
        gain.gain.value = volume;
        src.connect(gain).connect(output());
        src.start();
      })
      .catch(() => {
        // Битый или недекодируемый файл не должен ронять слой.
      });
  } catch {
    /* см. выше */
  }
}

/** Находки звучат не чаще раза в это окно: босс может высыпать пачку дорогого. */
const DROP_GAP_MS = 1500;
let lastDropAt = 0;
let lastDrop: Sound | null = null;

/**
 * Дорогая находка: «возвышение» или «дивайн». Внутри окна второй звук
 * не играет — кроме дивайна после возвышения: более ценное перебивает.
 */
export function playDrop(kind: 'exalted' | 'divine', volume: number): void {
  if (volume <= 0) return;
  const now = Date.now();
  if (now - lastDropAt < DROP_GAP_MS && !(kind === 'divine' && lastDrop === 'exalted')) return;
  lastDropAt = now;
  lastDrop = kind;
  play(kind, volume);
}

/** Рекорд сессии — «Пламя». */
export function playRecord(volume: number): void {
  play('record', volume);
}

/** Пробный звук для ползунка громкости — «возвышение», самый частый из трёх. */
export function playTest(volume: number): void {
  play('exalted', volume);
}

export type SoundPreview = Sound;

/** Прослушивание из настроек: каждый звук как в игре, мимо окна между находками. */
export function playPreview(kind: SoundPreview, volume: number): void {
  play(kind, volume);
}
