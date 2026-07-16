import type { NovaVerdict, SinkDiagnostic } from 'src/services/api';

/** IDs des sinks déficitaires : diagnostics du dernier run, puis verdict NoVa. */
export function deficitSinkIds(
  sinkDiagnostics: SinkDiagnostic[],
  novaVerdict: NovaVerdict | null,
): string[] {
  const fromDiagnostics = sinkDiagnostics.map((d) => d.node_id);
  if (fromDiagnostics.length > 0) return fromDiagnostics;
  return novaVerdict?.deficit_sinks ?? [];
}
