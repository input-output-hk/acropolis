import { buildUrl, ENDPOINTS } from '../config/endpoints';
import { getRandomItem, TEST_DATA } from '../config/test-data';
import { apiClient, MetricType } from '../utils/api-client';

export function testAssetsList(): void {
  apiClient.get(ENDPOINTS.ASSETS, {
    endpointName: 'GET /assets',
    tagName: 'list_assets',
    metricType: 'asset',
  });
}

export function testAssetDetails(): void {
  const assetId = getRandomItem(TEST_DATA.assetIds);
  const url = buildUrl(ENDPOINTS.ASSET, { asset: assetId });

  apiClient.get(url, {
    endpointName: 'GET /assets/{asset}',
    tagName: 'get_asset',
    metricType: MetricType.ASSET,
  });
}

export function testAssetHistory(): void {
  const assetId = getRandomItem(TEST_DATA.assetIds);
  const url = buildUrl(ENDPOINTS.ASSET_HISTORY, { asset: assetId });

  apiClient.get(url, {
    endpointName: 'GET /assets/{asset}/history',
    tagName: 'asset_history',
    metricType: MetricType.ASSET,
  });
}

export function testAssetTransactions(): void {
  const assetId = getRandomItem(TEST_DATA.assetIds);
  const url = buildUrl(ENDPOINTS.ASSET_TRANSACTIONS, { asset: assetId });

  apiClient.get(url, {
    endpointName: 'GET /assets/{asset}/transactions',
    tagName: 'asset_transactions',
    metricType: MetricType.ASSET,
  });
}

export function testAssetAddresses(): void {
  const assetId = getRandomItem(TEST_DATA.assetIds);
  const url = buildUrl(ENDPOINTS.ASSET_ADDRESSES, { asset: assetId });

  apiClient.get(url, {
    endpointName: 'GET /assets/{asset}/addresses',
    tagName: 'asset_addresses',
    metricType: MetricType.ASSET,
  });
}

export function testAssetPolicy(): void {
  const policyId = getRandomItem(TEST_DATA.policyIds);
  const url = buildUrl(ENDPOINTS.ASSET_POLICY, { policy_id: policyId });

  apiClient.get(url, {
    endpointName: 'GET /assets/policy/{policy_id}',
    tagName: 'asset_policy',
    metricType: 'asset',
  });
}

export function testAssetEndpoints(): void {
  const tests = [
    testAssetsList,
    testAssetDetails,
    testAssetHistory,
    testAssetTransactions,
    testAssetAddresses,
    testAssetPolicy,
  ];
  const randomTest = tests[Math.floor(Math.random() * tests.length)];
  randomTest();
}
