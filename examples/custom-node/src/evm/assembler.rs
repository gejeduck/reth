use crate::{
    chainspec::CustomChainSpec,
    primitives::{Block, BlockBody, CustomHeader, CustomTransaction},
};
use alloy_consensus::{
    constants::EMPTY_WITHDRAWALS, proofs, Header, TxReceipt, EMPTY_OMMER_ROOT_HASH,
};
use alloy_eips::{eip7685::EMPTY_REQUESTS_HASH, merge::BEACON_NONCE};
use alloy_evm::block::{BlockExecutionError, BlockExecutionResult, BlockExecutorFactory};
use alloy_op_evm::OpBlockExecutionCtx;
use alloy_primitives::logs_bloom;
use reth_ethereum::{
    evm::primitives::execute::{BlockAssembler, BlockAssemblerInput},
    primitives::Receipt,
};
use reth_op::DepositReceipt;
use reth_optimism_consensus::{calculate_receipt_root_no_memo_optimism, isthmus};
use reth_optimism_forks::OpHardforks;

#[derive(Clone, Debug)]
pub struct CustomBlockAssembler {
    chain_spec: CustomChainSpec,
}

impl<F> BlockAssembler<F> for CustomBlockAssembler
where
    F: for<'a> BlockExecutorFactory<
        ExecutionCtx<'a> = OpBlockExecutionCtx,
        Transaction = CustomTransaction,
        Receipt: Receipt + DepositReceipt,
    >,
{
    type Block = Block;

    fn assemble_block(
        &self,
        input: BlockAssemblerInput<'_, '_, F, CustomHeader>,
    ) -> Result<Self::Block, BlockExecutionError> {
        let BlockAssemblerInput {
            evm_env,
            execution_ctx: ctx,
            transactions,
            output: BlockExecutionResult { receipts, gas_used, .. },
            bundle_state,
            state_root,
            state_provider,
            ..
        } = input;

        let timestamp = evm_env.block_env.timestamp;

        let transactions_root = proofs::calculate_transaction_root(&transactions);
        let receipts_root =
            calculate_receipt_root_no_memo_optimism(receipts, &self.chain_spec, timestamp);
        let logs_bloom = logs_bloom(receipts.iter().flat_map(|r| r.logs()));

        let mut requests_hash = None;

        let withdrawals_root = if self.chain_spec.is_isthmus_active_at_timestamp(timestamp) {
            // always empty requests hash post isthmus
            requests_hash = Some(EMPTY_REQUESTS_HASH);

            // withdrawals root field in block header is used for storage root of L2 predeploy
            // `l2tol1-message-passer`
            Some(
                isthmus::withdrawals_root(bundle_state, state_provider)
                    .map_err(BlockExecutionError::other)?,
            )
        } else if self.chain_spec.is_canyon_active_at_timestamp(timestamp) {
            Some(EMPTY_WITHDRAWALS)
        } else {
            None
        };

        let (excess_blob_gas, blob_gas_used) =
            if self.chain_spec.is_ecotone_active_at_timestamp(timestamp) {
                (Some(0), Some(0))
            } else {
                (None, None)
            };

        let header = Header {
            parent_hash: ctx.parent_hash,
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            beneficiary: evm_env.block_env.beneficiary,
            state_root,
            transactions_root,
            receipts_root,
            withdrawals_root,
            logs_bloom,
            timestamp,
            mix_hash: evm_env.block_env.prevrandao.unwrap_or_default(),
            nonce: BEACON_NONCE.into(),
            base_fee_per_gas: Some(evm_env.block_env.basefee),
            number: evm_env.block_env.number,
            gas_limit: evm_env.block_env.gas_limit,
            difficulty: evm_env.block_env.difficulty,
            gas_used: *gas_used,
            extra_data: ctx.extra_data,
            parent_beacon_block_root: ctx.parent_beacon_block_root,
            blob_gas_used,
            excess_blob_gas,
            requests_hash,
        };

        Ok(Block::new(
            header.into(),
            BlockBody {
                transactions,
                ommers: Default::default(),
                withdrawals: self
                    .chain_spec
                    .is_canyon_active_at_timestamp(timestamp)
                    .then(Default::default),
            },
        ))
    }
}
