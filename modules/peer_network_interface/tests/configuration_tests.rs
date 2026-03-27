use acropolis_module_peer_network_interface::peer_manager::PeerManagerConfig;

#[test]
fn default_config_has_expected_peer_counts() {
    let config = PeerManagerConfig::default();
    assert_eq!(config.target_peer_count, 15);
    assert_eq!(config.min_hot_peers, 3);
    assert!(config.peer_sharing_enabled);
    assert_eq!(config.churn_interval_secs, 600);
    assert_eq!(config.peer_sharing_timeout_secs, 10);
}
