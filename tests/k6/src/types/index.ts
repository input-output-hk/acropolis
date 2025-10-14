export interface Proposal {
  txHash: string;
  certIndex: number;
  governanceType: string;
}

export interface TestData {
  stakeAddresses: string[];
  poolIds: string[];
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
