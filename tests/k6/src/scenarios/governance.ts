import { ENDPOINTS } from '../config/endpoints';
import { apiClient, MetricType } from '../utils/api-client';

export function testDRepsList(): void {
  apiClient.get(ENDPOINTS.GOV_DREPS, {
    endpointName: 'GET /governance/dreps',
    tagName: 'list_dreps',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testProposalsList(): void {
  apiClient.get(ENDPOINTS.GOV_PROPOSALS, {
    endpointName: 'GET /governance/proposals',
    tagName: 'list_proposals',
    metricType: MetricType.GOVERNANCE,
  });
}