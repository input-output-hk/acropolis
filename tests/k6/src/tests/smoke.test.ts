import { Options } from 'k6/options';
import { SMOKE_THRESHOLDS } from '../config/thresholds';
import { testGetAccount } from '../scenarios/accounts';
import { testEpochLatest, testEpochParameters } from '../scenarios/epochs';
import { testDRepsList, testProposalsList } from '../scenarios/governance';
import {
  testPoolDetails,
  testPoolsList,
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
  // Test all endpoints enabled by default

  // Accounts
  testGetAccount();

  // Epochs
  testEpochLatest();
  testEpochParameters();

  // Pools
  testPoolsList();
  testPoolDetails();
  testPoolsRetiring();

  // Governance
  testDRepsList();
  testProposalsList();

  randomSleep(1, 2);
}
