import { ENDPOINTS } from '../config/endpoints';
import { getRandomItem } from '../config/test-data';
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

export function testPoolsExtended(): void {
  apiClient.get(ENDPOINTS.POOLS_EXTENDED, {
    endpointName: 'GET /pools/extended',
    tagName: 'pools_extended',
    metricType: MetricType.POOL,
  });
}

export function testPoolsRetired(): void {
  apiClient.get(ENDPOINTS.POOLS_RETIRED, {
    endpointName: 'GET /pools/retired',
    tagName: 'pools_retired',
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
  const poolId = getRandomItem(TEST_DATA.poolIds);
  const url = buildUrl(ENDPOINTS.POOL, { pool_id: poolId });

  apiClient.get(url, {
    endpointName: 'GET /pools/{pool_id}',
    tagName: 'get_pool',
    metricType: MetricType.POOL,
  });
}