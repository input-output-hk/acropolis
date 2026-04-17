/// PNI–Consensus blockflow and chain selection scenarios tests.
///
/// These tests drive `BlockFlowHandler` directly (no real networking) and
/// verify the full offer → want → fetch round-trip with a real `Consensus`
/// module connected via an in-memory MockBus.
use std::path::PathBuf;
use std::sync::Arc;

use acropolis_common::messages::{
    BlockOfferedMessage, BlockRescindedMessage, CardanoMessage, ConsensusMessage,
    GenesisCompleteMessage, Message, StateTransitionMessage,
};
use acropolis_common::{
    BlockHash, BlockInfo, BlockIntent, BlockStatus, Era, configuration::BlockFlowMode,
};
use acropolis_module_consensus::Consensus;
use acropolis_test_utils::mainnet_genesis_values;
use caryatid_sdk::{Context, Subscription, mock_bus::MockBus};
use config::{Config, FileFormat};
use pallas::network::miniprotocols::Point;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, timeout};

use super::BlockSink;
use crate::block_flow::BlockFlowHandler;
use crate::configuration::{InterfaceConfig, SyncPoint};
use crate::connection::Header;
use crate::network::{NetworkEvent, PeerId};

const GENESIS_HASH: BlockHash = BlockHash::new([0; 32]);
const BLOCK_A: BlockHash = BlockHash::new([1; 32]);
const BLOCK_B: BlockHash = BlockHash::new([2; 32]);
const BLOCK_C: BlockHash = BlockHash::new([3; 32]);
const BLOCK_D: BlockHash = BlockHash::new([4; 32]);
const BLOCK_E: BlockHash = BlockHash::new([5; 32]);

const PEER_1: PeerId = PeerId(1);
const PEER_2: PeerId = PeerId(2);

const SLOT_A: u64 = 100;
const SLOT_B: u64 = 101;
const SLOT_C: u64 = 102;
const SLOT_D: u64 = 103;
const SLOT_E: u64 = 104;

struct TestHarness {
    flow: BlockFlowHandler,
    sink: BlockSink,
    events_rx: mpsc::Receiver<NetworkEvent>,
    /// Subscribed to `cardano.consensus.offers` — receives BlockOffered and
    /// BlockRescinded messages published by PNI to consensus.
    offers_sub: Box<dyn Subscription<Message>>,
    /// Subscribed to `cardano.block.proposed` — receives blocks that consensus
    /// has accepted and proposed to downstream (validators, state modules).
    proposed_sub: Box<dyn Subscription<Message>>,
}

fn make_config() -> Config {
    Config::builder()
        .add_source(config::File::from_str(
            r#"
            [startup]
            block-flow-mode = "consensus"

            [module.consensus]
            blocks-available-topic = "cardano.block.available"
            blocks-proposed-topic = "cardano.block.proposed"
            consensus-offers-topic = "cardano.consensus.offers"
            consensus-wants-topic = "cardano.consensus.wants"
            validators = []
            validation-timeout = 1
            force-validation = false
            "#,
            FileFormat::Toml,
        ))
        .build()
        .expect("test config is valid TOML")
}

fn make_context(config: Config) -> Arc<Context<Message>> {
    let config = Arc::new(config);
    let bus = Arc::new(MockBus::<Message>::new(&config));
    let (_tx, rx) = watch::channel(true);
    Arc::new(Context::new(config, bus, rx))
}

