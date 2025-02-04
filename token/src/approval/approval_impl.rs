use std::collections::HashMap;

use near_sdk::{assert_one_yocto, env, json_types::U128, log, require, AccountId, Gas, Promise};

use crate::approval::receiver::ext_approval_receiver;
use crate::{
    core::MultiToken,
    token::{Approval, TokenId},
    utils::{bytes_for_approved_account_id, expect_approval_for_token, refund_deposit},
};

use super::MultiTokenApproval;

pub const GAS_FOR_RESOLVE_APPROVE: Gas = Gas(15_000_000_000_000);
pub const GAS_FOR_MT_APPROVE_CALL: Gas = Gas(50_000_000_000_000 + GAS_FOR_RESOLVE_APPROVE.0);

impl MultiTokenApproval for MultiToken {
    fn mt_approve(
        &mut self,
        token_ids: Vec<TokenId>,
        amounts: Vec<U128>,
        grantee_id: AccountId,
        msg: Option<String>,
    ) -> Option<Promise> {
        let approver_id = env::predecessor_account_id();

        let mut new_approval_ids: Vec<u64> = Vec::new();

        let mut used_storage = 0;

        for (token_id, amount) in token_ids.iter().zip(&amounts) {
            // Get the balance to check if user has enough tokens
            let approver_balance = self
                .balances_per_token
                .get(token_id)
                .and_then(|balances_per_account| balances_per_account.get(&approver_id))
                .unwrap_or(0);
            require!(
                approver_balance >= amount.0,
                "Not enough balance to approve"
            );

            // Get the next approval id for the token
            let new_approval_id: u64 =
                expect_approval_for_token(self.next_approval_id_by_id.get(token_id), token_id);
            let new_approval = Approval {
                amount: amount.0,
                approval_id: new_approval_id,
            };
            log!("New approval: {:?}", new_approval);

            // Get existing approvals for this token. If one exists for the grantee_id, overwrite it.
            let mut by_owner = self.approvals_by_token_id.get(token_id).unwrap_or_default();
            let by_grantee = by_owner.get(&approver_id);
            let mut grantee_to_approval = if let Some(by_grantee) = by_grantee {
                by_grantee.clone()
            } else {
                HashMap::new()
            };

            let old_approval_id = grantee_to_approval.insert(grantee_id.clone(), new_approval);
            by_owner.insert(approver_id.clone(), grantee_to_approval);
            self.approvals_by_token_id.insert(token_id, &by_owner);
            self.next_approval_id_by_id
                .insert(token_id, &(new_approval_id + 1));

            new_approval_ids.push(new_approval_id);

            log!("Updated approvals by id: {:?}", old_approval_id);
            used_storage += if old_approval_id.is_none() {
                bytes_for_approved_account_id(&grantee_id)
            } else {
                0
            };
        }

        refund_deposit(used_storage);

        // if given `msg`, schedule call to `mt_on_approve` and return it. Else, return None.
        let receiver_gas: Gas = env::prepaid_gas()
            .0
            .checked_sub(GAS_FOR_MT_APPROVE_CALL.into())
            .unwrap_or_else(|| env::panic_str("Prepaid gas overflow"))
            .into();

        msg.map(|msg| {
            ext_approval_receiver::ext(grantee_id)
                .with_static_gas(receiver_gas)
                .mt_on_approve(token_ids, amounts, approver_id, new_approval_ids, msg)
        })
    }

    fn mt_revoke(&mut self, token_ids: Vec<TokenId>, account_id: AccountId) {
        assert_one_yocto();
        let owner_id = env::predecessor_account_id();

        for token_id in token_ids.iter() {
            // Remove approval for user & also clean maps to save space it it's empty
            let mut by_owner =
                expect_approval_for_token(self.approvals_by_token_id.get(token_id), token_id);
            let by_grantee = by_owner.get_mut(&owner_id);

            if let Some(grantee_to_approval) = by_grantee {
                grantee_to_approval.remove(&account_id);
                // The owner has no more approvals for this token.
                if grantee_to_approval.is_empty() {
                    by_owner.remove(&owner_id);
                }
            }

            if by_owner.is_empty() {
                self.approvals_by_token_id.remove(token_id);
            }
        }
    }

    fn mt_revoke_all(&mut self, token_ids: Vec<TokenId>) {
        assert_one_yocto();
        let owner_id = env::predecessor_account_id();
        for token_id in token_ids.iter() {
            let mut by_owner =
                expect_approval_for_token(self.approvals_by_token_id.get(token_id), token_id);
            by_owner.remove(&owner_id);
            self.approvals_by_token_id.insert(token_id, &by_owner);
        }
    }

    fn mt_is_approved(
        &self,
        owner_id: AccountId,
        token_ids: Vec<TokenId>,
        approved_account_id: AccountId,
        amounts: Vec<U128>,
        approval_ids: Option<Vec<u64>>,
    ) -> bool {
        let approval_ids = approval_ids.unwrap_or_default();
        require!(
            approval_ids.is_empty() || approval_ids.len() == token_ids.len(),
            "token_ids and approval_ids must have equal size"
        );

        for (idx, (token_id, amount)) in token_ids.iter().zip(amounts).enumerate() {
            let by_owner = self.approvals_by_token_id.get(token_id).unwrap_or_default();

            let approval = match by_owner
                .get(&owner_id)
                .and_then(|grantee_to_approval| grantee_to_approval.get(&approved_account_id))
            {
                Some(approval) if approval.amount.eq(&amount.into()) => approval,
                _ => return false,
            };

            if let Some(given_approval) = approval_ids.get(idx) {
                if !approval.approval_id.eq(given_approval) {
                    return false;
                }
            }
        }
        true
    }
}
