import { ENDPOINTS } from '../config/endpoints';
import { apiClient, MetricType } from '../utils/api-client';
import { TEST_DATA } from '../config/shelley-test-data';
import { buildUrl } from '../utils/helpers';

export function testPoolsList(): void {
  apiClient.get(ENDPOINTS.POOLS, {
    endpointName: 'GET /pools',
    tagName: 'list_pools',
    metricType: MetricType.POOL,
  });
}

export function testPoolsRetiring(): void {
  apiClient.get(ENDPOINTS.POOLS_RETIRING, {
    endpointName: 'GET /pools/retiring',
    tagName: 'pools_retiring',
    metricType: MetricType.POOL,
  });
}

export function testPoolDetails(): void {
  const poolId = TEST_DATA.poolIds[0];
  const url = buildUrl(ENDPOINTS.POOL, { pool_id: poolId });

  apiClient.get(url, {
    endpointName: 'GET /pools/{pool_id}',
    tagName: 'get_pool',
    metricType: MetricType.POOL,
  });
}