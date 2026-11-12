//! A peer connection on pallas-network2, set up like pallas' `examples/leios-testnet`: a
//! `Manager` over a `TcpInterface` and the stock `InitiatorBehavior`, with only the
//! handshake versions changed. There is one such stack per peer, so the network manager
//! keeps choosing which peer each command goes to; this module only translates between its
//! commands/events and the behavior's.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use acropolis_common::{BlockHash, Era};
use anyhow::{Result, anyhow};
use pallas::ledger::traverse::{MultiEraBlock, MultiEraHeader};
use pallas_network2::{
    Interface, Manager, PeerId,
    behavior::{
        AnyMessage,
        initiator::{
            Config as HandshakeConfig, HandshakeBehavior, InitiatorBehavior, InitiatorCommand,
            InitiatorEvent,
        },
    },
    interface::TcpInterface,
    protocol::{Point, handshake::n2n::VersionTable},
};
use tokio::{select, sync::mpsc};
use tracing::{debug, warn};

use crate::network::PeerMessageSender;

/// Same interval as pallas' `examples/leios-testnet`.
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(3);

/// Blocks asked of this peer whose body has not arrived yet, from which on the next header is
/// not asked for. Catching up, chainsync outruns blockfetch (one single-block range in flight
/// per peer) and every header waiting for its body stays in memory (pallas' blockfetch queue,
/// the chain state); without a cap that grows with the distance to the tip. Blockfetch only
/// needs one queued range to stay busy.
const MAX_PENDING_FETCHES: usize = 256;

pub struct PeerConnection {
    pub address: String,
    /// Commands to the task running the peer's `Manager`; dropping the connection stops it.
    commands: Result<mpsc::UnboundedSender<InitiatorCommand>>,
    intersect_asked: AtomicBool,
}

impl PeerConnection {
    pub fn new(address: String, magic: u32, sender: PeerMessageSender, delay: Duration) -> Self {
        Self::open_on(
            TcpInterface::new(),
            address,
            magic,
            sender,
            delay,
            HOUSEKEEPING_INTERVAL,
        )
    }

    /// `PeerId::from_str` panics on a bad port; addresses come from config and peer sharing.
    fn open_on<I>(
        interface: I,
        address: String,
        magic: u32,
        sender: PeerMessageSender,
        delay: Duration,
        housekeeping: Duration,
    ) -> Self
    where
        I: Interface<AnyMessage> + Send + 'static,
    {
        let commands =
            address.parse::<PeerId>().map_err(|err| anyhow!("{address}: {err}")).map(|peer| {
                let behavior = InitiatorBehavior {
                    handshake: HandshakeBehavior::new(HandshakeConfig {
                        supported_version: VersionTable::v11_and_above_with_query(
                            magic.into(),
                            false,
                        ),
                    }),
                    ..Default::default()
                };
                let manager = Manager::new(interface, behavior);
                let (commands, receiver) = mpsc::unbounded_channel();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    run(manager, peer, receiver, sender, housekeeping).await;
                });
                commands
            });
        Self {
            address,
            commands,
            intersect_asked: AtomicBool::new(false),
        }
    }

    /// Reaches the peer only while its chainsync is new: `InitiatorBehavior` sends
    /// `FindIntersect` once per connection. The network manager never asks a syncing peer
    /// for a new intersection with the shipped configurations, so a second call is only
    /// reported.
    pub fn find_intersect(&self, points: Vec<Point>) -> Result<()> {
        if self.intersect_asked.swap(true, Ordering::SeqCst) {
            warn!(
                peer = self.address,
                "new intersection asked; pallas-network2 only sends it if the peer is not syncing yet"
            );
        }
        self.execute(InitiatorCommand::StartSync(points))
    }

    pub fn request_block(&self, hash: BlockHash, slot: u64) -> Result<()> {
        let point = Point::Specific(slot, hash.to_vec());
        self.execute(InitiatorCommand::RequestBlocks((point.clone(), point)))
    }

    fn execute(&self, cmd: InitiatorCommand) -> Result<()> {
        let commands = self.commands.as_ref().map_err(|err| anyhow!("{err:#}"))?;
        commands.send(cmd).map_err(|_| anyhow!("peer connection has stopped"))
    }
}

