extern crate serde;
extern crate serde_json;
extern crate router;
extern crate bodyparser;
extern crate iron;

use exonum::blockchain::{self, Service, Transaction, ApiContext};
use exonum::node::{TransactionSend, ApiSender, NodeChannel};
use exonum::messages::{RawTransaction, FromRaw, Message};
use exonum::storage::{Fork, MapIndex};
use exonum::crypto::{PublicKey, Hash};
use exonum::encoding::{self, Field};
use exonum::api::{Api, ApiError};
use exonum::helpers::fabric::{ServiceFactory, Context};
use iron::prelude::*;
use iron::Handler;
use router::Router;

// Service identifier
const SERVICE_ID: u16 = 900;
// Identifier for wallet creating transaction
const TX_CREATE_WALLET_ID: u16 = 1;
// Identifier for coins transferring transaction
const TX_TRANSFER_ID: u16 = 2;
// Starting balance of a newly created wallet
const INIT_BALANCE: u64 = 100;


// Declare Persistent Data
encoding_struct! {
        struct Wallet {
            const SIZE = 48;

            field pub_key:            &PublicKey  [00 => 32]
            field name:               &str        [32 => 40]
            field balance:            u64         [40 => 48]
        }
    }

impl Wallet {
    pub fn increase(&mut self, amount: u64) {
        let balance = self.balance() + amount;
        Field::write(&balance, &mut self.raw, 40, 48);
    }

    pub fn decrease(&mut self, amount: u64) {
        let balance = self.balance() - amount;
        Field::write(&balance, &mut self.raw, 40, 48);
    }
}


// Create Schema
pub struct CurrencySchema<'a> {
    view: &'a mut Fork,
}

impl<'a> CurrencySchema<'a> {
    pub fn wallets(&mut self) -> MapIndex<&mut Fork, PublicKey, Wallet> {
        let prefix = blockchain::gen_prefix(SERVICE_ID, 0, &());
        MapIndex::new(prefix, self.view)
    }

    // Utility method to quickly get a separate wallet from the storage
    pub fn wallet(&mut self, pub_key: &PublicKey) -> Option<Wallet> {
        self.wallets().get(pub_key)
    }
}


// Define Transactions

// Creating New Wallet
message! {
        struct TxCreateWallet {
            const TYPE = SERVICE_ID;
            const ID = TX_CREATE_WALLET_ID;
            const SIZE = 40;

            field pub_key:     &PublicKey  [00 => 32]
            field name:        &str        [32 => 40]
        }
    }


// Transferring Coins
message! {
        struct TxTransfer {
            const TYPE = SERVICE_ID;
            const ID = TX_TRANSFER_ID;
            const SIZE = 80;

            field from:        &PublicKey  [00 => 32]
            field to:          &PublicKey  [32 => 64]
            field amount:      u64         [64 => 72]
            field seed:        u64         [72 => 80]
        }
    }

// Transaction Execution
impl Transaction for TxCreateWallet {
    fn verify(&self) -> bool {
        self.verify_signature(self.pub_key())
    }

    fn execute(&self, view: &mut Fork) {
        let mut schema = CurrencySchema { view };
        if schema.wallet(self.pub_key()).is_none() {
            let wallet = Wallet::new(self.pub_key(), self.name(), INIT_BALANCE);
            println!("Create the wallet: {:?}", wallet);
            schema.wallets().put(self.pub_key(), wallet)
        }
    }
}

impl Transaction for TxTransfer {
    fn verify(&self) -> bool {
        (*self.from() != *self.to()) && self.verify_signature(self.from())
    }

    fn execute(&self, view: &mut Fork) {
        let mut schema = CurrencySchema { view };
        let sender = schema.wallet(self.from());
        let receiver = schema.wallet(self.to());
        if let (Some(mut sender), Some(mut receiver)) = (sender, receiver) {
            let amount = self.amount();
            if sender.balance() >= amount {
                sender.decrease(amount);
                receiver.increase(amount);
                println!("Transfer between wallets: {:?} => {:?}", sender, receiver);
                let mut wallets = schema.wallets();
                wallets.put(self.from(), sender);
                wallets.put(self.to(), receiver);
            }
        }
    }
}


// Implement API
#[derive(Clone)]
struct CryptocurrencyApi {
    channel: ApiSender<NodeChannel>,
}

#[serde(untagged)]
#[derive(Clone, Serialize, Deserialize)]
enum TransactionRequest {
    CreateWallet(TxCreateWallet),
    Transfer(TxTransfer),
}

impl Into<Box<Transaction>> for TransactionRequest {
    fn into(self) -> Box<Transaction> {
        match self {
            TransactionRequest::CreateWallet(trans) => Box::new(trans),
            TransactionRequest::Transfer(trans) => Box::new(trans),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TransactionResponse {
    tx_hash: Hash,
}

impl Api for CryptocurrencyApi {
    fn wire(&self, router: &mut Router) {
        let self_ = self.clone();
        let tx_handler = move |req: &mut Request| -> IronResult<Response> {
            match req.get::<bodyparser::Struct<TransactionRequest>>() {
                Ok(Some(tx)) => {
                    let tx: Box<Transaction> = tx.into();
                    let tx_hash = tx.hash();
                    self_.channel.send(tx).map_err(|e| ApiError::Events(e))?;
                    let json = TransactionResponse { tx_hash };
                    self_.ok_response(&serde_json::to_value(&json).unwrap())
                }
                Ok(None) => Err(ApiError::IncorrectRequest("Empty request body".into()))?,
                Err(e) => Err(ApiError::IncorrectRequest(Box::new(e)))?,
            }
        };

        // Bind the transaction handler to a specific route.
        let route_post = "/v1/wallets/transaction";
        router.post(&route_post, tx_handler, "transaction");
    }
}


// Define Service
pub struct CurrencyService;

impl CurrencyService {
    pub fn new() -> CurrencyService {
        CurrencyService {}
    }
}

impl Service for CurrencyService {
    fn service_name(&self) -> &'static str {
        "cryptocurrency"
    }

    fn service_id(&self) -> u16 {
        SERVICE_ID
    }

    fn tx_from_raw(&self, raw: RawTransaction) -> Result<Box<Transaction>, encoding::Error> {
        println!("Currency service has incoming tx");
        let trans: Box<Transaction> = match raw.message_type() {
            TX_TRANSFER_ID => Box::new(TxTransfer::from_raw(raw)?),
            TX_CREATE_WALLET_ID => Box::new(TxCreateWallet::from_raw(raw)?),
            _ => {
                return Err(encoding::Error::IncorrectMessageType {
                    message_type: raw.message_type(),
                });
            }
        };
        Ok(trans)
    }

    fn public_api_handler(&self, ctx: &ApiContext) -> Option<Box<Handler>> {
        let mut router = Router::new();
        let api = CryptocurrencyApi { channel: ctx.node_channel().clone() };
        api.wire(&mut router);
        Some(Box::new(router))
    }
}

impl ServiceFactory for CurrencyService {
    fn make_service(_: &Context) -> Box<Service> {
        Box::new(CurrencyService::new())
    }
}