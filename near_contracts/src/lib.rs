// Find all our documentation at https://docs.near.org
use hex::decode;
use near_sdk::{env, ext_contract, near, require, Gas, NearToken, Promise, AccountId, log};
use near_sdk::store::{LookupMap};
//  Account id: clever-shelf.testnet


const PUBLIC_RLP_ENCODED_METHOD_NAMES: [&'static str; 1] = ["6a627842000000000000000000000000"];
const MPC_CONTRACT_ACCOUNT_ID: &str = "v5.multichain-mpc-dev.testnet";
const COST: NearToken = NearToken::from_near(1);

// interface for cross contract call to mpc contract
#[ext_contract(mpc)]
trait MPC {
    fn sign(&self, payload: [u8; 32], path: String, key_version: u32) -> Promise;
}

// automatically init the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            isCurrentSeller: LookupMap::new(b"map-bool-1".to_vec()),
            derivedAddressToAmount: LookupMap::new(b"map-u128-1".to_vec()),
            derivedAddressToSeller: LookupMap::new(b"map-address-1".to_vec()),
            isDerivedAddressInOffer: LookupMap::new(b"map-bool-1".to_vec()),
            derivedAddressToIsBuyerDeposited: LookupMap::new(b"map-bool-1".to_vec()),
            derivedAddressToAvailableWithdraw: LookupMap::new(b"map-bool-1".to_vec()),
        }
    }
}

#[near(contract_state)]
pub struct Contract {
    // TODO: it is better to have 1 struct, but rapid development of SDK doesn't allow to find proper solution.
    // To lock a seller. Seller in unlocked when seller withdraws his part.
    pub isCurrentSeller: LookupMap<AccountId, bool>,
    // Remember amount required from Buyer to fulfill the Offer and to whom send Buyer deposit at the end.
    pub derivedAddressToAmount: LookupMap<String, u128>,
    pub derivedAddressToSeller: LookupMap<String, AccountId>,
    // To store if current derived address allocated by someone for an Offer.
    // Currently, only 1 derived address for 1 Offer in the history. Thus, use more salt.
    pub isDerivedAddressInOffer: LookupMap<String, bool>,
    // If Buyer deposited his part of the offer to allow Seller to withdraw this part.
    pub derivedAddressToIsBuyerDeposited: LookupMap<String, bool>,
    // To understand if Seller already withdrawn his part of the Offer.
    // if in the map - available, if not in the map - not available (kinda hack since
    //  `let isSellerWithdrawn = match self.derivedAddressToIsSellerWithdrawn.get(&key) {` returns true??).
    pub derivedAddressToAvailableWithdraw: LookupMap<String, bool>,

}

//  old-bike.testnet
#[near]
impl Contract {
    // Create Offer from Seller perspective.
    //  Store {derived address to be used to deposit buyer money on Eth chain, requested amount from buyer on Near, }
    // We use amount in derived address, thus we could use derived address like key in map.
    // Restrictions:
    // - no more 1 Offer for 1 seller,
    // - the same could be created only once.
    // TODO: lock some collateral from Seller to prevent attack on locked addresses.
    pub fn create_offer(&mut self, derivedAddress: String, expectedAmount: u128) {
        log!("[Contract] Saving derivedAddress: {}", derivedAddress);
        log!("[Contract] Amount from Buyer: {}", NearToken::from_near(expectedAmount));
        let key: String = derivedAddress.clone().to_string();
        let predecessor_account_id = env::predecessor_account_id();
// TODO: add check expectedAmount > 0.
        require!(!self.isCurrentSeller.contains_key(&predecessor_account_id), "Seller already has an Offer.");
        require!(!self.isDerivedAddressInOffer.contains_key(&key), "Derived Address in Use in another Offer.");

        log!("[Contract] Seller: current account it {}", env::current_account_id());
        log!("[Contract] Seller: env::predecessor_account_id() {}", env::predecessor_account_id());

        // Lock seller.
        self.isCurrentSeller.insert(env::predecessor_account_id(), true);
        self.derivedAddressToAmount.insert(derivedAddress.clone(), expectedAmount);
        self.derivedAddressToSeller.insert(derivedAddress.clone(), env::predecessor_account_id());
        self.isDerivedAddressInOffer.insert(derivedAddress.clone(), true);
        self.derivedAddressToIsBuyerDeposited.insert(derivedAddress.clone(), false);
    }

