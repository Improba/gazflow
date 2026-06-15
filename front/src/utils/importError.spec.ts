import axios from 'axios';
import { describe, expect, it } from 'vitest';

import { formatImportError } from './importError';

describe('formatImportError', () => {
  it('returns API error payload from axios responses', () => {
    const err = new axios.AxiosError(
      'Request failed',
      'ERR_BAD_REQUEST',
      undefined,
      undefined,
      {
        status: 422,
        statusText: 'Unprocessable Entity',
        headers: {},
        config: { headers: new axios.AxiosHeaders() },
        data: { error: 'nœud orphelin: ORPH01' },
      },
    );

    expect(formatImportError(err)).toBe('nœud orphelin: ORPH01');
  });

  it('falls back to Error.message', () => {
    expect(formatImportError(new Error('fichier manquant'))).toBe('fichier manquant');
  });
});
