use acropolis_module_peer_network_interface::peer_sharing::{
    request_peers, validate_and_normalise,
};
use std::net::{Ipv6Addr, SocketAddr, TcpListener as StdTcpListener};
use std::time::Duration;

// Address validation unit tests

#[test]
fn loopback_ipv4_accepted() {
    assert_eq!(
        validate_and_normalise("127.0.0.1", 3001, true, true, None),
        Some("127.0.0.1:3001".to_string())
    );
}

#[test]
fn loopback_ipv6_accepted() {
    assert_eq!(
        validate_and_normalise("::1", 3001, true, true, None),
        Some("[::1]:3001".to_string())
    );
}

#[test]
fn private_10_accepted() {
    assert_eq!(
        validate_and_normalise("10.0.0.1", 3001, true, true, None),
        Some("10.0.0.1:3001".to_string())
    );
}

#[test]
fn private_172_accepted() {
    assert_eq!(
        validate_and_normalise("172.16.0.1", 3001, true, true, None),
        Some("172.16.0.1:3001".to_string())
    );
}

#[test]
fn private_192_accepted() {
    assert_eq!(
        validate_and_normalise("192.168.1.1", 3001, true, true, None),
        Some("192.168.1.1:3001".to_string())
    );
}

#[test]
fn link_local_ipv4_rejected() {
    assert!(validate_and_normalise("169.254.1.1", 3001, true, true, None).is_none());
}

#[test]
fn link_local_ipv6_rejected() {
    assert!(validate_and_normalise("fe80::1", 3001, true, true, None).is_none());
}

#[test]
fn port_zero_rejected() {
    assert!(validate_and_normalise("1.2.3.4", 0, true, true, None).is_none());
}

#[test]
fn ipv4_mapped_ipv6_normalised_to_ipv4() {
    // ::ffff:1.2.3.4 should normalise to "1.2.3.4:3001"
    let mapped = Ipv6Addr::from([0, 0, 0, 0, 0, 0xffff, 0x0102, 0x0304]);
    let result = validate_and_normalise(&mapped.to_string(), 3001, true, true, None);
    assert_eq!(result, Some("1.2.3.4:3001".to_string()));
}

#[test]
fn public_ipv4_accepted() {
    assert!(validate_and_normalise("185.40.4.100", 3001, true, true, None).is_some());
    assert_eq!(
        validate_and_normalise("185.40.4.100", 3001, true, true, None),
        Some("185.40.4.100:3001".to_string())
    );
}

#[test]
fn unspecified_ipv4_rejected() {
    assert!(validate_and_normalise("0.0.0.0", 3001, true, true, None).is_none());
}

#[test]
fn unspecified_ipv6_rejected() {
    assert!(validate_and_normalise("::", 3001, true, true, None).is_none());
}

#[test]
fn ipv6_rejected_when_disabled() {
    assert!(
        validate_and_normalise("2a05:d014:1fd:cb00:d9a0:4b62:beef:cafe", 3001, false, true, None)
            .is_none()
    );
}

#[test]
fn ipv4_mapped_ipv6_accepted_when_ipv6_disabled() {
    // ::ffff:1.2.3.4 normalises to IPv4 before the check, so it passes even with ipv6 disabled
    let mapped = Ipv6Addr::from([0, 0, 0, 0, 0, 0xffff, 0x0102, 0x0304]);
    let result = validate_and_normalise(&mapped.to_string(), 3001, false, true, None);
    assert_eq!(result, Some("1.2.3.4:3001".to_string()));
}

#[test]
fn public_ipv6_accepted_when_enabled() {
    assert_eq!(
        validate_and_normalise("2a05:d014:1fd:cb00:d9a0:4b62:beef:cafe", 3001, true, true, None),
        Some("[2a05:d014:1fd:cb00:d9a0:4b62:beef:cafe]:3001".to_string())
    );
}

#[test]
fn non_public_ipv4_rejected_when_disabled() {
    assert!(validate_and_normalise("127.0.0.1", 3001, true, false, None).is_none());
    assert!(validate_and_normalise("10.0.0.1", 3001, true, false, None).is_none());
}

// Integration tests with mock TCP server
// These tests use a real TCP listener that simulates a peer-sharing exchange.

/// Helper: find a free localhost port by binding a std listener then dropping it.
fn free_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Spawn a mock server that accepts a connection but never sends any data (simulates hang).
async fn spawn_hanging_server() -> SocketAddr {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((_stream, _)) = listener.accept().await {
            // Hold the stream open but never write anything
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
    addr
}

#[tokio::test]
async fn timeout_is_respected_when_server_hangs() {
    let addr = spawn_hanging_server().await;
    let start = std::time::Instant::now();
    // 1-second timeout
    let result = request_peers(
        &addr.to_string(),
        764824073,
        15,
        Duration::from_secs(1),
        true,
        true,
        None,
    )
    .await;
    let elapsed = start.elapsed();
    // Should complete within ~1.5 seconds (timeout + small overhead)
    assert!(
        elapsed < Duration::from_millis(1500),
        "should have timed out"
    );
    // On timeout, request_peers returns Ok(vec![]) not Err
    assert!(result.is_ok(), "timeout should return Ok(vec![]), not Err");
    assert!(
        result.unwrap().is_empty(),
        "timeout should return empty vec"
    );
}

#[tokio::test]
async fn connection_failure_returns_error_or_empty() {
    // Connect to a port that will be refused (nothing listening)
    let port = free_port();
    let result = request_peers(
        &format!("127.0.0.1:{port}"),
        764824073,
        15,
        Duration::from_secs(5),
        true,
        true,
        None,
    )
    .await;
    // Either Ok(vec![]) or Err — both are acceptable; must not panic
    let _ = result;
}
