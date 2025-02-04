use crate::core::MultiToken;
// use near_sdk::storage_management::{StorageBalance, StorageBalanceBounds, StorageManagement};

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{assert_one_yocto, env, log, require, AccountId, Balance, Promise};

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(feature = "abi", derive(schemars::JsonSchema))]
pub struct StorageBalance {
    pub total: U128,
    pub available: U128,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(feature = "abi", derive(schemars::JsonSchema))]
pub struct StorageBalanceBounds {
    pub min: U128,
    pub max: Option<U128>,
}

impl MultiToken {
    /// Internal method that returns the Account ID and the balance in case the account was
    /// unregistered.
    pub fn internal_storage_unregister(
        &mut self,
        force: Option<bool>,
    ) -> Option<(AccountId, Balance)> {
        assert_one_yocto();
        let account_id = env::predecessor_account_id();
        let force = force.unwrap_or(false);
        if let Some(balance) = self.accounts_storage.get(&account_id) {
            let tokens_amount = self.get_tokens_amount(&account_id);
            if tokens_amount == 0 || force {
                self.accounts_storage.remove(&account_id);
                Promise::new(account_id.clone()).transfer(balance);
                Some((account_id, balance))
            } else {
                env::panic_str(
                    "Can't unregister the account with the positive amount of tokens without force",
                )
            }
        } else {
            log!("The account {} is not registered", &account_id);
            None
        }
    }

    fn storage_cost(&self, account_id: &AccountId) -> Balance {
        if let Some(user_tokens) = self.tokens_per_owner.get(account_id) {
            return (user_tokens.len() * self.storage_usage_per_token + self.account_storage_usage)
                as Balance
                * env::storage_byte_cost();
        }

        (self.account_storage_usage + self.storage_usage_per_token) as Balance
            * env::storage_byte_cost()
    }

    fn get_tokens_amount(&self, account_id: &AccountId) -> u64 {
        if let Some(user_tokens) = self.tokens_per_owner.get(account_id) {
            return user_tokens.len();
        }

        0
    }

    pub fn assert_storage_usage(&self, account_id: &AccountId) {
        let storage_cost = self.storage_cost(account_id);
        let storage_balance = self.accounts_storage.get(account_id);
        if let Some(balance) = storage_balance {
            if balance < storage_cost {
                env::panic_str(
                    format!(
                        "The account doesn't have enough storage balance. Balance {}, required {}",
                        balance, storage_cost
                    )
                    .as_str(),
                );
            }
        } else {
            env::panic_str("The account is not registered");
        }
    }

    fn internal_withdraw_near(
        &mut self,
        account_id: &AccountId,
        amount: Option<Balance>,
    ) -> Balance {
        let balance = self.accounts_storage.get(account_id).unwrap_or_else(|| {
            env::panic_str(format!("The account {} is not registered", account_id).as_str())
        });
        let amount = amount.unwrap_or(balance);
        require!(amount > 0, "Zero withdraw");

        let new_storage_balance = balance
            .checked_sub(amount)
            .unwrap_or_else(|| env::panic_str("Not enough balance to withdraw"));
        self.accounts_storage
            .insert(account_id, &new_storage_balance);
        new_storage_balance
    }
}

// impl StorageManagement for MultiToken {
impl MultiToken {
    #[allow(unused_variables)]
    pub fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let amount: Balance = env::attached_deposit();
        let account_id = account_id.unwrap_or_else(env::predecessor_account_id);
        if self.accounts_storage.contains_key(&account_id) && registration_only.is_some() {
            log!("The account is already registered, refunding the deposit");
            if amount > 0 {
                Promise::new(env::predecessor_account_id()).transfer(amount);
            }
        } else {
            let min_balance: u128 = self.storage_balance_bounds().min.into();
            if amount < min_balance {
                env::panic_str("The attached deposit is less than the minimum storage balance");
            }

            let current_amount = self.accounts_storage.get(&account_id).unwrap_or(0);
            self.accounts_storage
                .insert(&account_id, &(amount + current_amount));
        }
        self.storage_balance_of(account_id.clone()).unwrap()
    }

    pub fn storage_withdraw(&mut self, amount: Option<U128>) -> StorageBalance {
        assert_one_yocto();
        let predecessor_account_id = env::predecessor_account_id();
        let to_withdraw =
            self.internal_withdraw_near(&predecessor_account_id, amount.map(|a| a.into()));
        Promise::new(predecessor_account_id.clone()).transfer(to_withdraw);
        self.storage_balance_of(predecessor_account_id).unwrap()
    }

    pub fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        self.internal_storage_unregister(force).is_some()
    }

    pub fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        let required_storage_balance = Balance::from(self.account_storage_usage)
            * env::storage_byte_cost()
            + Balance::from(self.storage_usage_per_token) * env::storage_byte_cost();
        StorageBalanceBounds {
            min: required_storage_balance.into(),
            // The max amount of storage is unlimited, because we don't know the amount of tokens
            max: None,
        }
    }

    pub fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.accounts_storage
            .get(&account_id)
            .map(|account_balance| StorageBalance {
                total: account_balance.into(),
                available: account_balance
                    .saturating_sub(self.storage_cost(&account_id))
                    .into(),
            })
    }
}
