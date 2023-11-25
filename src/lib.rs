use bewallet::*;

use chrono::Utc;
use electrsd::bitcoind::bitcoincore_rpc::{Auth, RpcApi};
use electrum_client::ElectrumApi;
// use elements::bitcoin::hashes::hex::ToHex;
// use elements::bitcoin::{Amount};
use elements::hash_types::BlockHash;
use log::LevelFilter;
use log::{info, warn, Metadata, Record};
use serde_json::Value;
use std::str::FromStr;
use std::sync::Once;
use std::thread;
use std::time::Duration;
use tempdir::TempDir;

const DUST_VALUE: u64 = 546;
static LOGGER: SimpleLogger = SimpleLogger;
//TODO duplicated why I cannot import?
pub struct SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!(
                "{} {} - {}",
                Utc::now().format("%S%.3f"),
                record.level(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

static START: Once = Once::new();

pub struct TestElectrumWallet {
    mnemonic: String,
    electrum_wallet: ElectrumWallet,
    tx_status: u64,
    _block_status: (u32, BlockHash),
    _db_root_dir: TempDir,
}

impl TestElectrumWallet {
    pub fn new(electrs_url: &str, mnemonic: String) -> Self {
        let tls = false;
        let validate_domain = false;
        let spv_enabled = true;
        let policy_asset_hex = &"5ac9f65c0efcc4775e0baec4ec03abdde22473cd3cf33c0419ca290e0751b225";
        let _db_root_dir = TempDir::new("electrum_integration_tests").unwrap();

        let db_root = format!("{}", _db_root_dir.path().display());

        let electrum_wallet = ElectrumWallet::new_testnet(
            electrs_url,
            tls,
            tls,
            validate_domain,
            &db_root,
            &mnemonic,
        )
        .unwrap();
        electrum_wallet.update_fee_estimates();

        let tx_status = electrum_wallet.tx_status().unwrap();
        assert_eq!(tx_status, 15130871412783076140);
        let mut i = 120;
        let _block_status = loop {
            assert!(i > 0, "1 minute without updates");
            i -= 1;
            let block_status = electrum_wallet.block_status().unwrap();
            if block_status.0 == 101 {
                break block_status;
            } else {
                thread::sleep(Duration::from_millis(500));
            }
        };
        assert_eq!(_block_status.0, 101);

        Self {
            mnemonic,
            electrum_wallet,
            tx_status,
            _block_status,
            _db_root_dir,
        }
    }

    pub fn policy_asset(&self) -> elements::issuance::AssetId {
        self.electrum_wallet.policy_asset()
    }

    /// Wait until tx appears in tx list (max 1 min)
    pub fn wait_for_tx(&mut self, txid: &str) {
        let mut opt = GetTransactionsOpt::default();
        opt.count = 100;
        for _ in 0..120 {
            let list = self.electrum_wallet.transactions(&opt).unwrap();
            if list.iter().any(|e| e.txid == txid) {
                return;
            }
            thread::sleep(Duration::from_millis(500));
        }
        panic!("Wallet does not have {} in its list", txid);
    }

    /// wait wallet tx status to change (max 1 min)
    fn wallet_wait_tx_status_change(&mut self) {
        for _ in 0..120 {
            if let Ok(new_status) = self.electrum_wallet.tx_status() {
                if self.tx_status != new_status {
                    self.tx_status = new_status;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// wait wallet block status to change (max 1 min)
    fn _wallet_wait_block_status_change(&mut self) {
        for _ in 0..120 {
            if let Ok(new_status) = self.electrum_wallet.block_status() {
                if self._block_status != new_status {
                    self._block_status = new_status;
                    break;
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// wait until wallet has a certain blockheight (max 1 min)
    pub fn wait_for_block(&mut self, new_height: u32) {
        for _ in 0..120 {
            if let Ok((height, _)) = self.electrum_wallet.block_status() {
                if height == new_height {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// asset balance in satoshi
    pub fn balance(&self, asset: &elements::issuance::AssetId) -> u64 {
        let balance = self.electrum_wallet.balance().unwrap();
        info!("balance: {:?}", balance);
        *balance.get(asset).unwrap_or(&0u64)
    }

    fn balance_btc(&self) -> u64 {
        self.balance(&self.policy_asset())
    }

    fn get_tx_from_list(&mut self, txid: &str) -> TransactionDetails {
        self.electrum_wallet.update_spv().unwrap();
        let mut opt = GetTransactionsOpt::default();
        opt.count = 100;
        let list = self.electrum_wallet.transactions(&opt).unwrap();
        let filtered_list: Vec<TransactionDetails> =
            list.iter().filter(|e| e.txid == txid).cloned().collect();
        assert!(
            !filtered_list.is_empty(),
            "just made tx {} is not in tx list",
            txid
        );
        filtered_list.first().unwrap().clone()
    }

    pub fn get_fee(&mut self, txid: &str) -> u64 {
        self.get_tx_from_list(txid).fee
    }

    pub fn fund_btc(&mut self, server: &mut TestElectrumServer) {
        let init_balance = self.balance_btc();
        let satoshi: u64 = 1_000_000;
        let address = self.electrum_wallet.address().unwrap();
        let txid = server.fund_btc(&address, satoshi);
        self.wallet_wait_tx_status_change();
        let balance = init_balance + self.balance_btc();
        // node is allowed to make tx below dust with dustrelayfee, but wallet should not see
        // this as spendable, thus the balance should not change
        let satoshi = if satoshi < DUST_VALUE {
            init_balance
        } else {
            init_balance + satoshi
        };
        assert_eq!(balance, satoshi);
        let wallet_txid = self.get_tx_from_list(&txid).txid;
        assert_eq!(txid, wallet_txid);
        let utxos = self.electrum_wallet.utxos().unwrap();
        assert_eq!(utxos.len(), 1);
    }

    pub fn fund_asset(&mut self, server: &mut TestElectrumServer) -> elements::issuance::AssetId {
        let num_utxos_before = self.electrum_wallet.utxos().unwrap().len();
        let satoshi = 10_000;
        let address = self.electrum_wallet.address().unwrap();
        let (txid, asset) = server.fund_asset(&address, satoshi);
        self.wait_for_tx(&txid);

        let balance_asset = self.balance(&asset);
        assert_eq!(balance_asset, satoshi);
        let wallet_txid = self.get_tx_from_list(&txid).txid;
        assert_eq!(txid, wallet_txid);
        let utxos = self.electrum_wallet.utxos().unwrap();
        assert_eq!(utxos.len(), num_utxos_before + 1);
        asset
    }

    /// send a tx from the wallet to the specified address
    pub fn send_tx(
        &mut self,
        address: &elements::Address,
        satoshi: u64,
        asset: Option<elements::issuance::AssetId>,
        utxos: Option<Vec<UnblindedTXO>>,
    ) -> String {
        let asset = asset.unwrap_or(self.policy_asset());
        let init_sat = self.balance(&asset);
        //let init_node_balance = self.node_balance(asset.clone());
        let mut create_opt = CreateTransactionOpt::default();
        let fee_rate = 100;
        create_opt.fee_rate = Some(fee_rate);
        let net = self.electrum_wallet.network();
        create_opt.addressees.push(
            Destination::new(&address.to_string(), satoshi, &asset.to_string(), net).unwrap(),
        );
        create_opt.utxos = utxos;
        let tx_details = self.electrum_wallet.create_tx(&mut create_opt).unwrap();
        let mut tx = tx_details.transaction.clone();
        let len_before = elements::encode::serialize(&tx).len();
        self.electrum_wallet
            .sign_tx(&mut tx, &self.mnemonic)
            .unwrap();
        let len_after = elements::encode::serialize(&tx).len();
        assert!(len_before < len_after, "sign tx did not increased tx size");
        //self.check_fee_rate(fee_rate, &signed_tx, MAX_FEE_PERCENT_DIFF);
        let txid = tx.txid().to_string();
        self.electrum_wallet.broadcast_tx(&tx).unwrap();
        self.wallet_wait_tx_status_change();

        self.tx_checks(&tx);

        let fee = if asset == self.policy_asset() {
            tx_details.fee
        } else {
            0
        };
        //assert_eq!(
        //    self.node_balance(asset.clone()),
        //    init_node_balance + satoshi,
        //    "node balance does not match"
        //);

        let expected = init_sat - satoshi - fee;
        for _ in 0..5 {
            if expected != self.balance(&asset) {
                // FIXME I should not wait again, but apparently after reconnect it's needed
                self.wallet_wait_tx_status_change();
            }
        }
        assert_eq!(self.balance(&asset), expected, "gdk balance does not match");

        //self.list_tx_contains(&txid, &vec![address.to_string()], true);
        let wallet_txid = self.get_tx_from_list(&txid).txid;
        assert_eq!(txid, wallet_txid);

        txid
    }

    pub fn send_tx_to_unconf(&mut self, server: &mut TestElectrumServer) {
        let init_sat = self.balance_btc();
        let address = self.electrum_wallet.address().unwrap();
        server.send_tx_to_unconf(&address);
        self.wallet_wait_tx_status_change();
        assert_eq!(init_sat, self.balance_btc());
    }

    pub fn is_verified(&mut self, txid: &str, verified: SPVVerifyResult) {
        let tx = self.get_tx_from_list(txid);
        assert_eq!(tx.spv_verified.to_string(), verified.to_string());
    }

    /// check create_tx failure reasons
    pub fn create_fails(&mut self, server: &mut TestElectrumServer) {
        let policy_asset = self.policy_asset();
        let init_sat = self.balance_btc();
        let mut create_opt = CreateTransactionOpt::default();
        let fee_rate = 1000;
        let address = server.node_getnewaddress(None).to_string();
        create_opt.fee_rate = Some(fee_rate);
        let net = self.electrum_wallet.network();
        create_opt.addressees =
            vec![Destination::new(&address, 0, &policy_asset.to_hex(), net).unwrap()];
        assert!(matches!(
            self.electrum_wallet.create_tx(&mut create_opt),
            Err(Error::InvalidAmount)
        ));

        create_opt.addressees =
            vec![Destination::new(&address, 200, &policy_asset.to_hex(), net).unwrap()];
        assert!(matches!(
            self.electrum_wallet.create_tx(&mut create_opt),
            Err(Error::InvalidAmount)
        ));

        create_opt.addressees = vec![Destination::new(
            &address,
            init_sat, // not enough to pay fee
            &policy_asset.to_hex(),
            net,
        )
        .unwrap()];
        assert!(matches!(
            self.electrum_wallet.create_tx(&mut create_opt),
            Err(Error::InsufficientFunds)
        ));

        assert!(matches!(
            Destination::new("x", 200, &policy_asset.to_hex(), net),
            Err(Error::InvalidAddress)
        ));

        assert!(
            matches!(
                Destination::new(
                    "38CMdevthTKYAtxaSkYYtcv5QgkHXdKKk5",
                    200,
                    &policy_asset.to_hex(),
                    net,
                ),
                Err(Error::InvalidAddress)
            ),
            "address with different network should fail"
        );

        assert!(
            matches!(
                Destination::new("VJLCbLBTCdxhWyjVLdjcSmGAksVMtabYg15maSi93zknQD2ihC38R7CUd8KbDFnV8A4hiykxnRB3Uv6d", 200, &policy_asset.to_hex(), net),
                Err(Error::InvalidAddress)
            ),
            "address with different network should fail"
        );

        // from bip173 test vectors
        assert!(
            matches!(
                Destination::new(
                    "bc1pw508d6qejxtdg4y5r3zarvary0c5xw7kw508d6qejxtdg4y5r3zarvary0c5xw7k7grplx",
                    200,
                    &policy_asset.to_hex(),
                    net,
                ),
                Err(Error::InvalidAddress)
            ),
            "segwit v1 should fail"
        );

        let mut addr = elements::Address::from_str(
            "Azpt6vXqrbPuUtsumAioGjKnvukPApDssC1HwoFdSWZaBYJrUVSe5K8x9nk2HVYiYANy9mVQbW3iQ6xU",
        )
        .unwrap();
        addr.blinding_pubkey = None;
        create_opt.addressees =
            vec![Destination::new(&addr.to_string(), 1000, &policy_asset.to_hex(), net).unwrap()];
        assert!(
            matches!(
                self.electrum_wallet.create_tx(&mut create_opt),
                Err(Error::InvalidAddress)
            ),
            "unblinded address should fail"
        );

        create_opt.addressees = vec![];
        assert!(matches!(
            self.electrum_wallet.create_tx(&mut create_opt),
            Err(Error::EmptyAddressees)
        ));
    }

    pub fn utxos(&self) -> Vec<UnblindedTXO> {
        self.electrum_wallet.utxos().unwrap()
    }

}

#[cfg(test)]
mod tests {
    use std::env;
    use electrum_client::Client;
    use bewallet::network::Config;

    use super::*;
    #[test]
    fn liquid() {
        let electrs_url = "ssl://electrum.bullbitcoin.com:50002";

        let debug = env::var("DEBUG").is_ok();
        let mnemonic = "bacon bacon bacon bacon bacon bacon bacon bacon bacon bacon bacon bacon".to_string();
        let mut wallet = TestElectrumWallet::new(&electrs_url, mnemonic);

        let node_address = server.node_getnewaddress(Some("p2sh-segwit"));
        let node_bech32_address = server.node_getnewaddress(Some("bech32"));
        let node_legacy_address = server.node_getnewaddress(Some("legacy"));

        wallet.fund_btc(&mut server);
        let asset = wallet.fund_asset(&mut server);

        let txid = wallet.send_tx(&node_address, 10_000, None, None);
        wallet.send_tx_to_unconf(&mut server);
        wallet.is_verified(&txid, SPVVerifyResult::InProgress);
        wallet.send_tx(&node_bech32_address, 1_000, None, None);
        wallet.send_tx(&node_legacy_address, 1_000, None, None);
        wallet.send_tx(&node_address, 1_000, Some(asset.clone()), None);
        wallet.send_tx(&node_address, 100, Some(asset.clone()), None); // asset should send below dust limit
        wallet.wait_for_block(server.mine_block());
        let asset1 = wallet.fund_asset(&mut server);
        let asset2 = wallet.fund_asset(&mut server);
        let asset3 = wallet.fund_asset(&mut server);
        let assets = vec![asset1, asset2, asset3];
        wallet.send_multi(3, 1_000, &vec![], &mut server);
        wallet.send_multi(10, 1_000, &assets, &mut server);
        wallet.wait_for_block(server.mine_block());
        wallet.create_fails(&mut server);
        wallet.is_verified(&txid, SPVVerifyResult::Verified);
        let utxos = wallet.utxos();
        wallet.send_tx(&node_address, 1_000, None, Some(utxos));

        server.stop();
    }
}
