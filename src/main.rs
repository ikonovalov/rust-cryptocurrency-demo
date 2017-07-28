extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate exonum;
extern crate exonum_configuration;
extern crate router;
extern crate bodyparser;
extern crate iron;

use exonum_configuration::ConfigurationService;
use exonum::helpers::fabric::NodeBuilder;


pub mod cryptocurrency;
use cryptocurrency::CurrencyService;


fn main() {
    exonum::helpers::init_logger().unwrap();
    exonum::crypto::init();

    // Create Keys
    //let (consensus_public_key, consensus_secret_key) = exonum::crypto::gen_keypair();
    //let (service_public_key, service_secret_key) = exonum::crypto::gen_keypair();

    //let mut node = Node::new(blockchain, node_cfg);
    //node.run().unwrap();

    NodeBuilder::new()
        .with_service::<ConfigurationService>()
        .with_service::<CurrencyService>()
        .run();

}
