//! Peer-sharing mini-protocol client for discovering new peer addresses.
//!
//! Primary reference: Ouroboros Network Specification.
use minicbor::Decoder;
use pallas::network::{
    miniprotocols::handshake::{self, n2n::VersionData},
    multiplexer::{Bearer, Plexer},
};
use std::collections::HashMap;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
    time::Duration,
};
use tracing::{debug, warn};

/// Node-to-node mux mini-protocol number for peer-sharing.
const PROTOCOL_N2N_PEER_SHARING: u16 = 10;

/// Peer-sharing mode value sent during the V11+ handshake.
/// Pallas 0.35 hardcodes `PEER_SHARING_DISABLED (0)` in all convenience
/// constructors, so we build the version table ourselves.
const PEER_SHARING_ENABLED: u8 = 1;

/// Errors that can occur during a peer-sharing exchange.
#[derive(thiserror::Error, Debug)]
pub enum PeerSharingError {
    /// CBOR decode error from a malformed response.
    #[error("CBOR decode error: {0}")]
    CborDecode(String),
    /// Handshake negotiation failed (e.g. incompatible versions).
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),
    /// TCP connection to the peer failed.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// The full peer-sharing exchange did not complete within the timeout.
    #[error("Peer-sharing exchange timed out")]
    Timeout,
}

/// Validate and normalise a peer address string + port.
pub fn validate_and_normalise(
    addr_str: &str,
    port: u16,
    ipv6_enabled: bool,
    allow_non_public_peer_addrs: bool,
    localhost_gateway_ip: Option<IpAddr>,
) -> Option<String> {
    if port == 0 {
        return None;
    }
    let ip = IpAddr::from_str(addr_str).ok()?;
    let ip = normalise_ip(ip, localhost_gateway_ip);
    if !ipv6_enabled && ip.is_ipv6() {
        return None;
    }
    if is_rejected(&ip, allow_non_public_peer_addrs) {
        return None;
    }
    Some(match ip {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    })
}

/// Normalise IPv4-mapped IPv6 (`::ffff:x.x.x.x`) to its IPv4 form, then rewrite
/// `localhost_gateway_ip` to localhost if configured.
fn normalise_ip(ip: IpAddr, localhost_gateway_ip: Option<IpAddr>) -> IpAddr {
    let ip = if let IpAddr::V6(v6) = ip
        && let Some(v4) = v6.to_ipv4_mapped()
    {
        IpAddr::V4(v4)
    } else {
        ip
    };
    if localhost_gateway_ip == Some(ip) {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        ip
    }
}

fn is_rejected(ip: &IpAddr, allow_non_public_peer_addrs: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_unspecified()  // 0.0.0.0
                || (!allow_non_public_peer_addrs && (v4.is_loopback() || v4.is_private()))
                || v4.is_link_local() // 169.254.0.0/16
        }
        IpAddr::V6(v6) => {
            v6.is_unspecified() // ::
                || (!allow_non_public_peer_addrs && v6.is_loopback())
                || is_v6_link_local(v6) // fe80::/10
        }
    }
}

fn is_v6_link_local(v6: &Ipv6Addr) -> bool {
    // fe80::/10 — first 10 bits are 1111111010
    let segs = v6.segments();
    (segs[0] & 0xffc0) == 0xfe80
}

// ---- CBOR message encoding / decoding ----
// Peer-sharing protocol messages:
//   msgShareRequest = [0, base.word8]
//   msgSharePeers   = [1, peerAddresses]
//   msgDone         = [2]
//   peerAddress     = [0, word32, portNumber] | [1, word32×4, portNumber]
fn encode_request(amount: u8) -> Vec<u8> {
    // CBOR: array(2) = 0x82, uint(0) = 0x00, uint(amount)
    if amount <= 23 {
        // CBOR major type 0: values 0..=23 use one byte; 24..=255 need 0x18 + byte
        vec![0x82, 0x00, amount]
    } else {
        vec![0x82, 0x00, 0x18, amount] // 0x18: uint follows in next byte
    }
}

