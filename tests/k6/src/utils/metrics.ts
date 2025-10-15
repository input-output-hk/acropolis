import { Trend, Rate, Counter } from 'k6/metrics';

export const metrics = {
  // Duration metrics per endpoint category
  accountDuration: new Trend('account_duration'),
  assetDuration: new Trend('asset_duration'),
  epochDuration: new Trend('epoch_duration'),
  governanceDuration: new Trend('governance_duration'),
  poolDuration: new Trend('pool_duration'),

  // Request counts per endpoint category
  accountRequests: new Counter('account_requests'),
  assetRequests: new Counter('asset_requests'),
  epochRequests: new Counter('epoch_requests'),
  governanceRequests: new Counter('governance_requests'),
  poolRequests: new Counter('pool_requests'),

  // Errors per endpoint category
  accountErrors: new Counter('account_errors'),
  assetErrors: new Counter('asset_errors'),
  epochErrors: new Counter('epoch_errors'),
  governanceErrors: new Counter('governance_errors'),
  poolErrors: new Counter('pool_errors'),

  // Error tracking
  errorRate: new Rate('error_rate'),
  successfulRequests: new Rate('successful_requests'),

  // Counters
  failedRequests: new Counter('failed_requests'),
  totalRequests: new Counter('total_requests'),
};
