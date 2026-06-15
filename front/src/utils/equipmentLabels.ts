/** Libellés et couleurs pour les organes P8 (alignés sur ConnectionKind camelCase API). */

export const EQUIPMENT_KIND_LABELS: Record<string, string> = {
  pressureRegulator: 'Détendeur / régulateur',
  deliveryStation: 'Poste de livraison',
  controlValve: 'Vanne de régulation',
  valve: 'Vanne',
  compressorStation: 'Compresseur',
  shortPipe: 'Liaison courte',
  resistor: 'Résistance',
  pipe: 'Canalisation',
};

export function equipmentKindLabel(kind: string): string {
  return EQUIPMENT_KIND_LABELS[kind] ?? kind;
}

export function isEquipmentKind(kind: string): boolean {
  return (
    kind === 'pressureRegulator' ||
    kind === 'deliveryStation' ||
    kind === 'controlValve' ||
    kind === 'valve' ||
    kind === 'compressorStation'
  );
}

export function regulatorModeLabel(mode: string): string {
  if (mode === 'active') return 'Actif (consigne aval)';
  if (mode === 'bypass') return 'Bypass (amont insuffisant)';
  return mode;
}

/** Couleur Cesium (#RRGGBB) pour marqueur carte. */
export function equipmentMarkerColor(kind: string): string {
  switch (kind) {
    case 'pressureRegulator':
      return '#FFD54F';
    case 'deliveryStation':
      return '#69F0AE';
    case 'controlValve':
      return '#EA80FC';
    case 'valve':
      return '#FF5252';
    case 'compressorStation':
      return '#40C4FF';
    default:
      return '#FFB74D';
  }
}