fn encode_done() -> Vec<u8> {
    // CBOR: array(1) = 0x81, uint(2) = 0x02
    vec![0x81, 0x02]
}

/// Decode a `MsgSharePeers` CBOR response, accepting at most `limit` addresses.
///
/// CBOR format: `[1, [[addr_type, ip_bytes, port], ...]]`
/// Extra entries beyond `limit` are skipped without allocation.
fn decode_response(
    bytes: &[u8],
    limit: usize,
    ipv6_enabled: bool,
    allow_non_public_peer_addrs: bool,
    localhost_gateway_ip: Option<IpAddr>,
) -> Result<Vec<String>, PeerSharingError> {
    let mut dec = Decoder::new(bytes);

    // Outer array (message envelope): must have length 2
    let outer_len = dec.array().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
    if outer_len != Some(2) {
        return Err(PeerSharingError::CborDecode(format!(
            "expected outer array len 2, got {outer_len:?}"
        )));
    }

    // Message type tag: must be 1 (MsgSharePeers)
    let tag: u64 = dec.u64().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
    if tag != 1 {
        return Err(PeerSharingError::CborDecode(format!(
            "expected MsgSharePeers tag 1, got {tag}"
        )));
    }

    // Inner array: list of peer entries
    let entries_len = dec.array().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
    let total = entries_len.unwrap_or(u64::MAX) as usize;

    let mut results = Vec::new();

    for _ in 0..total {
        // Try to begin reading an entry; on any decode error, stop (indefinite arrays use
        // break code 0xff, which minicbor surfaces as an error on the next array() call).
        let entry_len = match dec.array() {
            Ok(Some(n)) => n,
            _ => break, // break code or end of input
        };

        // Once we have enough valid addresses, keep the decoder aligned but
        // skip excess entries without decoding fields or allocating strings.
        if results.len() >= limit {
            for _ in 0..entry_len {
                dec.skip().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
            }
            continue;
        }

        let addr_type: u64 = dec.u64().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;

        let (addr_str, port) = match (addr_type, entry_len) {
            // IPv4: [0, word32, word16]
            (0, 3) => {
                let ip = dec.u32().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                let port = dec.u16().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                // Haskell encodes the raw HostAddress Word32 (network byte order)
                // without ntohl; on LE hosts the CBOR value has reversed octets.
                let octets = ip.to_le_bytes();
                (
                    Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]).to_string(),
                    port,
                )
            }
            // IPv6: [1, word32, word32, word32, word32, word16]
            (1, 6) => {
                let w0 = dec.u32().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                let w1 = dec.u32().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                let w2 = dec.u32().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                let w3 = dec.u32().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                let port = dec.u16().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                // Same LE byte-order issue as IPv4: each word is NBO without ntohl.
                let mut octets = [0u8; 16];
                octets[0..4].copy_from_slice(&w0.to_le_bytes());
                octets[4..8].copy_from_slice(&w1.to_le_bytes());
                octets[8..12].copy_from_slice(&w2.to_le_bytes());
                octets[12..16].copy_from_slice(&w3.to_le_bytes());
                (Ipv6Addr::from(octets).to_string(), port)
            }
            _ => {
                // Unknown addr_type/entry_len — skip remaining fields to keep decoder aligned
                for _ in 1..entry_len {
                    dec.skip().map_err(|e| PeerSharingError::CborDecode(e.to_string()))?;
                }
                continue;
            }
        };

        if let Some(normalised) =
            validate_and_normalise(&addr_str, port, ipv6_enabled, allow_non_public_peer_addrs, localhost_gateway_ip)
        {
            results.push(normalised);
        }
    }

    Ok(results)
}

/// Request peer addresses from the peer at `address` using the peer-sharing mini-protocol.
pub async fn request_peers(
    address: &str,
    magic: u32,
    amount: u8,
    timeout: Duration,
    ipv6_enabled: bool,
    allow_non_public_peer_addrs: bool,
    localhost_gateway_ip: Option<IpAddr>,
) -> Result<Vec<String>, PeerSharingError> {
    match tokio::time::timeout(
        timeout,
        request_peers_inner(
            address,
            magic,
            amount,
            ipv6_enabled,
            allow_non_public_peer_addrs,
            localhost_gateway_ip,
        ),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            warn!(peer = %address, "peer-sharing exchange timed out");
            Ok(vec![])
        }
    }
}

