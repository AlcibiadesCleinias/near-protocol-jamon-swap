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
            greeting: "Hello".to_string(),
            // TODO: it is better to have 1 struct, but rapid development of SDK doesn't allow to find proper solution.
            // To lock the seller.
            isCurrentSeller: LookupMap::new(b"map-bool-1".to_vec()),
            // Remember amount required from Buyer to fulfill the Offer and to whom send Buyer deposit at the end.
            derivedAddressToAmount: LookupMap::new(b"map-u128-1".to_vec()),
            derivedAddressToSeller: LookupMap::new(b"map-address-1".to_vec()),
            // To store if current derived address allocated by someone.
            isDerivedAddressInOffer: LookupMap::new(b"map-bool-1".to_vec()),
        }
    }
}

#[near(contract_state)]
pub struct Contract {
    greeting: String,
    pub isCurrentSeller: LookupMap<AccountId, bool>,
    pub derivedAddressToAmount: LookupMap<String, u128>,
    pub derivedAddressToSeller: LookupMap<String, AccountId>,
    pub isDerivedAddressInOffer: LookupMap<String, bool>,
}

// versed-crib.testnet
#[near]
impl Contract {
//     TODO: Create Offer from Seller perspective.
//  Store {derived address to be used to deposit buyer money on Eth chain, requested amount from buyer on Near, }
// We use amount in derived address, thus we could use derived address like key in map.
// Restrictions: No more 1 Offer for 1 seller.
// TODO: lock some collateral from Seller to prevent attack on locked addresses.
    pub fn create_offer(&mut self, derivedAddress: String, amountFromBuyer: u128) {
        log!("Saving derivedAddress: {}", derivedAddress);
        log!("Amount from Buyer: {}", NearToken::from_near(amountFromBuyer));
// TODO: add check amountFromBuyer > 0.
// TODO: uncomment.
//         assert!(self.isCurrentSeller.contains_key(env::current_account_id()), "Seller already has an Offer.");
//         assert!(self.isDerivedAddressInOffer.contains_key(derivedAddress.clone(), "Derived Address in Use in another Offer.");

        // Lock seller.
        self.isCurrentSeller.insert(env::current_account_id(), true);
        self.derivedAddressToAmount.insert(derivedAddress.clone(), amountFromBuyer);
        self.derivedAddressToSeller.insert(derivedAddress.clone(), env::current_account_id());
        self.isDerivedAddressInOffer.insert(derivedAddress.clone(), true);
    }

// TODO: withdrawSeller Seller withdraw deposited Nears.

    pub fn get_greeting(&self) -> String {
        self.greeting.clone()
    }

    // proxy to call MPC_CONTRACT_ACCOUNT_ID method sign if COST is deposited
    // Accept Offer from Buyer perspective after ensured Seller deposited his part on Eth chain.
    // deposit Near as his part of the Offer.
    // It uses derived address as offer id
    // store address on Eth for MPC to send Eth from derived address.
    // TODO: make this call idempotent (he could call it as much as he wants, since we use 1 derived address per each offer).
    #[payable]
    pub fn sign(&mut self, rlp_payload: String, path: String, key_version: u32) -> Promise {
        let owner = env::predecessor_account_id() == env::current_account_id();

        // check deposit requirement, contract owner doesn't pay
        let deposit = env::attached_deposit();
        if !owner {
            require!(deposit >= COST, "pay the piper");
        }

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
//         .functionCall("sign", JSON.stringify({ payload, path, key_version: 0 }), NO_DEPOSIT, this.gas_to_use)
}
