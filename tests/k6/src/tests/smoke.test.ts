import { Options } from 'k6/options';
import { SMOKE_THRESHOLDS } from '../config/thresholds';
import { testAccountEndpoints } from '../scenarios/accounts';
import { testAssetEndpoints } from '../scenarios/assets';
import { testEpochEndpoints } from '../scenarios/epochs';
import { testGovernanceEndpoints } from '../scenarios/governance';
import { testPoolEndpoints } from '../scenarios/pools';
import { randomSleep } from '../utils/helpers';

export const options: Options = {
  vus: 5,
  duration: '1m',
  thresholds: SMOKE_THRESHOLDS,
};

export default function () {
  testAccountEndpoints();
  // testAssetEndpoints();
  testEpochEndpoints();
  // testGovernanceEndpoints();
  testPoolEndpoints();

  randomSleep(1, 2);
}