async fn make_harness() -> TestHarness {
    let config = make_config();
    let context = make_context(config);

    // Init the real Consensus module.
    let consensus = Consensus;
    consensus.init(context.clone(), context.config.clone()).await.expect("consensus init failed");

    // Publish GenesisComplete so the consensus module can proceed past its
    // genesis wait and enter the main select loop.
    let genesis_block_info = BlockInfo {
        status: BlockStatus::Bootstrap,
        intent: BlockIntent::Apply,
        slot: 0,
        number: 0,
        hash: BlockHash::new([0; 32]),
        epoch: 0,
        epoch_slot: 0,
        new_epoch: false,
        is_new_era: false,
        tip_slot: None,
        timestamp: 0,
        era: Era::Byron,
    };
    context
        .publish(
            "cardano.sequence.bootstrapped",
            Arc::new(Message::Cardano((
                genesis_block_info,
                CardanoMessage::GenesisComplete(GenesisCompleteMessage {
                    values: mainnet_genesis_values(),
                }),
            ))),
        )
        .await
        .expect("publish GenesisComplete");

    // Subscribe to offers topic before creating the flow handler so we don't
    // miss any early messages.
    let offers_sub =
        context.subscribe("cardano.consensus.offers").await.expect("offers subscription failed");

    let (events_sender, events_rx) = mpsc::channel(64);

    let cfg = InterfaceConfig {
        block_topic: "cardano.block.available".to_string(),
        sync_point: SyncPoint::Origin,
        genesis_completion_topic: "cardano.sequence.bootstrapped".to_string(),
        sync_command_topic: "cardano.sync.command".to_string(),
        node_addresses: vec![],
        cache_dir: PathBuf::from("/tmp"),
        genesis_values: None,
        consensus_topic: "cardano.consensus.offers".to_string(),
        block_wanted_topic: "cardano.consensus.wants".to_string(),
        target_peer_count: 15,
        min_hot_peers: 3,
        peer_sharing_enabled: false,
        churn_interval_secs: 600,
        peer_sharing_timeout_secs: 10,
        connect_timeout_secs: 15,
        ipv6_enabled: false,
        allow_non_public_peer_addrs: true,
        discovery_interval_secs: 0,
        peer_sharing_cooldown_secs: 0,
    };

    let block_wanted_subscription =
        Some(context.message_bus.subscribe(&cfg.block_wanted_topic).await.unwrap());
    let flow = BlockFlowHandler::new(
        &cfg,
        BlockFlowMode::Consensus,
        acropolis_common::params::SECURITY_PARAMETER_K,
        context.clone(),
        events_sender,
        block_wanted_subscription,
    );

    // BlockSink private fields are accessible here because this module is a
    // child of the crate root where BlockSink is defined.
    let sink = BlockSink {
        context: context.clone(),
        topic: "cardano.block.available".to_string(),
        genesis_values: mainnet_genesis_values(),
        upstream_cache: None,
        last_epoch: None,
        era: None,
        rolled_back: false,
    };

    let proposed_sub =
        context.subscribe("cardano.block.proposed").await.expect("proposed subscription failed");

    TestHarness {
        flow,
        sink,
        events_rx,
        offers_sub,
        proposed_sub,
    }
}

// Helpers

fn make_header(slot: u64, number: u64, hash: BlockHash, parent: BlockHash) -> Header {
    Header {
        hash,
        slot,
        number,
        bytes: vec![],
        era: Era::Conway,
        parent_hash: Some(parent),
    }
}

/// Simulate a peer announcing a block: set tip, roll forward, publish.
async fn drive_offer(
    h: &mut TestHarness,
    peer: PeerId,
    slot: u64,
    number: u64,
    hash: BlockHash,
    parent: BlockHash,
) {
    let header = make_header(slot, number, hash, parent);
    h.flow.handle_tip(peer, Point::Specific(slot, hash.to_vec()));
    h.flow.handle_roll_forward(peer, header);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish failed");
}

/// Assert the next message on `offers_sub` is `BlockOffered` with the given hash.
async fn assert_block_offered(h: &mut TestHarness, expected: BlockHash) {
    let (_, msg) = timeout(Duration::from_secs(1), h.offers_sub.read())
        .await
        .expect("timed out waiting for BlockOffered")
        .expect("offers subscription closed");
    match msg.as_ref() {
        Message::Consensus(ConsensusMessage::BlockOffered(BlockOfferedMessage {
            hash, ..
        })) => {
            assert_eq!(*hash, expected, "BlockOffered hash mismatch");
        }
        other => panic!("expected BlockOffered({expected}), got {other:?}"),
    }
}

/// Assert the next message on `offers_sub` is `BlockRescinded` with the given hash.
async fn assert_block_rescinded(h: &mut TestHarness, expected: BlockHash) {
    let (_, msg) = timeout(Duration::from_secs(1), h.offers_sub.read())
        .await
        .expect("timed out waiting for BlockRescinded")
        .expect("offers subscription closed");
    match msg.as_ref() {
        Message::Consensus(ConsensusMessage::BlockRescinded(BlockRescindedMessage {
            hash,
            ..
        })) => {
            assert_eq!(*hash, expected, "BlockRescinded hash mismatch");
        }
        other => panic!("expected BlockRescinded({expected}), got {other:?}"),
    }
}

