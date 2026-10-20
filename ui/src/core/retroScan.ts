/** Прогресс ретро-скана — договор с приложением (из core/retroScan.ts эталона). */
export interface RetroScanProgress {
  file: string;
  index: number;
  total: number;
  /** Забегов найдено к началу этого файла — живая цифра для заглушки. */
  runsFound: number;
}
