// For now, resort to testing only Shelley Era ledger state endpoints
export const ENDPOINTS = {
  // Accounts
  ACCOUNT: '/accounts/{stake_address}',

  // Epochs
  EPOCHS_LATEST: '/epochs/latest',
  EPOCHS_LATEST_PARAMETERS: '/epochs/latest/parameters',

  // Pools
  POOLS: '/pools',
  POOLS_EXTENDED: '/pools/extended',
  POOLS_RETIRING: '/pools/retiring',
  POOL: '/pools/{pool_id}',
} as const;
