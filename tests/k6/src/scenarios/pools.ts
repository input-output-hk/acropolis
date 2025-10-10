import http from 'k6/http';
import { group } from 'k6';
import { ENDPOINTS, buildUrl } from '../config/endpoints';
import { TEST_DATA, getRandomItem } from '../config/test-data';
import { checkResponse } from '../utils/checks';
import { metrics } from '../utils/metrics';
import { getEnv } from '../utils/helpers';

const BASE_URL = getEnv('API_URL', 'http://127.0.0.1:4340');

export function testPoolEndpoints(): void {
  group('Pool Endpoints', () => {
    const poolListResponses = http.batch([
      ['GET', BASE_URL + ENDPOINTS.POOLS, null, { tags: { name: 'list_pools' } }],
      ['GET', BASE_URL + ENDPOINTS.POOLS_EXTENDED, null, { tags: { name: 'pools_extended' } }],
      ['GET', BASE_URL + ENDPOINTS.POOLS_RETIRED, null, { tags: { name: 'pools_retired' } }],
      ['GET', BASE_URL + ENDPOINTS.POOLS_RETIRING, null, { tags: { name: 'pools_retiring' } }],
    ]);

    poolListResponses.forEach((res, i) => {
      const names = [
        'GET /pools',
        'GET /pools/extended',
        'GET /pools/retired',
        'GET /pools/retiring',
      ];
      checkResponse(res, names[i]);
      metrics.poolDuration.add(res.timings.duration);
      metrics.totalRequests.add(1);
    });

    // Test specific pool
    const poolId = getRandomItem(TEST_DATA.poolIds);
    const poolRes = http.get(BASE_URL + buildUrl(ENDPOINTS.POOL, { pool_id: poolId }), {
      tags: { name: 'get_pool' },
    });

    const result = checkResponse(poolRes, 'GET /pools/{pool_id}');
    metrics.poolDuration.add(poolRes.timings.duration);
    metrics.totalRequests.add(1);
    metrics.successfulRequests.add(result.passed ? 1 : 0);

    if (!result.passed) {
      metrics.failedRequests.add(1);
    }
  });
}
