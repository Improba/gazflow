import { useDemandProfilesStore } from 'src/stores/demandProfiles';
import { useNetworkStore } from 'src/stores/network';
import { useSimulateStore } from 'src/stores/simulate';
import {
  profileFromCategory,
  resolveDemands,
  type DayType,
  type DemandProfileDto,
} from 'src/utils/demandProfiles';

export const DEMO_NETWORK_ID = 'GasLib-11';
export const DEMO_T_EXT_C = -5;
export const DEMO_HOUR = 7;
export const DEMO_DAY_TYPE: DayType = 'weekday';
export const DEMO_DESCRIPTION =
  'Cas démo hiver — GasLib-11, 7 h, −5 °C, profils résidentiels';

export async function runDemoCase(): Promise<void> {
  const networkStore = useNetworkStore();
  const demandProfilesStore = useDemandProfilesStore();
  const simulateStore = useSimulateStore();

  await networkStore.selectNetwork(DEMO_NETWORK_ID);
  demandProfilesStore.load(DEMO_NETWORK_ID);

  const profiles: Record<string, DemandProfileDto> = {};
  for (const node of networkStore.nodes) {
    if (node.pressure_fixed_bar != null) {
      continue;
    }
    const profile = profileFromCategory('residential', DEMO_DAY_TYPE);
    demandProfilesStore.setProfile(node.id, profile, DEMO_NETWORK_ID);
    profiles[node.id] = { ...profile };
  }

  const demands = resolveDemands(profiles, DEMO_T_EXT_C, DEMO_HOUR);
  simulateStore.setRunScenarioSummary({
    tExtC: DEMO_T_EXT_C,
    hour: DEMO_HOUR,
    dayType: DEMO_DAY_TYPE,
    description: DEMO_DESCRIPTION,
  });

  await simulateStore.runSimulation(demands, {
    gas_composition: { ...networkStore.gas.composition },
  });
}
