export const THRESHOLDS = {
  // HTTP request duration (95th and 99th percentiles)
  http_req_duration: ['p(95)<800', 'p(99)<1500'],

  // Error rate must be below 1%
  http_req_failed: ['rate<0.01'],

  // Endpoint-specific thresholds
  account_duration: ['p(95)<500'],
  asset_duration: ['p(95)<600'],
  epoch_duration: ['p(95)<400'],
  governance_duration: ['p(95)<700'],
  pool_duration: ['p(95)<500'],

  // Success rate
  successful_requests: ['rate>0.99'],
};

export const SMOKE_THRESHOLDS = {
  http_req_duration: ['p(95)<1000'],
  http_req_failed: ['rate<0.05'],
};
