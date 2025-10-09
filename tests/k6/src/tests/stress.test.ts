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
    { duration: '2m', target: 50 }, // Ramp up to normal load
    { duration: '3m', target: 100 }, // Increase to double
    { duration: '3m', target: 200 }, // Push harder
    { duration: '3m', target: 300 }, // Find the breaking point
    { duration: '2m', target: 0 }, // Ramp down
  ],
  thresholds: {
    ...THRESHOLDS,
    // Relax thresholds slightly for stress test
    http_req_duration: ['p(95)<1200', 'p(99)<2000'],
    http_req_failed: ['rate<0.05'], // Allow 5% error rate
  },
};

export default function () {
  const scenarios: EndpointWeight[] = [
    { name: 'epochs', weight: 30, fn: testEpochEndpoints },
    { name: 'pools', weight: 25, fn: testPoolEndpoints },
    { name: 'assets', weight: 20, fn: testAssetEndpoints },
    { name: 'accounts', weight: 15, fn: testAccountEndpoints },
    { name: 'governance', weight: 10, fn: testGovernanceEndpoints },
  ];

  const selectedScenario = weightedRandomChoice(scenarios);
  selectedScenario();

  randomSleep(0.5, 2); // Faster requests in stress test
}
