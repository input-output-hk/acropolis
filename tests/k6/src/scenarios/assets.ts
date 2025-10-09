import http from 'k6/http';
import { group } from 'k6';
import { ENDPOINTS, buildUrl } from '../config/endpoints';
import { TEST_DATA, getRandomItem } from '../config/test-data';
import { checkResponse, checkBatchResponses } from '../utils/checks';
import { metrics } from '../utils/metrics';
import { getEnv } from '../utils/helpers';

const BASE_URL = getEnv('API_URL', 'http://127.0.0.1:4340');

export function testAssetEndpoints(): void {
  group('Asset Endpoints', () => {
    // Test asset list
    const listRes = http.get(BASE_URL + ENDPOINTS.ASSETS, {
      tags: { name: 'list_assets' },
    });
    checkResponse(listRes, 'GET /assets');

    // Test specific asset with batch requests
    const assetId = getRandomItem(TEST_DATA.assetIds);
    const policyId = getRandomItem(TEST_DATA.policyIds);

    const responses = http.batch([
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.ASSET, { asset: assetId }),
        null,
        { tags: { name: 'get_asset' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.ASSET_HISTORY, { asset: assetId }),
        null,
        { tags: { name: 'asset_history' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.ASSET_TRANSACTIONS, { asset: assetId }),
        null,
        { tags: { name: 'asset_txs' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.ASSET_ADDRESSES, { asset: assetId }),
        null,
        { tags: { name: 'asset_addresses' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.ASSET_POLICY, { policy_id: policyId }),
        null,
        { tags: { name: 'asset_policy' } },
      ],
    ]);

    const results = checkBatchResponses(responses, [
      'GET /assets/{asset}',
      'GET /assets/{asset}/history',
      'GET /assets/{asset}/transactions',
      'GET /assets/{asset}/addresses',
      'GET /assets/policy/{policy_id}',
    ]);

    responses.forEach((res) => metrics.assetDuration.add(res.timings.duration));
    metrics.totalRequests.add(responses.length);

    results.forEach((result) => {
      metrics.successfulRequests.add(result.passed ? 1 : 0);
      metrics.errorRate.add(!result.passed ? 1 : 0);
      if (!result.passed) metrics.failedRequests.add(1);
    });
  });
}