#[derive(Debug)]
pub enum PeerEvent {
    ChainSync(PeerChainSyncEvent),
    BlockFetched(BlockFetched),
    Disconnected,
}

#[derive(Debug)]
pub enum PeerChainSyncEvent {
    RollForward(Header, Point),
    RollBackward(Point, Point),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub hash: BlockHash,
    pub slot: u64,
    pub number: u64,
    pub bytes: Vec<u8>,
    pub era: Era,
    pub parent_hash: Option<BlockHash>,
}

#[derive(Debug)]
pub struct BlockFetched {
    pub slot: u64,
    pub hash: BlockHash,
    pub body: Vec<u8>,
}

/// Decode a chainsync header; `None` for an epoch boundary block.
fn parse_header(variant: u8, byron_tag: Option<u8>, cbor: Vec<u8>) -> Result<Option<Header>> {
    let hdr = MultiEraHeader::decode(variant, byron_tag, &cbor)?;
    if hdr.as_eb().is_some() {
        // skip EpochBoundary blocks
        return Ok(None);
    }
    let era = Era::try_from(variant)?;
    let parent_hash = hdr.previous_hash().map(|h| BlockHash::new(*h));
    Ok(Some(Header {
        hash: BlockHash::new(*hdr.hash()),
        slot: hdr.slot(),
        number: hdr.number(),
        bytes: cbor,
        era,
        parent_hash,
    }))
}

/// The peer sent something we cannot decode. A peer whose headers or blocks we cannot read
/// is useless to follow, so its connection is dropped and the network manager replaces it.
#[derive(Debug, thiserror::Error)]
enum UndecodableReply {
    #[error("undecodable header: {0:#}")]
    Header(anyhow::Error),
    #[error("undecodable block: {0}")]
    Block(pallas::ledger::traverse::Error),
}

/// Why the task driving a peer's `Manager` stops.
#[derive(Debug)]
enum ConnectionEnd {
    /// The `PeerConnection` was dropped: the network manager let this peer go.
    Released,
    /// The network manager no longer receives events.
    ManagerGone,
    /// The peer sent something we cannot decode.
    Undecodable(UndecodableReply),
}

async fn run<I: Interface<AnyMessage>>(
    mut manager: Manager<I, InitiatorBehavior, AnyMessage>,
    peer: PeerId,
    commands: mpsc::UnboundedReceiver<InitiatorCommand>,
    sender: PeerMessageSender,
    housekeeping: Duration,
) {
    manager.execute(InitiatorCommand::IncludePeer(peer.clone()));
    match drive(manager, commands, &sender, housekeeping).await {
        // Nobody is listening any more: there is no one to report the end to.
        ConnectionEnd::ManagerGone => {}
        ConnectionEnd::Released => {
            let _ = sender.write(PeerEvent::Disconnected).await;
        }
        ConnectionEnd::Undecodable(reply) => {
            warn!(%peer, "{reply}; dropping the peer");
            // So that the network manager replaces the peer.
            let _ = sender.write(PeerEvent::Disconnected).await;
        }
    }
}

/// Run the peer's `Manager` until the connection has to end, and say why.
async fn drive<I: Interface<AnyMessage>>(
    mut manager: Manager<I, InitiatorBehavior, AnyMessage>,
    mut commands: mpsc::UnboundedReceiver<InitiatorCommand>,
    sender: &PeerMessageSender,
    housekeeping: Duration,
) -> ConnectionEnd {
    let mut housekeeping = tokio::time::interval(housekeeping);
    let mut fetches = FetchBackpressure::default();
    loop {
        select! {
            _ = housekeeping.tick() => {
                manager.execute(InitiatorCommand::Housekeeping);
                fetches.housekeeping(&mut manager);
            }
            cmd = commands.recv() => match cmd {
                Some(cmd) => {
                    if matches!(cmd, InitiatorCommand::RequestBlocks(_)) {
                        fetches.requested();
                    }
                    manager.execute(cmd)
                }
                None => return ConnectionEnd::Released,
            },
            event = manager.poll_next() => {
                // `None` only means there was nothing to report this time: the manager
                // keeps running. Not a pattern on the branch, which would disable it.
                let Some(event) = event else { continue };
                match translate(&mut manager, &mut fetches, event) {
                    Ok(Some(event)) => {
                        if sender.write(event).await.is_err() {
                            return ConnectionEnd::ManagerGone;
                        }
                    }
                    Ok(None) => {}
                    Err(reply) => return ConnectionEnd::Undecodable(reply),
                }
            }
        }
    }
}

