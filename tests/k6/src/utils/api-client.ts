import http, { Response } from 'k6/http';
import { checkResponse } from './checks';
import { metrics } from './metrics';

export enum MetricType {
  ACCOUNT = 'account',
  ASSET = 'asset',
  EPOCH = 'epoch',
  GOVERNANCE = 'governance',
  POOL = 'pool',
}

interface RequestOptions {
  endpointName: string;
  tagName: string;
  metricType: MetricType;
}

export class ApiClient {
  constructor(private baseUrl: string) {}

  /**
   * Make a GET request with automatic metrics tracking and checks
   */
  get(url: string, options: RequestOptions): Response {
    const res = http.get(this.baseUrl + url, {
      tags: { name: options.tagName },
    });

    const result = checkResponse(res, options.endpointName);
    this.trackMetrics(res, result.passed, options.metricType);

    return res;
  }

  /**
   * Make a GET request without metrics tracking (for setup/helper requests)
   */
  getRaw(url: string, tagName?: string): Response {
    return http.get(this.baseUrl + url, tagName ? { tags: { name: tagName } } : undefined);
  }

  /**
   * Track metrics for a request
   */
  private trackMetrics(res: Response, passed: boolean, metricType: MetricType): void {
    switch (metricType) {
      case MetricType.ACCOUNT:
        metrics.accountDuration.add(res.timings.duration);
        metrics.accountRequests.add(1);
        if (!passed) {
          metrics.accountErrors.add(1);
        }
        break;
      case MetricType.ASSET:
        metrics.assetDuration.add(res.timings.duration);
        metrics.assetRequests.add(1);
        if (!passed) {
          metrics.assetErrors.add(1);
        }
        break;
      case MetricType.EPOCH:
        metrics.epochDuration.add(res.timings.duration);
        metrics.epochRequests.add(1);
        if (!passed) {
          metrics.epochErrors.add(1);
        }
        break;
      case MetricType.GOVERNANCE:
        metrics.governanceDuration.add(res.timings.duration);
        metrics.governanceRequests.add(1);
        if (!passed) {
          metrics.governanceErrors.add(1);
        }
        break;
      case MetricType.POOL:
        metrics.poolDuration.add(res.timings.duration);
        metrics.poolRequests.add(1);
        if (!passed) {
          metrics.poolErrors.add(1);
        }
        break;
    }

    // Overall metrics
    metrics.totalRequests.add(1);
    metrics.successfulRequests.add(passed ? 1 : 0);

    if (!passed) {
      metrics.failedRequests.add(1);
    }
  }
}

// Singleton instance
const BASE_URL = __ENV.API_URL || 'http://127.0.0.1:4340';
export const apiClient = new ApiClient(BASE_URL);
