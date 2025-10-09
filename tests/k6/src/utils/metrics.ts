import { Trend, Rate, Counter } from 'k6/metrics';

export const metrics = {
  // Duration metrics per endpoint category
  accountDuration: new Trend('account_duration'),
  assetDuration: new Trend('asset_duration'),
  epochDuration: new Trend('epoch_duration'),
  governanceDuration: new Trend('governance_duration'),
  poolDuration: new Trend('pool_duration'),

  // Error tracking
  errorRate: new Rate('error_rate'),
  successfulRequests: new Rate('successful_requests'),

  // Counters
  failedRequests: new Counter('failed_requests'),
  totalRequests: new Counter('total_requests'),
};