    // WithdrawSeller Seller withdraw deposited Nears.
    // It uses derivedAddress as Offer Id.
    pub fn withdrawBySeller(&mut self, derivedAddress: String) {
        let key: String = derivedAddress.clone().to_string();
        require!(self.derivedAddressToIsBuyerDeposited.contains_key(&key), "Derived Address not registered in offers. Or Offer already fulfilled.");
        let isBuyerDeposited = match self.derivedAddressToIsBuyerDeposited.get(&key) {
          Some(x) => x,
          None => panic!("Inconsistent state on contract for derivedAddressToIsBuyerDeposited."),
        };
        require!(isBuyerDeposited, "Buyer has not deposited to accept the Offer. Nothing to withdraw.");
        require!(self.derivedAddressToAvailableWithdraw.contains_key(&key), "This derivedAddress is not available for withdrawal. Possibly already withdrawn.");

        let amountToSend = match self.derivedAddressToAmount.get(&key) {
          Some(x) => x,
          None => panic!("Inconsistent state on contract for derivedAddressToAmount."),
        };
        let sendTo = match self.derivedAddressToSeller.get(&key) {
          Some(x) => x,
          None => panic!("Inconsistent state on contract for derivedAddressToSeller."),
        };
        log!("[Contract] Send deposited to: {}", sendTo);

        self.derivedAddressToIsBuyerDeposited.insert(derivedAddress.clone(), false);
        self.derivedAddressToAvailableWithdraw.remove(&key);
        self.isCurrentSeller.remove(sendTo);
        // Transfer Near to Seller finally.
        Promise::new(sendTo.clone()).transfer(NearToken::from_near(amountToSend.clone()));
    }

    // Accept Offer from Buyer perspective after ensured Seller deposited his part on Eth chain (not in this contract).
    // Buyer deposits Near as his part of the Offer.
    // It uses derived address as offer id.
    // This call is idempotent but require deposit to be made.
    // Under the hood there is a proxy to call MPC_CONTRACT_ACCOUNT_ID method sign.
    #[payable]
    pub fn sign(&mut self, rlp_payload: String, path: String, key_version: u32, derivedAddress: String) -> Promise {
        let key: String = derivedAddress.clone().to_string();
        require!(self.isDerivedAddressInOffer.contains_key(&key), "Derived Address not registered in offers. Or Offer already fulfilled.");

        //         TODO: checks
        let amountToDeposit = match self.derivedAddressToAmount.get(&key) {
          Some(x) => x,
          None => panic!("Inconsistent state on contract for derivedAddressToAmount."),
        };
        // Check deposit requirement.
        let deposit = env::attached_deposit();
        require!(deposit >= NearToken::from_near(amountToDeposit.clone()), "Insufficient Deposit for the Offer.");

//         let owner = env::predecessor_account_id() == env::current_account_id();

        // check if rlp encoded eth transaction is calling a public method name
        let mut public = false;
        for n in PUBLIC_RLP_ENCODED_METHOD_NAMES {
            if rlp_payload.find(n).is_some() {
                public = true
            }
        }

//         // only the Near contract owner can call sign of arbitrary payloads for chain signature accounts based on env::current_account_id()
//         if !public {
//             require!(
//                 owner,
//                 "only contract owner can sign arbitrary EVM transactions"
//             );
//         }

        // Mark Buyer deposited his part - enable Seller to withdraw his part after this call.
        // TODO: move below.
        self.derivedAddressToIsBuyerDeposited.insert(derivedAddress.clone(), true);
        self.derivedAddressToAvailableWithdraw.insert(derivedAddress.clone(), true);

        // hash and reverse rlp encoded payload
        let payload: [u8; 32] = env::keccak256_array(&decode(rlp_payload).unwrap())
            .into_iter()
            .rev()
            .collect::<Vec<u8>>()
            .try_into()
            .unwrap();

        // call mpc sign and return promise
        mpc::ext(MPC_CONTRACT_ACCOUNT_ID.parse().unwrap())
            .with_static_gas(Gas::from_tgas(250))
            .sign(payload, path, key_version)
    }
}
