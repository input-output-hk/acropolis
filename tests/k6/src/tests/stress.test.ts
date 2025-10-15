import { Options } from 'k6/options';
import { THRESHOLDS } from '../config/thresholds';
import { testGetAccount } from '../scenarios/accounts';
import { testEpochLatest, testEpochParameters, testEpochSpecific } from '../scenarios/epochs';
import {
  testPoolDetails,
  testPoolsExtended,
  testPoolsList,
  testPoolsRetired,
  testPoolsRetiring,
} from '../scenarios/pools';
import { randomSleep, weightedRandomChoice } from '../utils/helpers';
import { EndpointWeight } from '../types';

export const options: Options = {
  // These are just some hypothetical stages for a stress test
  // They need to be adjusted based on the actual system capacity and goals for the
  // Blockfrost API.
  stages: [
    { duration: '2m', target: 50 },
    { duration: '3m', target: 100 },
    { duration: '3m', target: 200 },
    { duration: '3m', target: 300 },
    { duration: '2m', target: 0 },
  ],
  thresholds: {
    ...THRESHOLDS,
    // Relax thresholds for stress test - we expect some degradation
    http_req_duration: ['p(95)<1500', 'p(99)<3000'],
    http_req_failed: ['rate<0.05'],
  },
};

export default function () {
  const scenarios: EndpointWeight[] = [
    // Accounts
    { name: 'accounts', weight: 40, fn: testGetAccount },

    // Epochs
    { name: 'epoch_latest', weight: 20, fn: testEpochLatest },
    { name: 'epoch_params', weight: 7, fn: testEpochParameters },
    { name: 'epoch_specific', weight: 3, fn: testEpochSpecific },

    // Pools
    { name: 'pools_list', weight: 10, fn: testPoolsList },
    { name: 'pools_details', weight: 12, fn: testPoolDetails },
    { name: 'pools_extended', weight: 4, fn: testPoolsExtended },
    { name: 'pools_retired', weight: 2, fn: testPoolsRetired },
    { name: 'pools_retiring', weight: 2, fn: testPoolsRetiring },

    // Assets
    // { name: 'assets_list', weight: 5, fn: testAssetsList },
    // { name: 'assets_details', weight: 7, fn: testAssetDetails },
    // { name: 'assets_history', weight: 3, fn: testAssetHistory },
    // { name: 'assets_transactions', weight: 2, fn: testAssetTransactions },
    // { name: 'assets_addresses', weight: 2, fn: testAssetAddresses },
    // { name: 'assets_policy', weight: 1, fn: testAssetPolicy },

    // Governance
    // { name: 'gov_dreps', weight: 3, fn: testGovernanceDReps },
    // { name: 'gov_drep_details', weight: 2, fn: testGovernanceDRepDetails },
    // { name: 'gov_drep_delegators', weight: 1, fn: testGovernanceDRepDelegators },
    // { name: 'gov_drep_metadata', weight: 1, fn: testGovernanceDRepMetadata },
    // { name: 'gov_drep_updates', weight: 1, fn: testGovernanceDRepUpdates },
    // { name: 'gov_drep_votes', weight: 1, fn: testGovernanceDRepVotes },
    // { name: 'gov_proposals', weight: 1, fn: testGovernanceProposals },
  ];

  const selectedScenario = weightedRandomChoice(scenarios);
  selectedScenario();

  randomSleep(0.5, 2);
}