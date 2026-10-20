interface ImportMetaEnv {
  readonly DEV: boolean;
  readonly PROD: boolean;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
  /** Vite собирает файлы по шаблону на этапе сборки. */
  glob<T>(
    pattern: string,
    options: { query?: string; import?: string; eager: true },
  ): Record<string, T>;
  /** Без eager — каждый файл в своём чанке, загрузка по требованию. */
  glob<T>(
    pattern: string,
    options: { query?: string; import?: string; eager?: false },
  ): Record<string, () => Promise<T>>;
}

declare module '*.css' {
  const content: string;
  export default content;
}

/** Vite отдаёт импортированные ассеты как URL-строки. */
declare module '*.webp' {
  const src: string;
  export default src;
}

declare module '*.png' {
  const src: string;
  export default src;
}

declare module '*.svg' {
  const src: string;
  export default src;
}
