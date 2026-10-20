/** Состояние обновления — договор с приложением (типы из main/updater.ts эталона). */
export interface UpdateState {
  /** Обновления в принципе возможны: только для установленной версии. */
  supported: boolean;
  status: 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error';
  version: string | null;
  /** Доля загруженного, 0…1. */
  progress: number;
  error: string | null;
}