async fn request_peers_inner(
    address: &str,
    magic: u32,
    amount: u8,
    ipv6_enabled: bool,
    allow_non_public_peer_addrs: bool,
    localhost_gateway_ip: Option<IpAddr>,
) -> Result<Vec<String>, PeerSharingError> {
    // Step 1: TCP connect
    let bearer = Bearer::connect_tcp(address)
        .await
        .map_err(|e| PeerSharingError::ConnectionFailed(e.to_string()))?;

    // Step 2: Set up multiplexer — subscribe channels before spawning
    let mut plexer = Plexer::new(bearer);
    let hs_channel = plexer.subscribe_client(0); // handshake protocol
    let mut ps_channel = plexer.subscribe_client(PROTOCOL_N2N_PEER_SHARING);
    let running = plexer.spawn();

    // Step 3: V11 handshake with PeerSharing=Enabled.
    // Pallas 0.35 hardcodes PeerSharing=Disabled(0) in all convenience constructors,
    // so we build the table manually with PeerSharing=Enabled(1).
    let versions = version_table_peer_sharing_enabled(magic as u64);
    let mut hs_client = handshake::Client::new(hs_channel);

    match hs_client.handshake(versions).await {
        Err(e) => {
            debug!(peer = %address, error = %e, "V11 handshake failed (peer may not support V11+)");
            running.abort().await;
            return Ok(vec![]);
        }
        Ok(handshake::Confirmation::Rejected(reason)) => {
            debug!(peer = %address, ?reason, "V11 handshake rejected by peer");
            running.abort().await;
            return Ok(vec![]);
        }
        Ok(_) => {} // Accepted — proceed
    }

    // Step 4: Send MsgShareRequest
    let request_bytes = encode_request(amount);
    ps_channel
        .enqueue_chunk(request_bytes)
        .await
        .map_err(|e| PeerSharingError::ConnectionFailed(e.to_string()))?;

    // Step 5: Receive MsgSharePeers — accumulate chunks until we can decode
    let mut buffer = Vec::new();
    loop {
        let chunk = ps_channel
            .dequeue_chunk()
            .await
            .map_err(|e| PeerSharingError::ConnectionFailed(e.to_string()))?;
        buffer.extend_from_slice(&chunk);

        // Try decoding — succeed on valid CBOR, continue if incomplete
        match decode_response(
            &buffer,
            amount as usize,
            ipv6_enabled,
            allow_non_public_peer_addrs,
            localhost_gateway_ip,
        ) {
            Ok(addrs) => {
                // Step 6: Send MsgDone (best-effort)
                let _ = ps_channel.enqueue_chunk(encode_done()).await;
                running.abort().await;
                return Ok(addrs);
            }
            Err(PeerSharingError::CborDecode(_)) if !looks_complete(&buffer) => {
                // Incomplete CBOR — wait for more chunks
                continue;
            }
            Err(e) => {
                running.abort().await;
                return Err(e);
            }
        }
    }
}

/// Heuristic: enough bytes that a minimal valid `MsgSharePeers` could be complete.
/// Smallest encoding is `[1, []]` → `0x82 0x01 0x80` (array(2), uint 1, empty array) = 3 bytes.
/// Used to distinguish "incomplete input" from "malformed CBOR".
fn looks_complete(buf: &[u8]) -> bool {
    !buf.is_empty() && buf.len() >= 3
}

/// Build a V11–V14 version table with `PeerSharing = Enabled (1)`.
fn version_table_peer_sharing_enabled(network_magic: u64) -> handshake::n2n::VersionTable {
    let values: HashMap<u64, VersionData> = (11..=14)
        .map(|v| {
            (
                v,
                VersionData::new(network_magic, true, Some(PEER_SHARING_ENABLED), Some(false)),
            )
        })
        .collect();
    handshake::n2n::VersionTable { values }
}
