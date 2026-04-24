export const ENDPOINTS = {
  // Accounts
  ACCOUNT: '/accounts/{stake_address}',

  // Epochs
  EPOCHS_LATEST: '/epochs/latest',
  EPOCHS_LATEST_PARAMETERS: '/epochs/latest/parameters',

  // Governance
  GOV_DREPS: '/governance/dreps',
  GOV_PROPOSALS: '/governance/proposals',

  // Pools
  POOLS: '/pools',
  POOLS_RETIRING: '/pools/retiring',
  POOL: '/pools/{pool_id}',
} as const;


