import { buildUrl, ENDPOINTS } from '../config/endpoints';
import { apiClient, MetricType } from '../utils/api-client';

export function testEpochLatest(): void {
  apiClient.get(ENDPOINTS.EPOCHS_LATEST, {
    endpointName: 'GET /epochs/latest',
    tagName: 'epoch_latest',
    metricType: MetricType.EPOCH,
  });
}

export function testEpochParameters(): void {
  apiClient.get(ENDPOINTS.EPOCHS_LATEST_PARAMETERS, {
    endpointName: 'GET /epochs/latest/parameters',
    tagName: 'epoch_parameters',
    metricType: MetricType.EPOCH,
  });
}

export function testEpochSpecific(): void {
  const latestRes = apiClient.getRaw(ENDPOINTS.EPOCHS_LATEST);

  if (latestRes.status === 200 && latestRes.json('epoch')) {
    const epochNo = latestRes.json('epoch') as number;
    const url = buildUrl(ENDPOINTS.EPOCH, { epoch_no: (epochNo - 1).toString() });

    apiClient.get(url, {
      endpointName: 'GET /epochs/{epoch_no}',
      tagName: 'epoch_specific',
      metricType: MetricType.EPOCH,
    });
  }
}

export function testEpochEndpoints(): void {
  const tests = [testEpochLatest, testEpochParameters, testEpochSpecific];
  const randomTest = tests[Math.floor(Math.random() * tests.length)];
  randomTest();
}
