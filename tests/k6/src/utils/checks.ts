import { Response } from 'k6/http';
import { check } from 'k6';
import { CheckResult } from '../types';

function hasResponseBody(r: Response): boolean {
  if (!r.body) return false;
  if (typeof r.body === 'string') return r.body.length > 0;
  return r.body.byteLength > 0;
}

export function checkResponse(
  res: Response,
  endpointName: string,
  expectedStatus: number = 200,
): CheckResult {
  const checks = {
    [`${endpointName} - status is ${expectedStatus}`]: (r: Response): boolean =>
      r.status === expectedStatus,
    [`${endpointName} - has response body`]: (r: Response): boolean => hasResponseBody(r),
  };

  const passed = check(res, checks);

  return {
    passed: passed,
    endpoint: endpointName,
    statusCode: res.status,
    duration: res.timings.duration,
  };
}
