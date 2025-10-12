export interface TestConfig {
  baseUrl: string;
  timeout: number;
}

export interface Proposal {
  txHash: string;
  certIndex: number;
  governanceType: string;
}

export interface TestData {
  stakeAddresses: string[];
  poolIds: string[];
  /**
   * TODO: Make these non-optional once boot from snapshot is implemented and we can test endpoints
   * * that require these IDs
   */
  assetIds: string[];
  policyIds: string[];
  drepIds: string[];
  proposals: Proposal[];
}

export interface EndpointWeight {
  name: string;
  weight: number;
  fn: () => void;
}

export interface CheckResult {
  passed: boolean;
  endpoint: string;
  statusCode: number;
  duration: number;
}
