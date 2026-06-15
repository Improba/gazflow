import axios from 'axios';

export function formatImportError(err: unknown): string {
  if (axios.isAxiosError(err)) {
    const data = err.response?.data as { error?: string } | undefined;
    if (data?.error) return data.error;
  }
  if (err instanceof Error) return err.message;
  return String(err);
}

/** Alias sémantique pour les erreurs REST/WebSocket (même extraction payload `{ error }`). */
export const formatApiError = formatImportError;
