import { buildUrl, ENDPOINTS } from '../config/endpoints';
import { getRandomItem, TEST_DATA } from '../config/test-data';
import { apiClient, MetricType } from '../utils/api-client';

export function testGovernanceDReps(): void {
  apiClient.get(ENDPOINTS.GOV_DREPS, {
    endpointName: 'GET /governance/dreps',
    tagName: 'list_dreps',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceDRepDetails(): void {
  const drepId = getRandomItem(TEST_DATA.drepIds);
  const url = buildUrl(ENDPOINTS.GOV_DREP, { drep_id: drepId });

  apiClient.get(url, {
    endpointName: 'GET /governance/dreps/{drep_id}',
    tagName: 'get_drep',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceDRepDelegators(): void {
  const drepId = getRandomItem(TEST_DATA.drepIds);
  const url = buildUrl(ENDPOINTS.GOV_DREP_DELEGATORS, { drep_id: drepId });

  apiClient.get(url, {
    endpointName: 'GET /governance/dreps/{drep_id}/delegators',
    tagName: 'drep_delegators',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceDRepMetadata(): void {
  const drepId = getRandomItem(TEST_DATA.drepIds);
  const url = buildUrl(ENDPOINTS.GOV_DREP_METADATA, { drep_id: drepId });

  apiClient.get(url, {
    endpointName: 'GET /governance/dreps/{drep_id}/metadata',
    tagName: 'drep_metadata',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceDRepUpdates(): void {
  const drepId = getRandomItem(TEST_DATA.drepIds);
  const url = buildUrl(ENDPOINTS.GOV_DREP_UPDATES, { drep_id: drepId });

  apiClient.get(url, {
    endpointName: 'GET /governance/dreps/{drep_id}/updates',
    tagName: 'drep_updates',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceDRepVotes(): void {
  const drepId = getRandomItem(TEST_DATA.drepIds);
  const url = buildUrl(ENDPOINTS.GOV_DREP_VOTES, { drep_id: drepId });

  apiClient.get(url, {
    endpointName: 'GET /governance/dreps/{drep_id}/votes',
    tagName: 'drep_votes',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceProposals(): void {
  apiClient.get(ENDPOINTS.GOV_PROPOSALS, {
    endpointName: 'GET /governance/proposals',
    tagName: 'list_proposals',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceProposalDetails(): void {
  const proposal = getRandomItem(TEST_DATA.proposals);
  const { txHash, certIndex } = proposal;
  const url = buildUrl(ENDPOINTS.GOV_PROPOSAL, {
    tx_hash: txHash,
    cert_index: certIndex.toString(),
  });

  apiClient.get(url, {
    endpointName: 'GET /governance/proposals/{tx_hash}/{cert_index}',
    tagName: 'get_proposal',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceProposalVotes(): void {
  const proposal = getRandomItem(TEST_DATA.proposals);
  const { txHash, certIndex } = proposal;
  const url = buildUrl(ENDPOINTS.GOV_PROPOSAL_VOTES, {
    tx_hash: txHash,
    cert_index: certIndex.toString(),
  });

  apiClient.get(url, {
    endpointName: 'GET /governance/proposals/{tx_hash}/{cert_index}/votes',
    tagName: 'proposal_votes',
    metricType: MetricType.GOVERNANCE,
  });
}

export function testGovernanceProposalMetadata(): void {
  const proposal = getRandomItem(TEST_DATA.proposals);
  const { txHash, certIndex } = proposal;
  const url = buildUrl(ENDPOINTS.GOV_PROPOSAL_METADATA, {
    tx_hash: txHash,
    cert_index: certIndex.toString(),
  });

  apiClient.get(url, {
    endpointName: 'GET /governance/proposals/{tx_hash}/{cert_index}/metadata',
    tagName: 'proposal_metadata',
    metricType: MetricType.GOVERNANCE,
  });
}
