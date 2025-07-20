pub struct {
    context: Arc<Context<Message>>,
    config: Arc<Config>,
    peer: &mut PeerClient,
    cache: &mut UpstreamCache,

    last_epoch: Option<u64>,
}