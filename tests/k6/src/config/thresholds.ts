export const THRESHOLDS = {
  http_req_duration: ['p(95)<500', 'p(99)<750'],
  http_req_failed: ['rate<0.01'],

  // Endpoint-specific thresholds
  account_duration: ['p(95)<500'],
  epoch_duration: ['p(95)<500'],
  governance_duration: ['p(95)<500'],
  pool_duration: ['p(95)<500'],

  successful_requests: ['rate>0.999'],
};

export const SMOKE_THRESHOLDS = {
  http_req_duration: ['p(95)<300', 'p(99)<600'],
  http_req_failed: ['rate<0.05'],

  account_duration: ['p(95)<200'],
  epoch_duration: ['p(95)<250'],
  governance_duration: ['p(95)<200'],
  pool_duration: ['p(95)<300'],
};
