//! Contract for managing movement of funds from cold to hot storage
use super::undo_send::UndoSendInternal;
use bitcoin::util::amount::CoinAmount;
use sapio::contract::*;
use sapio::*;
use sapio_base::timelocks::AnyRelTimeLock;
use schemars::*;
use serde::*;
use std::convert::{TryFrom, TryInto};
use std::rc::Rc;

/// A Vault makes a "annuity chain" which pays out to `hot_storage` every `timeout` period for `n_steps`.
/// The funds in `hot_storage` are in an UndoSend contract for a timeout of
/// `mature`. At any time the remaining funds can be moved to `cold_storage`, which may vary based on the amount.
pub struct Vault {
    cold_storage: Rc<dyn Fn(CoinAmount, &Context) -> Result<Compiled, CompilationError>>,
    hot_storage: bitcoin::Address,
    n_steps: u64,
    amount_step: CoinAmount,
    timeout: AnyRelTimeLock,
    mature: AnyRelTimeLock,
}

impl Vault {
    then! {step |s, ctx| {
        let builder = ctx.template()
        .add_output(s.amount_step.try_into()?,
                &UndoSendInternal {
                    from_contract: (s.cold_storage)(s.amount_step, ctx)?,
                    to_contract: Compiled::from_address(s.hot_storage.clone(), None),
                    timeout: s.mature,
                    amount: s.amount_step.into(),
                }, None)?
       .set_sequence(0, s.timeout)?;

        if s.n_steps > 1 {
            let sub_amount = bitcoin::Amount::try_from(s.amount_step).map_err(|_e| contract::CompilationError::TerminateCompilation)?.checked_mul(s.n_steps - 1).ok_or(contract::CompilationError::TerminateCompilation)?;
            let sub_vault = Vault {
                cold_storage: s.cold_storage.clone(),
                hot_storage: s.hot_storage.clone(),
                n_steps: s.n_steps -1,
                amount_step: s.amount_step,
                timeout: s.timeout,
                mature: s.mature,

            };
            builder.add_output(sub_amount, &sub_vault, None)?
        } else {
            builder
        }.into()
    }}
    then! {to_cold |s, ctx| {
        let amount = bitcoin::Amount::try_from(s.amount_step).map_err(|_e| contract::CompilationError::TerminateCompilation)?.checked_mul(s.n_steps).ok_or(contract::CompilationError::TerminateCompilation)?;
        ctx.template()
            .add_output(amount, &(s.cold_storage)(amount.into(), ctx)?, None)?
            .into()
    }}
}

impl Contract for Vault {
    declare! {then, Self::step, Self::to_cold}
    declare! {non updatable}
}

/// A specialization of `Vault` where cold storage is a regular `bitcoin::Address`
#[derive(JsonSchema, Deserialize)]
pub struct VaultAddress {
    cold_storage: bitcoin::Address,
    hot_storage: bitcoin::Address,
    n_steps: u64,
    amount_step: CoinAmount,
    timeout: AnyRelTimeLock,
    mature: AnyRelTimeLock,
}

impl From<VaultAddress> for Vault {
    fn from(v: VaultAddress) -> Self {
        Vault {
            cold_storage: Rc::new({
                let cs = v.cold_storage.clone();
                move |_a, _ctx| Ok(Compiled::from_address(cs.clone(), None))
            }),
            hot_storage: v.hot_storage,
            n_steps: v.n_steps,
            amount_step: v.amount_step,
            timeout: v.timeout,
            mature: v.mature,
        }
    }
}

/// A specialization of `Vault` where cold storage is a tree payment to a `bitcoin::Address`
/// split up based on a max amount per address
#[derive(JsonSchema, Deserialize)]
pub struct VaultTree {
    cold_storage: bitcoin::Address,
    max_per_address: CoinAmount,
    radix: usize,
    hot_storage: bitcoin::Address,
    n_steps: u64,
    amount_step: CoinAmount,
    timeout: AnyRelTimeLock,
    mature: AnyRelTimeLock,
}

impl TryFrom<VaultTree> for Vault {
    type Error = CompilationError;
    fn try_from(v: VaultTree) -> Result<Self, CompilationError> {
        Ok(Vault {
            cold_storage: Rc::new({
                let cs = v.cold_storage.clone();
                let max: bitcoin::Amount = bitcoin::Amount::try_from(v.max_per_address)
                    .map_err(|_| CompilationError::TerminateCompilation)?;
                let rad = v.radix;
                move |a, ctx| {
                    let mut amt: bitcoin::Amount = bitcoin::Amount::try_from(a)
                        .map_err(|_| CompilationError::TerminateCompilation)?;
                    let mut pmts = vec![];
                    while amt > max {
                        pmts.push(super::treepay::Payment {
                            amount: max.into(),
                            address: cs.clone(),
                        });
                        amt -= max;
                    }
                    if amt > bitcoin::Amount::from_sat(0) {
                        pmts.push(super::treepay::Payment {
                            amount: max.into(),
                            address: cs.clone(),
                        });
                    }
                    ctx.compile(super::treepay::TreePay {
                        participants: pmts,
                        radix: rad,
                    })
                }
            }),
            hot_storage: v.hot_storage,
            n_steps: v.n_steps,
            amount_step: v.amount_step,
            timeout: v.timeout,
            mature: v.mature,
        })
    }
}
