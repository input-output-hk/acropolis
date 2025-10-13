import { Options } from 'k6/options';
import { SMOKE_THRESHOLDS } from '../config/thresholds';
import { testAccountEndpoints } from '../scenarios/accounts';
import { testEpochLatest, testEpochParameters } from '../scenarios/epochs';
import {
  testPoolDetails,
  testPoolsExtended,
  testPoolsList,
  testPoolsRetired,
  testPoolsRetiring,
} from '../scenarios/pools';
import { randomSleep } from '../utils/helpers';

export const options: Options = {
  scenarios: {
    smoke: {
      vus: 3,
      executor: 'externally-controlled',
      duration: '1m',
    },
  },
  thresholds: SMOKE_THRESHOLDS,
};

export default function () {
  // Accounts
  testAccountEndpoints();

  // Epochs
  testEpochLatest();
  testEpochParameters();

  // Pools
  testPoolsList();
  testPoolDetails();

  // Pools extended
  testPoolsExtended();
  testPoolsRetired();
  testPoolsRetiring();

  // Assets - 20% (uncomment when ready)
  // { name: 'assets_list', weight: 5, fn: testAssetsList },
  // { name: 'assets_details', weight: 7, fn: testAssetDetails },
  // { name: 'assets_history', weight: 3, fn: testAssetHistory },
  // { name: 'assets_transactions', weight: 2, fn: testAssetTransactions },
  // { name: 'assets_addresses', weight: 2, fn: testAssetAddresses },
  // { name: 'assets_policy', weight: 1, fn: testAssetPolicy },

  // Governance - 10% (uncomment when ready)
  // { name: 'gov_dreps', weight: 3, fn: testGovernanceDReps },
  // { name: 'gov_drep_details', weight: 2, fn: testGovernanceDRepDetails },
  // { name: 'gov_drep_delegators', weight: 1, fn: testGovernanceDRepDelegators },
  // { name: 'gov_drep_metadata', weight: 1, fn: testGovernanceDRepMetadata },
  // { name: 'gov_drep_updates', weight: 1, fn: testGovernanceDRepUpdates },
  // { name: 'gov_drep_votes', weight: 1, fn: testGovernanceDRepVotes },
  // { name: 'gov_proposals', weight: 1, fn: testGovernanceProposals },

  randomSleep(1, 2);
}