/// Assert the next event in `events_rx` is `BlockWanted` with the given hash.
async fn assert_block_wanted(h: &mut TestHarness, expected: BlockHash) {
    let event = timeout(Duration::from_secs(1), h.events_rx.recv())
        .await
        .expect("timed out waiting for BlockWanted")
        .expect("events channel closed");
    match event {
        NetworkEvent::BlockWanted { hash, .. } => {
            assert_eq!(hash, expected, "BlockWanted hash mismatch");
        }
        _other => panic!("expected BlockWanted({expected}), got a different NetworkEvent variant"),
    }
}

/// Assert no offer-topic message arrives within 200 ms.
async fn assert_no_offers_msg(h: &mut TestHarness) {
    let result = timeout(Duration::from_millis(200), h.offers_sub.read()).await;
    assert!(
        result.is_err(),
        "expected no message on offers topic, but received one"
    );
}

/// Assert no network event arrives within 200 ms.
async fn assert_no_network_event(h: &mut TestHarness) {
    let result = timeout(Duration::from_millis(200), h.events_rx.recv()).await;
    assert!(
        result.is_err(),
        "expected no network event, but received one"
    );
}

/// Assert the next message on `proposed_sub` is a BlockProposed (BlockAvailable)
/// with the given hash.
async fn assert_block_proposed(h: &mut TestHarness, expected: BlockHash) {
    let (_, msg) = timeout(Duration::from_secs(2), h.proposed_sub.read())
        .await
        .expect("timed out waiting for BlockProposed")
        .expect("proposed subscription closed");
    match msg.as_ref() {
        Message::Cardano((info, CardanoMessage::BlockAvailable(_))) => {
            assert_eq!(info.hash, expected, "BlockProposed hash mismatch");
        }
        other => panic!("expected BlockProposed/BlockAvailable({expected}), got {other:?}"),
    }
}

/// Assert a Rollback message arrives on `proposed_sub`.
async fn assert_rollback(h: &mut TestHarness) {
    let (_, msg) = timeout(Duration::from_secs(2), h.proposed_sub.read())
        .await
        .expect("timed out waiting for Rollback")
        .expect("proposed subscription closed");
    match msg.as_ref() {
        Message::Cardano((
            _,
            CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
        )) => {}
        other => panic!("expected Rollback, got {other:?}"),
    }
}

/// Drive offer + want + fetch + publish for a single block.
async fn drive_full_block(
    h: &mut TestHarness,
    peer: PeerId,
    slot: u64,
    number: u64,
    hash: BlockHash,
    parent: BlockHash,
) {
    drive_offer(h, peer, slot, number, hash, parent).await;
    assert_block_offered(h, hash).await;
    assert_block_wanted(h, hash).await;
    drive_fetch(h, slot, hash).await;
}