/// Holds chainsync back while too many requested blocks are still to arrive
/// (see [`MAX_PENDING_FETCHES`]).
#[derive(Debug, Default)]
struct FetchBackpressure {
    /// Blocks requested whose body has not arrived yet.
    pending: usize,
    /// The peer whose next header we did not ask for yet.
    held: Option<PeerId>,
    /// Whether a body arrived since the last housekeeping tick.
    progressed: bool,
}

impl FetchBackpressure {
    fn requested(&mut self) {
        self.pending += 1;
    }

    /// Ask for the next header now, or once enough bodies have arrived.
    fn continue_sync<I: Interface<AnyMessage>>(
        &mut self,
        manager: &mut Manager<I, InitiatorBehavior, AnyMessage>,
        peer: PeerId,
    ) {
        if self.pending >= MAX_PENDING_FETCHES {
            self.held = Some(peer);
        } else {
            manager.execute(InitiatorCommand::ContinueSync(peer));
        }
    }

    fn body_received<I: Interface<AnyMessage>>(
        &mut self,
        manager: &mut Manager<I, InitiatorBehavior, AnyMessage>,
    ) {
        // Saturating: a multi-block range answers one request with several bodies.
        self.pending = self.pending.saturating_sub(1);
        self.progressed = true;
        if self.pending < MAX_PENDING_FETCHES
            && let Some(peer) = self.held.take()
        {
            manager.execute(InitiatorCommand::ContinueSync(peer));
        }
    }

    /// A request the peer never answers (e.g. a block it no longer has) would hold chainsync
    /// forever: if no body arrived over a whole tick, stop counting and release it.
    fn housekeeping<I: Interface<AnyMessage>>(
        &mut self,
        manager: &mut Manager<I, InitiatorBehavior, AnyMessage>,
    ) {
        if !self.progressed
            && let Some(peer) = self.held.take()
        {
            warn!(
                pending = self.pending,
                "no block body over a housekeeping interval; resuming chainsync"
            );
            self.pending = 0;
            manager.execute(InitiatorCommand::ContinueSync(peer));
        }
        self.progressed = false;
    }
}

