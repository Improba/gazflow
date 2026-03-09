import { describe, expect, it } from 'vitest';

import { buildWsUrlForOrigin } from './ws';

describe('buildWsUrlForOrigin', () => {
  it('maps http origin to ws url', () => {
    expect(buildWsUrlForOrigin('http://localhost:9000', '/api/ws/sim'))
      .toBe('ws://localhost:9000/api/ws/sim');
  });

  it('maps https origin to wss url', () => {
    expect(buildWsUrlForOrigin('https://gazsim.example.com', '/api/ws/sim'))
      .toBe('wss://gazsim.example.com/api/ws/sim');
  });
});
