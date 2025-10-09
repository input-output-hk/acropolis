import http from 'k6/http';
import { group } from 'k6';
import { ENDPOINTS, buildUrl } from '../config/endpoints';
import { TEST_DATA, getRandomItem } from '../config/test-data';
import { checkResponse } from '../utils/checks';
import { metrics } from '../utils/metrics';
import { getEnv } from '../utils/helpers';

const BASE_URL = getEnv('API_URL', 'http://127.0.0.1:4340');

export function testAccountEndpoints(): void {
  group('Account Endpoints', () => {
    const stakeAddress = getRandomItem(TEST_DATA.stakeAddresses);
    const url = BASE_URL + buildUrl(ENDPOINTS.ACCOUNT, { stake_address: stakeAddress });

    const res = http.get(url, { tags: { name: 'get_account' } });
    const result = checkResponse(res, 'GET /accounts/{stake_address}');

    metrics.accountDuration.add(res.timings.duration);
    metrics.totalRequests.add(1);
    metrics.successfulRequests.add(result.passed ? 1 : 0);
    metrics.errorRate.add(!result.passed ? 1 : 0);

    if (!result.passed) {
      metrics.failedRequests.add(1);
    }
  });
}
