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
    { duration: '2m', target: 20 }, // Ramp up
    { duration: '5m', target: 20 }, // Stay at 20 users
    { duration: '2m', target: 50 }, // Ramp to 50
    { duration: '5m', target: 50 }, // Stay at 50
    { duration: '2m', target: 0 }, // Ramp down
  ],
  thresholds: THRESHOLDS,
};

export default function () {
  // I want to weight the distribution based on expected usage patterns,
  // ideally needs to be data driven eventually.
  const scenarios: EndpointWeight[] = [
    { name: 'epochs', weight: 30, fn: testEpochEndpoints },
    { name: 'pools', weight: 30, fn: testPoolEndpoints },
    // { name: 'assets', weight: 20, fn: testAssetEndpoints },
    { name: 'accounts', weight: 40, fn: testAccountEndpoints },
    // { name: 'governance', weight: 10, fn: testGovernanceEndpoints },
  ];

  const selectedScenario = weightedRandomChoice(scenarios);
  selectedScenario();

  randomSleep(1, 3);
}
