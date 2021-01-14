use bitcoin::hash_types::*;
use bitcoincore_rpc_async as rpc;
use rpc::RpcApi;
use sapio_base::txindex::{TxIndex, TxIndexError};
use std::sync::Arc;
pub struct BitcoinNodeIndex {
    client: rpc::Client,
    runtime: tokio::runtime::Runtime,
    can_add: bool,
}

type Result<T> = std::result::Result<T, TxIndexError>;
impl TxIndex for BitcoinNodeIndex {
    fn lookup_tx(&self, b: &Txid) -> Result<Arc<bitcoin::Transaction>> {
        tokio::task::block_in_place(|| {
            self.runtime
                .block_on(self.client.get_raw_transaction(b, None))
                .map(Arc::new)
                .map_err(|e| {
                    let b: Box<dyn std::error::Error> = Box::new(e);
                    TxIndexError::RpcError(b)
                })
        })
    }
    fn add_tx(&self, tx: Arc<bitcoin::Transaction>) -> Result<Txid> {
        let txid = tx.txid();
        if self.can_add {
            tokio::task::block_in_place(|| {
                self.runtime
                    .block_on(self.client.send_raw_transaction(&*tx))
            })
            .map_err(|e| {
                let b: Box<dyn std::error::Error> = Box::new(e);
                TxIndexError::RpcError(b)
            })
        } else {
            Ok(txid)
        }
    }
}
