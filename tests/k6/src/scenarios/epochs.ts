import http from 'k6/http';
import { group } from 'k6';
import { ENDPOINTS, buildUrl } from '../config/endpoints';
import { checkResponse } from '../utils/checks';
import { metrics } from '../utils/metrics';
import { getEnv } from '../utils/helpers';

const BASE_URL = getEnv('API_URL', 'http://127.0.0.1:4340');

export function testEpochEndpoints(): void {
  group('Epoch Endpoints', () => {
    const latestRes = http.get(BASE_URL + ENDPOINTS.EPOCHS_LATEST, {
      tags: { name: 'latest_epoch' },
    });
    const latestCheck = checkResponse(latestRes, 'GET /epochs/latest');

    metrics.epochDuration.add(latestRes.timings.duration);
    metrics.totalRequests.add(1);
    metrics.successfulRequests.add(latestCheck.passed ? 1 : 0);

    const paramsRes = http.get(BASE_URL + ENDPOINTS.EPOCHS_LATEST_PARAMETERS, {
      tags: { name: 'epoch_parameters' },
    });
    checkResponse(paramsRes, 'GET /epochs/latest/parameters');

    metrics.epochDuration.add(paramsRes.timings.duration);
    metrics.totalRequests.add(1);

    if (latestCheck.passed && latestRes.json('epoch')) {
      const epochNo = latestRes.json('epoch') as number;
      const specificRes = http.get(
        BASE_URL + buildUrl(ENDPOINTS.EPOCH, { epoch_no: (epochNo - 1).toString() }),
        { tags: { name: 'specific_epoch' } },
      );
      checkResponse(specificRes, 'GET /epochs/{epoch_no}');
      metrics.epochDuration.add(specificRes.timings.duration);
      metrics.totalRequests.add(1);
    }
  });
}
