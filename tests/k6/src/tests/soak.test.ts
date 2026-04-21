import { Options } from 'k6/options';
import { THRESHOLDS } from '../config/thresholds';
import { testGetAccount } from '../scenarios/accounts';
import { testEpochLatest, testEpochParameters } from '../scenarios/epochs';
import { testDRepsList, testProposalsList } from '../scenarios/governance';
import {
  testPoolDetails,
  testPoolsList,
  testPoolsRetiring,
} from '../scenarios/pools';
import { randomSleep, weightedRandomChoice } from '../utils/helpers';
import { EndpointWeight } from '../types';

export const options: Options = {
  stages: [
    { duration: '5m', target: 30 },
    { duration: '2h', target: 30 },
    { duration: '5m', target: 0 },
  ],
  thresholds: THRESHOLDS,
};

export default function () {
  const scenarios: EndpointWeight[] = [
    // Test all endpoints enabled by default

    // Accounts
    { name: 'accounts', weight: 40, fn: testGetAccount },

    // Epochs
    { name: 'epoch_latest', weight: 20, fn: testEpochLatest },
    { name: 'epoch_params', weight: 7, fn: testEpochParameters },

    // Pools
    { name: 'pools_list', weight: 10, fn: testPoolsList },
    { name: 'pools_details', weight: 12, fn: testPoolDetails },
    { name: 'pools_retiring', weight: 2, fn: testPoolsRetiring },

    // Governance
    { name: 'dreps_list', weight: 3, fn: testDRepsList },
    { name: 'proposals_list', weight: 2, fn: testProposalsList },
  ];

  const selectedScenario = weightedRandomChoice(scenarios);
  selectedScenario();

  randomSleep(2, 4);
}
