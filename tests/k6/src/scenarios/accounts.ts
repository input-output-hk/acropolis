import { ENDPOINTS } from '../config/endpoints';
import { getRandomItem } from '../config/test-data';
import { TEST_DATA } from '../config/shelley-test-data';
import { apiClient, MetricType } from '../utils/api-client';
import { buildUrl } from '../utils/helpers';

export function testGetAccount(): void {
  const stakeAddress = getRandomItem(TEST_DATA.stakeAddresses);
  const url = buildUrl(ENDPOINTS.ACCOUNT, { stake_address: stakeAddress });

  apiClient.get(url, {
    endpointName: 'GET /accounts/{stake_address}',
    tagName: 'get_account',
    metricType: MetricType.ACCOUNT,
  });
}
