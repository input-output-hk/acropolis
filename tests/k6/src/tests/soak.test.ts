import { Options } from 'k6/options';
import { THRESHOLDS } from '../config/thresholds';
import { testAccountEndpoints } from '../scenarios/accounts';
import { testAssetEndpoints } from '../scenarios/assets';
import { testEpochEndpoints } from '../scenarios/epochs';
import { testGovernanceEndpoints } from '../scenarios/governance';
import { testPoolEndpoints } from '../scenarios/pools';
import { weightedRandomChoice, randomSleep } from '../utils/helpers';
import { EndpointWeight } from '../types';

export const options: Options = {
  stages: [
    { duration: '5m', target: 30 }, // Ramp up
    { duration: '2h', target: 30 }, // Stay for 2 hours
    { duration: '5m', target: 0 }, // Ramp down
  ],
  thresholds: THRESHOLDS,
};

export default function () {
  const scenarios: EndpointWeight[] = [
    { name: 'epochs', weight: 30, fn: testEpochEndpoints },
    { name: 'pools', weight: 30, fn: testPoolEndpoints },
    // { name: 'assets', weight: 20, fn: testAssetEndpoints },
    { name: 'accounts', weight: 40, fn: testAccountEndpoints },
    // { name: 'governance', weight: 10, fn: testGovernanceEndpoints },
  ];

  const selectedScenario = weightedRandomChoice(scenarios);
  selectedScenario();

  randomSleep(2, 4); // Longer think time for soak test
}
