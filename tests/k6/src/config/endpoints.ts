export const ENDPOINTS = {
  // Accounts
  ACCOUNT: '/accounts/{stake_address}',

  // Assets
  ASSETS: '/assets',
  ASSET: '/assets/{asset}',
  ASSET_HISTORY: '/assets/{asset}/history',
  ASSET_TRANSACTIONS: '/assets/{asset}/transactions',
  ASSET_ADDRESSES: '/assets/{asset}/addresses',
  ASSET_POLICY: '/assets/policy/{policy_id}',

  // Epochs
  EPOCHS_LATEST: '/epochs/latest',
  EPOCH: '/epochs/{epoch_no}',
  EPOCHS_LATEST_PARAMETERS: '/epochs/latest/parameters',

  // Governance
  GOV_DREPS: '/governance/dreps',
  GOV_DREP: '/governance/dreps/{drep_id}',
  GOV_DREP_DELEGATORS: '/governance/dreps/{drep_id}/delegators',
  GOV_DREP_METADATA: '/governance/dreps/{drep_id}/metadata',
  GOV_DREP_UPDATES: '/governance/dreps/{drep_id}/updates',
  GOV_DREP_VOTES: '/governance/dreps/{drep_id}/votes',
  GOV_PROPOSALS: '/governance/proposals',
  GOV_PROPOSAL: '/governance/proposals/{tx_hash}/{cert_index}',
  GOV_PROPOSAL_VOTES: '/governance/proposals/{tx_hash}/{cert_index}/votes',
  GOV_PROPOSAL_METADATA: '/governance/proposals/{tx_hash}/{cert_index}/metadata',

  // Pools
  POOLS: '/pools',
  POOLS_EXTENDED: '/pools/extended',
  POOLS_RETIRED: '/pools/retired',
  POOLS_RETIRING: '/pools/retiring',
  POOL: '/pools/{pool_id}',
} as const;

export function buildUrl(endpoint: string, params: Record<string, string>): string {
  let url = endpoint;
  for (const [key, value] of Object.entries(params)) {
    url = url.replace(`{${key}}`, value);
  }
  return url;
}
