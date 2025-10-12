export const THRESHOLDS = {
  // HTTP request duration (95th and 99th percentiles)
  http_req_duration: ['p(95)<2000', 'p(99)<2200'],

  // Error rate must be below 1%
  http_req_failed: ['rate<0.01'],

  // Endpoint-specific thresholds
  account_duration: ['p(95)<700'],
  asset_duration: ['p(95)<800'],
  epoch_duration: ['p(95)<900'],
  governance_duration: ['p(95)<800'],
  pool_duration: ['p(95)<2000'],

  // Success rate
  successful_requests: ['rate>0.99'],
};

export const SMOKE_THRESHOLDS = {
  http_req_duration: ['p(95)<1500', 'p(99)<2000'],
  http_req_failed: ['rate<0.05'],

  account_duration: ['p(95)<1000'],
  asset_duration: ['p(95)<800'],
  epoch_duration: ['p(95)<1000'],
  governance_duration: ['p(95)<800'],
  pool_duration: ['avg < 1000', 'p(90) < 1500', 'p(95)<2000'],
};