/// Fetch a block body and publish it as BlockAvailable.
///
/// Use after `assert_block_wanted` — the block must already be in the tracker.
async fn drive_fetch(h: &mut TestHarness, slot: u64, hash: BlockHash) {
    h.flow.handle_block_fetched(slot, hash, vec![0u8; 32]);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after fetch failed");
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// Tests

/// Happy path: single block flows offer → want → fetch.
/// See `full_round_trip_single_block_proposed` for the full chain including BlockProposed.
#[tokio::test]
async fn pni_consensus_happy_path_single_block() {
    let mut h = make_harness().await;

    // Step 1: peer announces block A.
    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;

    // PNI publishes BlockOffered to consensus.
    assert_block_offered(&mut h, BLOCK_A).await;

    // Consensus replies with BlockWanted → forwarded to events_rx.
    assert_block_wanted(&mut h, BLOCK_A).await;

    // Step 2: block body fetched and published as BlockAvailable.
    h.flow.handle_block_fetched(SLOT_A, BLOCK_A, vec![0u8; 32]);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after fetch failed");
}

/// Two peers announce the same block: exactly one BlockOffered and one BlockWanted.
#[tokio::test]
async fn pni_consensus_two_peers_same_block() {
    let mut h = make_harness().await;

    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    drive_offer(&mut h, PEER_2, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;

    // Exactly one BlockOffered (deduplicated by BlockTracker).
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_no_offers_msg(&mut h).await;

    // Exactly one BlockWanted from consensus.
    assert_block_wanted(&mut h, BLOCK_A).await;
    assert_no_network_event(&mut h).await;
}

/// Two peers announce competing blocks at the same height.
/// Consensus selects the block on the favoured chain (the first offered, BLOCK_A)
/// and wants only that one.
#[tokio::test]
async fn pni_consensus_fork_competing_blocks() {
    let mut h = make_harness().await;

    // Peer 1 announces BLOCK_A (slot 100); sets tree root + favoured tip.
    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    // Peer 2 announces BLOCK_B (slot 101) at the same height.
    // Both blocks extend GENESIS, so chain length is tied at 1; tie-break
    // keeps BLOCK_A as the favoured tip.
    drive_offer(&mut h, PEER_2, SLOT_B, 1, BLOCK_B, GENESIS_HASH).await;

    // Both blocks are offered.
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_offered(&mut h, BLOCK_B).await;

    // Only BLOCK_A is wanted (it is on the favoured chain).
    assert_block_wanted(&mut h, BLOCK_A).await;
    assert_no_network_event(&mut h).await;
}

/// Peer rolls back after announcing: BlockOffered then BlockRescinded.
#[tokio::test]
async fn pni_consensus_rollback_rescinds() {
    let mut h = make_harness().await;

    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_wanted(&mut h, BLOCK_A).await;

    // Peer rolls back past BLOCK_A.
    h.flow.handle_roll_backward(PEER_1, Point::Origin);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after rollback failed");

    assert_block_rescinded(&mut h, BLOCK_A).await;
}

/// Sole announcer disconnects: BlockOffered then BlockRescinded.
#[tokio::test]
async fn pni_consensus_all_announcers_disconnect() {
    let mut h = make_harness().await;

    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_wanted(&mut h, BLOCK_A).await;

    // The only announcer disconnects.
    h.flow.handle_disconnect(PEER_1, None);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after disconnect failed");

    assert_block_rescinded(&mut h, BLOCK_A).await;
}

/// Two peers announce the same block; one rolls back.
/// BLOCK_A is not rescinded because PEER_2 still has it.
#[tokio::test]
async fn pni_consensus_redundancy_one_rolls_back() {
    let mut h = make_harness().await;

    // Both peers announce BLOCK_A.
    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    drive_offer(&mut h, PEER_2, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;

    // Consume the single BlockOffered and BlockWanted before continuing.
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_wanted(&mut h, BLOCK_A).await;

    // PEER_1 rolls back.
    h.flow.handle_roll_backward(PEER_1, Point::Origin);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after rollback failed");

    // PEER_2 still has BLOCK_A — no Rescinded, no new network event.
    assert_no_offers_msg(&mut h).await;
    assert_no_network_event(&mut h).await;
}

/// Full round trip: after block fetch, `block_rejected_announcers` returns the
/// correct peer set and is then cleared on a second call.
#[tokio::test]
async fn pni_consensus_block_rejected_returns_announcers() {
    let mut h = make_harness().await;

    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_wanted(&mut h, BLOCK_A).await;

    // Simulate block fetch (moves announcers to the `fetched` map).
    h.flow.handle_block_fetched(SLOT_A, BLOCK_A, vec![0u8; 32]);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after fetch failed");

    // First query: returns PEER_1 and clears the entry.
    let announcers = h.flow.block_rejected_announcers(BLOCK_A);
    assert_eq!(
        announcers,
        vec![PEER_1],
        "expected PEER_1 as the rejectable announcer"
    );

    // Second query: entry has been consumed.
    let announcers2 = h.flow.block_rejected_announcers(BLOCK_A);
    assert!(announcers2.is_empty(), "expected empty on second call");
}

/// Offer → rescind → re-offer from a new peer → new BlockOffered and BlockWanted.
#[tokio::test]
async fn pni_consensus_rewant_after_rescind() {
    let mut h = make_harness().await;

    // Initial offer and want.
    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_wanted(&mut h, BLOCK_A).await;

    // PEER_1 rolls back → Rescinded.
    h.flow.handle_roll_backward(PEER_1, Point::Origin);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish after rollback failed");
    assert_block_rescinded(&mut h, BLOCK_A).await;

    // PEER_2 re-offers the same block.
    drive_offer(&mut h, PEER_2, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;

    // A fresh BlockOffered is published (block was removed from tracker by Rescinded).
    assert_block_offered(&mut h, BLOCK_A).await;

    // Consensus re-wants BLOCK_A (it was removed from the tree by BlockRescinded).
    assert_block_wanted(&mut h, BLOCK_A).await;
}

// Full round-trip tests (require consensus to accept BlockAvailable)

/// Full offer → want → fetch → BlockProposed round-trip for a single block.
/// Consensus skips CBOR header parsing for blocks already in the tree (offered
/// via BlockOffered) and trusts the tree's stored parent hash.
#[tokio::test]
async fn full_round_trip_single_block_proposed() {
    let mut h = make_harness().await;

    drive_full_block(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;

    // Consensus should accept the block and publish BlockProposed.
    assert_block_proposed(&mut h, BLOCK_A).await;
}

/// Full chain of 3 blocks: each must be proposed in order.
#[tokio::test]
async fn full_round_trip_chain_of_three() {
    let mut h = make_harness().await;

    drive_full_block(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_proposed(&mut h, BLOCK_A).await;

    drive_full_block(&mut h, PEER_1, SLOT_B, 2, BLOCK_B, BLOCK_A).await;
    assert_block_proposed(&mut h, BLOCK_B).await;

    drive_full_block(&mut h, PEER_1, SLOT_C, 3, BLOCK_C, BLOCK_B).await;
    assert_block_proposed(&mut h, BLOCK_C).await;
}

/// Two peers build competing forks; after fetch, only the favoured chain
/// gets BlockProposed.
#[tokio::test]
async fn full_round_trip_fork_favoured_chain_proposed() {
    let mut h = make_harness().await;

    // Both peers offer their first block (same parent = genesis).
    drive_offer(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    drive_offer(&mut h, PEER_2, SLOT_A, 1, BLOCK_B, GENESIS_HASH).await;

    assert_block_offered(&mut h, BLOCK_A).await;
    assert_block_offered(&mut h, BLOCK_B).await;

    // Consensus wants only the favoured chain tip (BLOCK_A).
    assert_block_wanted(&mut h, BLOCK_A).await;

    // Fetch BLOCK_A.
    h.flow.handle_block_fetched(SLOT_A, BLOCK_A, vec![0u8; 32]);
    let mut published = 0;
    h.flow.publish(&mut h.sink, &mut published).await.expect("publish failed");
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Only BLOCK_A is proposed.
    assert_block_proposed(&mut h, BLOCK_A).await;
}

// Chain selection tests

/// Longer fork overtakes: Peer 1 builds A→B (length 2), Peer 2 builds a
/// longer fork C→D→E (length 3). Consensus should rollback B, then propose
/// the new chain's blocks.
///
/// Timeline:
///   Peer 1: GENESIS → A → B  (favoured, both fetched & proposed)
///   Peer 2: GENESIS → C → D → E  (overtakes at E, triggers chain switch)
#[tokio::test]
async fn chain_selection_longer_fork_overtakes() {
    let mut h = make_harness().await;

    // Peer 1 builds favoured chain: A → B.
    drive_full_block(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_proposed(&mut h, BLOCK_A).await;

    drive_full_block(&mut h, PEER_1, SLOT_B, 2, BLOCK_B, BLOCK_A).await;
    assert_block_proposed(&mut h, BLOCK_B).await;

    // Peer 2 offers a competing fork from genesis: C and D.
    // At length 2 this ties with Peer 1's chain — incumbent wins, no switch.
    drive_offer(&mut h, PEER_2, SLOT_C, 1, BLOCK_C, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_C).await;
    // BLOCK_C is on unfavoured fork — not wanted yet.
    assert_no_network_event(&mut h).await;

    drive_offer(&mut h, PEER_2, SLOT_D, 2, BLOCK_D, BLOCK_C).await;
    assert_block_offered(&mut h, BLOCK_D).await;
    // Still tied (length 2 vs 2) — incumbent retains, no want.
    assert_no_network_event(&mut h).await;

    // Peer 2 extends to length 3 → chain switch!
    // Consensus fires: Rollback, then BlockWanted for C and D (newly wanted).
    drive_offer(&mut h, PEER_2, SLOT_E, 3, BLOCK_E, BLOCK_D).await;
    assert_block_offered(&mut h, BLOCK_E).await;

    // Chain switch: consensus wants blocks on the new favoured chain.
    // The order of wanted messages matches ascending block number.
    assert_block_wanted(&mut h, BLOCK_C).await;
    assert_block_wanted(&mut h, BLOCK_D).await;
    assert_block_wanted(&mut h, BLOCK_E).await;

    // Rollback is published on proposed_sub (rolls back past B to genesis).
    assert_rollback(&mut h).await;

    // Fetch and deliver the new chain's blocks (already offered, just need bodies).
    drive_fetch(&mut h, SLOT_C, BLOCK_C).await;
    assert_block_proposed(&mut h, BLOCK_C).await;

    drive_fetch(&mut h, SLOT_D, BLOCK_D).await;
    assert_block_proposed(&mut h, BLOCK_D).await;

    drive_fetch(&mut h, SLOT_E, BLOCK_E).await;
    assert_block_proposed(&mut h, BLOCK_E).await;
}

/// Equal-length forks: incumbent tip is retained (Praos tie-breaking).
///
///   Peer 1: GENESIS → A → B  (favoured, fetched)
///   Peer 2: GENESIS → C → D  (same length — no switch)
#[tokio::test]
async fn chain_selection_equal_length_incumbent_wins() {
    let mut h = make_harness().await;

    // Peer 1 builds A → B.
    drive_full_block(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_proposed(&mut h, BLOCK_A).await;

    drive_full_block(&mut h, PEER_1, SLOT_B, 2, BLOCK_B, BLOCK_A).await;
    assert_block_proposed(&mut h, BLOCK_B).await;

    // Peer 2 offers equal-length fork: C → D.
    drive_offer(&mut h, PEER_2, SLOT_C, 1, BLOCK_C, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_C).await;
    assert_no_network_event(&mut h).await;

    drive_offer(&mut h, PEER_2, SLOT_D, 2, BLOCK_D, BLOCK_C).await;
    assert_block_offered(&mut h, BLOCK_D).await;

    // No chain switch — incumbent wins. No new wants or rollbacks.
    assert_no_network_event(&mut h).await;
}

/// Late extension: Peer 1's chain is fetched and proposed, then Peer 2
/// extends its fork past Peer 1. Chain switch occurs, Rollback fires,
/// and the new chain's blocks are re-proposed.
///
///   Peer 1: GENESIS → A (fetched, proposed)
///   Peer 2: GENESIS → C → D (overtakes at D, triggers chain switch)
#[tokio::test]
async fn chain_selection_late_extension_triggers_switch() {
    let mut h = make_harness().await;

    // Peer 1 builds single block A.
    drive_full_block(&mut h, PEER_1, SLOT_A, 1, BLOCK_A, GENESIS_HASH).await;
    assert_block_proposed(&mut h, BLOCK_A).await;

    // Peer 2 offers C (ties at length 1 — no switch).
    drive_offer(&mut h, PEER_2, SLOT_C, 1, BLOCK_C, GENESIS_HASH).await;
    assert_block_offered(&mut h, BLOCK_C).await;
    assert_no_network_event(&mut h).await;

    // Peer 2 extends to D → overtakes! Chain switch.
    drive_offer(&mut h, PEER_2, SLOT_D, 2, BLOCK_D, BLOCK_C).await;
    assert_block_offered(&mut h, BLOCK_D).await;

    // Consensus wants C and D on the new chain.
    assert_block_wanted(&mut h, BLOCK_C).await;
    assert_block_wanted(&mut h, BLOCK_D).await;

    // Rollback from A to genesis.
    assert_rollback(&mut h).await;

    // Fetch and deliver the new chain's blocks (already offered, just need bodies).
    drive_fetch(&mut h, SLOT_C, BLOCK_C).await;
    assert_block_proposed(&mut h, BLOCK_C).await;

    drive_fetch(&mut h, SLOT_D, BLOCK_D).await;
    assert_block_proposed(&mut h, BLOCK_D).await;
}
