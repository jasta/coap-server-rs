use coap_lite::{BlockHandler, BlockHandlerConfig};

pub fn new_block_handler<Endpoint: Ord + Clone>(mtu: Option<u32>) -> BlockHandler<Endpoint> {
    let mut config = BlockHandlerConfig::default();
    if let Some(mtu) = mtu {
        if let Ok(mtu) = usize::try_from(mtu) {
            config.max_total_message_size = mtu;
        }
    }
    BlockHandler::new(config)
}
