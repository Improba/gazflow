import { readFileSync } from 'node:fs';
import { describe, expect, it, vi } from 'vitest';

vi.mock('#q-app/wrappers', () => ({
  defineBoot: (bootFn: unknown) => bootFn,
}));

import piniaBoot from 'src/boot/pinia';

describe('dev integration safeguards', () => {
  it('keeps pinia boot registered in Quasar config', () => {
    const configSource = readFileSync(new URL('../../quasar.config.ts', import.meta.url), 'utf8');
    expect(configSource).toMatch(/boot:\s*\[[^\]]*['"]pinia['"][^\]]*]/s);
  });

  it('keeps websocket proxy enabled for /api', () => {
    const configSource = readFileSync(new URL('../../quasar.config.ts', import.meta.url), 'utf8');
    expect(configSource).toMatch(/['"]\/api['"]\s*:\s*\{[\s\S]*?ws:\s*true[\s\S]*?\}/m);
  });

  it('registers pinia plugin during boot', () => {
    const app = { use: vi.fn() };

    piniaBoot({ app } as never);

    expect(app.use).toHaveBeenCalledTimes(1);
  });
});
