import http from 'k6/http';
import { group } from 'k6';
import { ENDPOINTS, buildUrl } from '../config/endpoints';
import { TEST_DATA, getRandomItem } from '../config/test-data';
import { checkResponse } from '../utils/checks';
import { metrics } from '../utils/metrics';
import { getEnv } from '../utils/helpers';

const BASE_URL = getEnv('API_URL', 'http://127.0.0.1:4340');

export function testGovernanceEndpoints(): void {
  group('Governance Endpoints', () => {
    const drepsRes = http.get(BASE_URL + ENDPOINTS.GOV_DREPS, {
      tags: { name: 'list_dreps' },
    });
    checkResponse(drepsRes, 'GET /governance/dreps');
    metrics.governanceDuration.add(drepsRes.timings.duration);
    metrics.totalRequests.add(1);

    const drepId = getRandomItem(TEST_DATA.drepIds);
    const drepResponses = http.batch([
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.GOV_DREP, { drep_id: drepId }),
        null,
        { tags: { name: 'get_drep' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.GOV_DREP_DELEGATORS, { drep_id: drepId }),
        null,
        { tags: { name: 'drep_delegators' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.GOV_DREP_METADATA, { drep_id: drepId }),
        null,
        { tags: { name: 'drep_metadata' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.GOV_DREP_UPDATES, { drep_id: drepId }),
        null,
        { tags: { name: 'drep_updates' } },
      ],
      [
        'GET',
        BASE_URL + buildUrl(ENDPOINTS.GOV_DREP_VOTES, { drep_id: drepId }),
        null,
        { tags: { name: 'drep_votes' } },
      ],
    ]);

    drepResponses.forEach((res) => {
      metrics.governanceDuration.add(res.timings.duration);
      metrics.totalRequests.add(1);
    });

    const proposalsRes = http.get(BASE_URL + ENDPOINTS.GOV_PROPOSALS, {
      tags: { name: 'list_proposals' },
    });
    checkResponse(proposalsRes, 'GET /governance/proposals');
    metrics.governanceDuration.add(proposalsRes.timings.duration);
    metrics.totalRequests.add(1);

    const proposal = getRandomItem(TEST_DATA.proposals);
    const { txHash, certIndex } = proposal;

    const proposalResponses = http.batch([
      [
        'GET',
        BASE_URL +
          buildUrl(ENDPOINTS.GOV_PROPOSAL, {
            tx_hash: txHash,
            cert_index: certIndex.toString(),
          }),
        null,
        { tags: { name: 'get_proposal' } },
      ],
      [
        'GET',
        BASE_URL +
          buildUrl(ENDPOINTS.GOV_PROPOSAL_VOTES, {
            tx_hash: txHash,
            cert_index: certIndex.toString(),
          }),
        null,
        { tags: { name: 'proposal_votes' } },
      ],
      [
        'GET',
        BASE_URL +
          buildUrl(ENDPOINTS.GOV_PROPOSAL_METADATA, {
            tx_hash: txHash,
            cert_index: certIndex.toString(),
          }),
        null,
        { tags: { name: 'proposal_metadata' } },
      ],
    ]);

    proposalResponses.forEach((res) => {
      metrics.governanceDuration.add(res.timings.duration);
      metrics.totalRequests.add(1);
    });
  });
}