/// Map a behavior event to the network manager's event, if it has one, asking for the next
/// header after each chainsync event like the example does, unless too many blocks are still
/// to arrive. Fails when the peer sent something we cannot decode; chainsync is then not
/// continued.
fn translate<I: Interface<AnyMessage>>(
    manager: &mut Manager<I, InitiatorBehavior, AnyMessage>,
    fetches: &mut FetchBackpressure,
    event: InitiatorEvent,
) -> Result<Option<PeerEvent>, UndecodableReply> {
    match event {
        InitiatorEvent::IntersectionFound(peer, ..) => {
            manager.execute(InitiatorCommand::ContinueSync(peer));
            Ok(None)
        }
        InitiatorEvent::BlockHeaderReceived(peer, header, tip) => {
            let byron_tag = header.byron_prefix.map(|prefix| prefix.0);
            let parsed = parse_header(header.variant, byron_tag, header.cbor)
                .map_err(UndecodableReply::Header)?;
            fetches.continue_sync(manager, peer);
            // `None` for an epoch boundary block, which the network manager does not follow.
            Ok(parsed
                .map(|header| PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header, tip.0))))
        }
        InitiatorEvent::RollbackReceived(peer, point, tip) => {
            fetches.continue_sync(manager, peer);
            Ok(Some(PeerEvent::ChainSync(
                PeerChainSyncEvent::RollBackward(point, tip.0),
            )))
        }
        InitiatorEvent::BlockBodyReceived(_peer, body) => {
            // The event carries no point: decode the block to recover it.
            let block = MultiEraBlock::decode(&body).map_err(UndecodableReply::Block)?;
            fetches.body_received(manager);
            Ok(Some(PeerEvent::BlockFetched(BlockFetched {
                slot: block.slot(),
                hash: BlockHash::new(*block.hash()),
                body,
            })))
        }
        other => {
            debug!(?other, "unhandled pallas-network2 event");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    //! The stock `InitiatorBehavior` against emulated peers (pallas-network2 `emulation`),
    //! one peer type per test. Each test asserts what the former pallas-network stack did; the
    //! ignored ones show where the behavior falls short for the network manager.

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use pallas_network2::{
        InterfaceEvent,
        emulation::{Emulator, ReplyQueue, Rules},
        protocol::{
            blockfetch,
            chainsync::{self, HeaderContent, Tip},
            handshake::{self, RefuseReason},
        },
    };
    use tokio::time::{sleep, timeout};

    use super::*;
    use crate::network::{NetworkEvent, PeerId as ConnectionId};

    /// A Conway block from preview, already in the repository for the bootstrapper tests.
    const BLOCK: &[u8] = include_bytes!(
        "../../snapshot_bootstrapper/data/preview/blocks/block.86399953.5d37babd87c35b9d5464b8e6c81f655a2fae44a422a1104ca67a7f72476286af.cbor"
    );
    const BLOCK_SLOT: u64 = 86399953;
    /// Chainsync era variant of Conway headers.
    const CONWAY_VARIANT: u8 = 6;
    const WAIT: Duration = Duration::from_secs(2);

    fn block_hash() -> BlockHash {
        BlockHash::new(*MultiEraBlock::decode(BLOCK).unwrap().hash())
    }

    fn tip() -> Tip {
        Tip(Point::Specific(BLOCK_SLOT, block_hash().to_vec()), 1)
    }

    fn header() -> HeaderContent {
        let block = MultiEraBlock::decode(BLOCK).unwrap();
        HeaderContent {
            variant: CONWAY_VARIANT,
            byron_prefix: None,
            cbor: block.header().cbor().to_vec(),
        }
    }

    fn reply(queue: &mut ReplyQueue<AnyMessage>, pid: &PeerId, msg: AnyMessage) {
        queue.push_jittered_msg(pid.clone(), msg, Duration::ZERO);
    }

    /// Accepts our highest version. Echoing our data keeps peer sharing off, as a real node
    /// negotiates it.
    fn accept(
        queue: &mut ReplyQueue<AnyMessage>,
        pid: &PeerId,
        versions: handshake::n2n::VersionTable,
    ) {
        let (version, data) =
            versions.values.into_iter().max_by_key(|(version, _)| *version).unwrap();
        reply(
            queue,
            pid,
            AnyMessage::Handshake(handshake::Message::Accept(version, data)),
        );
    }

    /// Intersects on the first point offered.
    fn intersect(points: Vec<Point>) -> AnyMessage {
        let point = points.into_iter().next().unwrap_or(Point::Origin);
        AnyMessage::ChainSync(chainsync::Message::IntersectFound(point, tip()))
    }

    /// Rolls forward to the preview block the first time, then has nothing new.
    fn next_header(rolled: &AtomicBool) -> AnyMessage {
        let msg = if rolled.swap(true, Ordering::SeqCst) {
            chainsync::Message::AwaitReply
        } else {
            chainsync::Message::RollForward(header(), tip())
        };
        AnyMessage::ChainSync(msg)
    }

    /// A peer that follows the chain up to the preview block and serves that block.
    #[derive(Default)]
    struct ServingPeer {
        rolled: AtomicBool,
    }

    impl Rules for ServingPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            match msg {
                AnyMessage::Handshake(handshake::Message::Propose(versions)) => {
                    accept(queue, &pid, versions)
                }
                AnyMessage::ChainSync(chainsync::Message::FindIntersect(points)) => {
                    reply(queue, &pid, intersect(points))
                }
                AnyMessage::ChainSync(chainsync::Message::RequestNext) => {
                    reply(queue, &pid, next_header(&self.rolled))
                }
                AnyMessage::BlockFetch(blockfetch::Message::RequestRange(_)) => {
                    // One delivery, so the three messages cannot be reordered.
                    let msgs = [
                        blockfetch::Message::StartBatch,
                        blockfetch::Message::Block(BLOCK.to_vec()),
                        blockfetch::Message::BatchDone,
                    ];
                    let msgs = msgs.into_iter().map(AnyMessage::BlockFetch).collect();
                    queue.push(Box::pin(async move { InterfaceEvent::Recv(pid, msgs) }));
                }
                _ => {}
            }
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// A peer that accepts the handshake and finds no intersection.
    #[derive(Default)]
    struct NoIntersectionPeer;

    impl Rules for NoIntersectionPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            match msg {
                AnyMessage::Handshake(handshake::Message::Propose(versions)) => {
                    accept(queue, &pid, versions)
                }
                AnyMessage::ChainSync(chainsync::Message::FindIntersect(_)) => {
                    let msg = chainsync::Message::IntersectNotFound(tip());
                    reply(queue, &pid, AnyMessage::ChainSync(msg));
                }
                _ => {}
            }
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// A peer that drops the connection as soon as it hears from us.
    #[derive(Default)]
    struct DroppingPeer;

    impl Rules for DroppingPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            _msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            queue.push_jittered_disconnect(pid, Duration::ZERO);
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// A peer that refuses the handshake.
    #[derive(Default)]
    struct RefusingPeer;

    impl Rules for RefusingPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            if let AnyMessage::Handshake(handshake::Message::Propose(versions)) = msg {
                let version = versions.values.keys().copied().max().unwrap();
                let reason = RefuseReason::Refused(version, "emulated".into());
                reply(
                    queue,
                    &pid,
                    AnyMessage::Handshake(handshake::Message::Refuse(reason)),
                );
            }
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// A peer that rolls forward to a header we cannot decode.
    #[derive(Default)]
    struct GarbledHeaderPeer;

    impl Rules for GarbledHeaderPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            match msg {
                AnyMessage::Handshake(handshake::Message::Propose(versions)) => {
                    accept(queue, &pid, versions)
                }
                AnyMessage::ChainSync(chainsync::Message::FindIntersect(points)) => {
                    reply(queue, &pid, intersect(points))
                }
                AnyMessage::ChainSync(chainsync::Message::RequestNext) => {
                    let header = HeaderContent {
                        cbor: vec![0xff],
                        ..header()
                    };
                    let msg = chainsync::Message::RollForward(header, tip());
                    reply(queue, &pid, AnyMessage::ChainSync(msg));
                }
                _ => {}
            }
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// A peer that serves a block we cannot decode.
    #[derive(Default)]
    struct GarbledBlockPeer;

    impl Rules for GarbledBlockPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            match msg {
                AnyMessage::Handshake(handshake::Message::Propose(versions)) => {
                    accept(queue, &pid, versions)
                }
                AnyMessage::BlockFetch(blockfetch::Message::RequestRange(_)) => {
                    let msgs = [
                        blockfetch::Message::StartBatch,
                        blockfetch::Message::Block(vec![0xff]),
                        blockfetch::Message::BatchDone,
                    ];
                    let msgs = msgs.into_iter().map(AnyMessage::BlockFetch).collect();
                    queue.push(Box::pin(async move { InterfaceEvent::Recv(pid, msgs) }));
                }
                _ => {}
            }
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    /// Block requests received by `BystanderPeer`.
    static BLOCKS_ASKED_FROM_BYSTANDER: AtomicUsize = AtomicUsize::new(0);

    /// A peer that follows the chain and counts the block requests it gets, without
    /// serving them.
    #[derive(Default)]
    struct BystanderPeer {
        rolled: AtomicBool,
    }

    impl Rules for BystanderPeer {
        type Message = AnyMessage;

        fn reply_to(
            &self,
            pid: PeerId,
            msg: AnyMessage,
            _jitter: Duration,
            queue: &mut ReplyQueue<AnyMessage>,
        ) {
            match msg {
                AnyMessage::Handshake(handshake::Message::Propose(versions)) => {
                    accept(queue, &pid, versions)
                }
                AnyMessage::ChainSync(chainsync::Message::FindIntersect(points)) => {
                    reply(queue, &pid, intersect(points))
                }
                AnyMessage::ChainSync(chainsync::Message::RequestNext) => {
                    reply(queue, &pid, next_header(&self.rolled))
                }
                AnyMessage::BlockFetch(blockfetch::Message::RequestRange(_)) => {
                    BLOCKS_ASKED_FROM_BYSTANDER.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
        }

        fn jitter(&self) -> Duration {
            Duration::ZERO
        }
    }

    struct Harness {
        sink: mpsc::Sender<NetworkEvent>,
        events: mpsc::Receiver<NetworkEvent>,
    }

    impl Harness {
        fn new() -> Self {
            let (sink, events) = mpsc::channel(64);
            Self { sink, events }
        }

        /// One emulated pallas-network2 stack per peer, like `PeerConnection::new`, the peer
        /// following `R`.
        fn open_with<R>(&self, id: u64, address: &str) -> PeerConnection
        where
            R: Rules<Message = AnyMessage> + Default + Unpin + Send + 'static,
        {
            let sender = PeerMessageSender::new(ConnectionId(id), self.sink.clone());
            let housekeeping = Duration::from_millis(20);
            PeerConnection::open_on(
                Emulator::<AnyMessage, R>::default(),
                address.to_string(),
                2,
                sender,
                Duration::ZERO,
                housekeeping,
            )
        }

        /// Whether an event matching `wanted` reaches the network manager within `WAIT`.
        async fn receives(&mut self, wanted: impl Fn(ConnectionId, &PeerEvent) -> bool) -> bool {
            let search = async {
                while let Some(event) = self.events.recv().await {
                    if let NetworkEvent::PeerUpdate { peer, event } = event
                        && wanted(peer, &event)
                    {
                        return true;
                    }
                }
                false
            };
            timeout(WAIT, search).await.unwrap_or(false)
        }
    }

    fn is_roll_forward_to_block(event: &PeerEvent) -> bool {
        matches!(
            event,
            PeerEvent::ChainSync(PeerChainSyncEvent::RollForward(header, _))
                if header.slot == BLOCK_SLOT
        )
    }

    /// Control: headers and blocks flow through the harness.
    #[tokio::test]
    async fn follows_a_header_then_fetches_its_block() {
        let mut harness = Harness::new();
        let connection = harness.open_with::<ServingPeer>(0, "peer.test:3001");
        connection.find_intersect(vec![Point::Origin]).unwrap();
        assert!(
            harness.receives(|_, event| is_roll_forward_to_block(event)).await,
            "no roll forward"
        );

        connection.request_block(block_hash(), BLOCK_SLOT).unwrap();
        assert!(
            harness
                .receives(|peer, event| peer == ConnectionId(0)
                    && matches!(event, PeerEvent::BlockFetched(block) if block.slot == BLOCK_SLOT))
                .await,
            "block not fetched"
        );
    }

    /// A peer that shares no ancestor with our chain is behind us or on another chain: the
    /// network manager must hear it to drop and replace that peer. In pallas-network2 1.4.0,
    /// `InitiatorBehavior` instead marks the peer as misbehaving and bans it without any
    /// event, so the network manager waits forever for headers from that peer.
    /// Waiting for the missing-intersection event to reach pallas:
    /// <https://github.com/txpipe/pallas/pull/841>
    /// `PeerChainSyncEvent` has no variant for it until then; this peer never rolls, so any
    /// chain-sync event from it can only be the missing intersection. Match
    /// `PeerChainSyncEvent::IntersectNotFound` once it is reintroduced.
    #[tokio::test]
    #[ignore = "pallas-network2 1.4.0 bans a peer on IntersectNotFound without reporting it; waiting for txpipe/pallas#841"]
    async fn missing_intersection_is_reported() {
        let mut harness = Harness::new();
        let connection = harness.open_with::<NoIntersectionPeer>(0, "peer.test:3001");
        connection.find_intersect(vec![Point::Specific(1, vec![0; 32])]).unwrap();
        assert!(
            harness.receives(|_, event| matches!(event, PeerEvent::ChainSync(_))).await,
            "missing intersection never reported"
        );
        // Once `PeerChainSyncEvent::IntersectNotFound` is reintroduced (txpipe/pallas#841),
        // replace the assert above with:
        // assert!(
        //     harness
        //         .receives(|_, event| matches!(
        //             event,
        //             PeerEvent::ChainSync(PeerChainSyncEvent::IntersectNotFound(_))
        //         ))
        //         .await,
        //     "IntersectNotFound never reported"
        // );
    }

    /// The network manager replaces a peer once it hears it is gone. Here the peer drops the
    /// connection; the stack is still running, so only `InitiatorBehavior` could tell. In
    /// pallas-network2 1.4.0 it does not: it resets the peer and reconnects it silently.
    /// The network manager then still counts the dropped peer as live and waits forever for
    /// what it asked from it (headers, blocks), where the former pallas-network stack moved on to
    /// another peer. Waiting for:
    /// - <https://github.com/txpipe/pallas/pull/842>: `InitiatorEvent::PeerDisconnected`;
    /// - <https://github.com/txpipe/pallas/pull/843>: the keepalive timeout, so that a peer
    ///   that goes silent without closing the connection is dropped too.
    #[tokio::test]
    #[ignore = "pallas-network2 1.4.0 reports no disconnect; waiting for txpipe/pallas#842 and #843"]
    async fn lost_connection_is_reported() {
        let mut harness = Harness::new();
        let _connection = harness.open_with::<DroppingPeer>(0, "peer.test:3001");
        assert!(
            harness.receives(|_, event| matches!(event, PeerEvent::Disconnected)).await,
            "disconnection never reported"
        );
    }

    /// A peer whose headers we cannot decode cannot be followed: its connection is dropped
    /// and reported, so the network manager replaces it.
    #[tokio::test]
    async fn undecodable_header_drops_the_peer() {
        let mut harness = Harness::new();
        let connection = harness.open_with::<GarbledHeaderPeer>(0, "peer.test:3001");
        connection.find_intersect(vec![Point::Origin]).unwrap();
        assert!(
            harness.receives(|_, event| matches!(event, PeerEvent::Disconnected)).await,
            "peer with an undecodable header not dropped"
        );
    }

    /// Same for a block we cannot decode.
    #[tokio::test]
    async fn undecodable_block_drops_the_peer() {
        let mut harness = Harness::new();
        let connection = harness.open_with::<GarbledBlockPeer>(0, "peer.test:3001");
        connection.request_block(block_hash(), BLOCK_SLOT).unwrap();
        assert!(
            harness.receives(|_, event| matches!(event, PeerEvent::Disconnected)).await,
            "peer with an undecodable block not dropped"
        );
    }

    /// The network manager asks a block from a peer that announced it: only announcers are
    /// sure to hold it (forks, lagging peers), in-flight requests are re-sent to other
    /// announcers when a peer drops, and consensus sanctions the announcers of rejected
    /// blocks. The asked peer refuses the handshake so that, with a shared stack, the other
    /// peer would be the only candidate.
    #[tokio::test]
    async fn block_is_requested_only_from_a_peer_that_announced_it() {
        let mut harness = Harness::new();
        let asked = harness.open_with::<RefusingPeer>(0, "asked.test:3001");
        let other = harness.open_with::<BystanderPeer>(1, "other.test:3001");
        other.find_intersect(vec![Point::Origin]).unwrap();
        assert!(
            harness.receives(|_, event| is_roll_forward_to_block(event)).await,
            "other peer not syncing"
        );

        asked.request_block(block_hash(), BLOCK_SLOT).unwrap();
        sleep(WAIT).await;
        assert_eq!(
            BLOCKS_ASKED_FROM_BYSTANDER.load(Ordering::SeqCst),
            0,
            "block asked from one peer was requested from another"
        );
    }

    /// The network manager replaces a peer once it hears it is gone, including one that never
    /// connects. Here the peer refuses the handshake; an unreachable peer ends up the same
    /// way. In pallas-network2 1.4.0, `InitiatorBehavior` retries the peer and then bans it
    /// without any event, so the peer keeps a hot slot. The old stack reported a failed or
    /// timed-out connect (`connect-timeout-secs`, TCP connect + handshake). Waiting for:
    /// - <https://github.com/txpipe/pallas/pull/842>: `InitiatorEvent::PeerDisconnected`,
    ///   also emitted for a refused connect;
    /// - <https://github.com/txpipe/pallas/pull/843>: TCP connect and handshake timeouts, so
    ///   that an unreachable peer is dropped, and then reported, in bounded time.
    #[tokio::test]
    #[ignore = "pallas-network2 1.4.0 reports no failed connect and has no connect timeout; waiting for txpipe/pallas#842 and #843"]
    async fn peer_that_never_connects_is_reported() {
        let mut harness = Harness::new();
        let _connection = harness.open_with::<RefusingPeer>(0, "peer.test:3001");
        assert!(
            harness.receives(|_, event| matches!(event, PeerEvent::Disconnected)).await,
            "peer that never connected not reported"
        );
    }
}
