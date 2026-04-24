import { ENDPOINTS } from '../config/endpoints';
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
