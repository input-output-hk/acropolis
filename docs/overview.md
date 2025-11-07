# Acropolis Cardano Node
## Deliverable Descriptions:

### **1. Archival Node** (formerly "Data node")

A read-only node that synchronizes blockchain data and provides query access without participating in validation or consensus.

**Who Uses This:**
- Users requiring blockchain data access without validation responsibilities
- Applications needing transaction submission capabilities to the network, e.g. DApps

**Components Required:**
- Network: Miniprotocols, Upstream Chain Fetch, Snapshot Boot, Tx Submission Outbound
- Consensus: Chain Store
- Ledger: UTXOs, Pools, Governance, Stake, Monetary, Rewards
- APIs: Blockfrost

**What It Does for External Users:**
- Synchronizes with the Cardano blockchain from genesis or a snapshot
- Stores a window complete ledger state (UTXOs, stake distribution, pool info, governance data)
- Provides REST API access (Blockfrost-compatible) to query blockchain data over the window of history
- Allows users to submit transactions to the network
- Acts as a read-only data source for wallets and applications

**External Runtime Requirements:**
- **Needs to connect to:** Established Cardano relay nodes for initial sync
- **Network access:** Must reach multiple relay nodes (typically 3-5) for chain-fetch protocol ("multi-peer basic")
- **Storage:** Significant disk space for blockchain data (hundreds of GB)
- **Does NOT need:** Block producer credentials, stake pool keys, or direct peer-to-peer listening capabilities

---

### **2. Validation Node** (formerly "Wallet")

A node that fully validates all blocks and transactions according to Cardano's ledger rules, providing trustless verification of chain state.

**Who Uses This:**
- Users requiring independent verification of blockchain state
- Applications needing cryptographic guarantees of chain validity
- Wallet applications

**Components Required:**
- All components from Archival Node, plus:
- Network: Multi-peer Consensus
- Consensus: Header Validation, Tx Validation Phase 1, Chain Selection, Peer Management
- Ledger: Ledger Validation (full validation rules)

**What It Does for External Users:**
- Validates all blocks and transactions according to ledger rules
- Maintains validated chain state with cryptographic guarantees
- Supports wallet backends with trusted validation
- Enables secure local transaction construction
- Provides confidence in chain data without trusting external APIs
- Can detect and reject invalid blocks/transactions

**External Runtime Requirements:**
- **Needs to connect to:** Multiple diverse relay nodes for redundancy (5-10 peers recommended)
- **Network access:** Bidirectional TCP connections, can accept incoming connections from untrusted peers
- **Validation overhead:** Higher CPU usage for script validation and signature checks
- **Does NOT need:** Ability to produce blocks, VRF keys, or KES keys
- **Does NOT provide:** Full mempool services or block propagation

---

### **3. Relay Node**

A publicly accessible node that propagates blocks and transactions across the network while maintaining a mempool and validating Plutus scripts.

**Who Uses This:**
- **Stake pool operators (SPOs)** as part of their stake pool infrastructure to protect block-producing nodes
- Network participants contributing to blockchain decentralization and resilience

**Components Required:**
- All components from Validation Node, plus:
- Network: Peer Server, Tx Submission Inbound
- Consensus: Tx Validation Phase 2 (Plutus Scripts), MemPool, Block Production (block validation only)

**What It Does for External Users:**
- Acts as a network relay, propagating blocks and transactions across the network
- Validates Plutus scripts (Phase 2 validation)
- Maintains a mempool of pending transactions
- Accepts incoming connections from other nodes and wallets
- Serves as infrastructure for the decentralized network
- Provides high-availability access point for SPOs' block producers

**External Runtime Requirements:**
- **Needs to connect to:** 20-50 other relay nodes in a diverse topology
- **Network access:** Must accept incoming connections (public IP or port forwarding)
- **Firewall configuration:** Open TCP port (typically 3001) for P2P communication
- **Bandwidth:** Sustained bandwidth for continuous block/tx propagation
- **Does NOT need:** Block signing keys, stake pool operational certificates
- **Topology:** Should connect to both community relays and your own block producer (if applicable)

---

### **4. Block Producing Node** (formerly "Praos Block Producing Node")

A full consensus node that participates in Ouroboros Praos to produce blocks when elected as slot leader.

**Who Uses This:**
- **Stake pool operators (SPOs)** running registered stake pools to produce blocks and earn rewards

**Components Required:**
- All components from Relay Node, plus:
- Network: Multi-peer Auto P2P, OP N164 Protocols, EB Distribution
- Consensus: Full block production capability
- Ledger:

**What It Does for External Users:**
- Participates in Ouroboros Praos consensus as a block producer
- Generates blocks when elected by VRF lottery
- Signs blocks with operational certificates and KES keys
- Maintains full consensus state including leadership schedule
- Enables stake pool operation
- Can participate in governance actions (voting)

**External Runtime Requirements:**
- **Needs to connect to:** Your own relay nodes (private topology) + trusted community relays
- **Network topology:** Should NOT be directly exposed to internet; connects through relay(s)
- **Credentials required:** 
  - VRF key (for leader election)
  - KES keys (for block signing, rotated periodically)
  - Operational certificate
  - Stake pool registration on-chain
- **High availability:** Needs reliable uptime during slot leadership
- **Time synchronization:** NTP critical for accurate slot timing
- **Secure environment:** Air-gapped or highly secured key management

---

### **5. Leios Node** (formerly "Leios Block Producing Node")

A next-generation consensus node implementing the Leios protocol for significantly higher transaction throughput while maintaining Praos compatibility.

**Who Uses This:**
- Early protocol adopters and testers (specific user types not yet documented as Leios is in research/development phase)

**Components Required:**
- All components from Block Producing Node, plus:
- Network: Additional Leios-specific protocols
- Consensus: Leios consensus mechanism, EB/RB production
- Ledger: Leios voting, Blockfrost Leios extensions

**What It Does for External Users:**
- Implements next-generation Leios consensus protocol
- Provides significantly higher transaction throughput
- Enables faster block production with Input Blocks (IBs) and Endorsement Blocks (EBs)
- Maintains backward compatibility with Praos
- Allows participation in experimental high-performance network

**External Runtime Requirements:**
- **Needs to connect to:** Other Leios-enabled nodes (likely testnet initially)
- **Network requirements:** Higher bandwidth for increased throughput
- **All requirements from Block Producing Node:** Plus Leios-specific credentials
- **Experimental phase:** May require separate network or testnet participation
- **Does NOT immediately replace:** Praos consensus (gradual transition expected)
